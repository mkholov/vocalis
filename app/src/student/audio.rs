use std::collections::{HashMap, VecDeque};
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use lingua_common::{
    crypto, encode_audio_packet, new_decoder, new_encoder, split_audio_packet, Resampler,
    SequenceTracker, FRAME_SAMPLES, MIC_PORT, OPUS_SAMPLE_RATE, PEER_PORT, SCREEN_AUDIO_PORT,
    TEACHER_INTERCOM_PORT,
};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

use super::state::AppState;

/// Queried once at startup and threaded through every decode path, so audio is
/// resampled straight to the speaker's native rate as it arrives instead of being
/// re-resampled from scratch (with a phase reset, i.e. an audible click) on every
/// single output callback.
pub fn default_output_sample_rate() -> u32 {
    cpal::default_host()
        .default_output_device()
        .and_then(|d| d.default_output_config().ok())
        .map(|c| c.sample_rate().0)
        .unwrap_or(OPUS_SAMPLE_RATE)
}

/// Caps a per-source queue at ~0.5s of audio so a stalled source can't build up latency.
fn max_queue_samples(rate: u32) -> usize {
    rate as usize / 2
}

/// All audio sources currently feeding the speakers, already resampled to the output
/// device's native rate: the teacher's broadcast (if any), the teacher's private
/// intercom audio (if any), plus one queue per group member — additively mixed at
/// playback time.
#[derive(Default)]
pub struct MixState {
    pub broadcast: VecDeque<i16>,
    /// The teacher's private intercom audio, kept in its own queue (rather than
    /// folded into `broadcast`) purely so it can be identified/measured separately
    /// if ever needed — it's mixed in exactly the same way at playback time.
    pub intercom: VecDeque<i16>,
    pub peers: HashMap<SocketAddr, VecDeque<i16>>,
    /// The teacher's *system* audio (screen-demo sound, not their mic) — its own
    /// queue so it stays distinct from `broadcast` even though both can be live
    /// during a demo where the teacher also talks over it.
    pub screen_demo_audio: VecDeque<i16>,
}

pub type SharedMix = Arc<Mutex<MixState>>;

pub fn new_mix_state() -> SharedMix {
    Arc::new(Mutex::new(MixState::default()))
}

fn push_capped(q: &mut VecDeque<i16>, samples: &[i16], device_rate: u32) {
    q.extend(samples);
    let max_len = max_queue_samples(device_rate);
    while q.len() > max_len {
        q.pop_front();
    }
}

static OUTPUT_STARTED: AtomicBool = AtomicBool::new(false);
/// Current mic input level (RMS, fixed-point *1000), read by the GUI for a VU meter.
pub static MIC_LEVEL_MILLIS: AtomicI32 = AtomicI32::new(0);

/// Spawns a dedicated OS thread that owns the speaker output stream for the lifetime
/// of the process and continuously mixes every active source in `mix`. Every source
/// has already been resampled to the device's native rate before landing in `mix`,
/// so this thread only ever mixes and channel-duplicates — no resampling here.
pub fn ensure_output_started(mix: SharedMix) {
    if OUTPUT_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        if let Err(e) = run_output_stream(mix) {
            tracing::warn!("audio output stream failed: {e:#}");
        }
    });
}

fn pull_mixed(mix: &SharedMix, count: usize) -> Vec<i16> {
    let mut state = mix.lock().unwrap();
    (0..count)
        .map(|_| {
            let mut acc: i32 = state.broadcast.pop_front().unwrap_or(0) as i32;
            acc += state.intercom.pop_front().unwrap_or(0) as i32;
            acc += state.screen_demo_audio.pop_front().unwrap_or(0) as i32;
            for q in state.peers.values_mut() {
                acc += q.pop_front().unwrap_or(0) as i32;
            }
            acc.clamp(i16::MIN as i32, i16::MAX as i32) as i16
        })
        .collect()
}

fn run_output_stream(mix: SharedMix) -> Result<()> {
    let host = cpal::default_host();
    let device = host
        .default_output_device()
        .context("no default output device")?;
    // The device's own mix format (commonly 2ch/48kHz) — every source in `mix` has
    // already been resampled to this exact rate as it was decoded.
    let config = device
        .default_output_config()
        .context("no default output config")?;
    let channels = config.channels() as usize;
    let sample_format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let err_fn = |err| tracing::warn!("audio output stream error: {err}");

    let stream = match sample_format {
        cpal::SampleFormat::I16 => device.build_output_stream(
            &stream_config,
            move |data: &mut [i16], _: &cpal::OutputCallbackInfo| {
                let native = pull_mixed(&mix, data.len() / channels);
                for (frame, &s) in data.chunks_mut(channels).zip(native.iter()) {
                    frame.fill(s);
                }
            },
            err_fn,
            None,
        )?,
        _ => device.build_output_stream(
            &stream_config,
            move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                let native = pull_mixed(&mix, data.len() / channels);
                for (frame, &s) in data.chunks_mut(channels).zip(native.iter()) {
                    frame.fill(s as f32 / i16::MAX as f32);
                }
            },
            err_fn,
            None,
        )?,
    };
    stream.play()?;
    std::thread::park();
    Ok(())
}

/// Listens for the teacher's mic broadcast, conceals dropped packets, resamples to
/// the speaker's native rate and mixes the result into `mix.broadcast`.
pub async fn run_mic_broadcast_receiver(state: AppState, mix: SharedMix, output_rate: u32) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", MIC_PORT)).await?;
    let mut decoder = new_decoder()?;
    let mut tracker = SequenceTracker::new();
    let mut resampler = Resampler::new(OPUS_SAMPLE_RATE, output_rate);
    let mut buf = [0u8; 4096];
    loop {
        let (len, _from) = socket.recv_from(&mut buf).await?;
        let Some(key) = state.lock().unwrap().session_key else {
            continue;
        };
        let Ok(plaintext) = crypto::decrypt(&key, &buf[..len]) else {
            continue;
        };
        let Some((seq, payload)) = split_audio_packet(&plaintext) else {
            continue;
        };
        let samples = tracker.decode(&mut decoder, seq, payload);
        if samples.is_empty() {
            continue;
        }
        ensure_output_started(mix.clone());
        // "Model pronunciation": while the teacher is playing a material, cache
        // what's coming through (pre-resample, at the wire's own OPUS_SAMPLE_RATE —
        // there's no more fidelity to gain from resampling up) so the student can
        // play it back as a reference after recording their own attempt. Same tap
        // idea as the mic-recording feature, just on the receive side.
        if let Some(reference) = state.lock().unwrap().reference_capture.as_mut() {
            reference.samples.extend_from_slice(&samples);
        }
        let resampled = resampler.push(&samples);
        push_capped(&mut mix.lock().unwrap().broadcast, &resampled, output_rate);
    }
}

/// Listens for the teacher's private intercom audio (addressed to this student
/// only), conceals dropped packets, resamples to the speaker's native rate and
/// mixes the result into `mix.intercom`. Always bound, exactly like the broadcast
/// receiver above — it's simply idle whenever the teacher isn't privately talking
/// to this student, no start/stop signaling needed on the socket itself.
pub async fn run_intercom_receiver(state: AppState, mix: SharedMix, output_rate: u32) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", TEACHER_INTERCOM_PORT)).await?;
    let mut decoder = new_decoder()?;
    let mut tracker = SequenceTracker::new();
    let mut resampler = Resampler::new(OPUS_SAMPLE_RATE, output_rate);
    let mut buf = [0u8; 4096];
    loop {
        let (len, _from) = socket.recv_from(&mut buf).await?;
        let Some(key) = state.lock().unwrap().session_key else {
            continue;
        };
        let Ok(plaintext) = crypto::decrypt(&key, &buf[..len]) else {
            continue;
        };
        let Some((seq, payload)) = split_audio_packet(&plaintext) else {
            continue;
        };
        let samples = tracker.decode(&mut decoder, seq, payload);
        if samples.is_empty() {
            continue;
        }
        ensure_output_started(mix.clone());
        let resampled = resampler.push(&samples);
        push_capped(&mut mix.lock().unwrap().intercom, &resampled, output_rate);
    }
}

/// Listens for the teacher's system audio (screen-demo sound, not their mic —
/// see `teacher::system_audio`), conceals dropped packets, resamples to the
/// speaker's native rate and mixes the result into `mix.screen_demo_audio`.
/// Always bound, exactly like the broadcast/intercom receivers above — idle
/// (nothing arrives) whenever no teacher-sourced demo with system audio is
/// running, including on platforms where the teacher side never captures any
/// (there's nothing student-side that needs to know or care why).
pub async fn run_screen_audio_receiver(state: AppState, mix: SharedMix, output_rate: u32) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", SCREEN_AUDIO_PORT)).await?;
    let mut decoder = new_decoder()?;
    let mut tracker = SequenceTracker::new();
    let mut resampler = Resampler::new(OPUS_SAMPLE_RATE, output_rate);
    let mut buf = [0u8; 4096];
    loop {
        let (len, _from) = socket.recv_from(&mut buf).await?;
        let Some(key) = state.lock().unwrap().session_key else {
            continue;
        };
        let Ok(plaintext) = crypto::decrypt(&key, &buf[..len]) else {
            continue;
        };
        let Some((seq, payload)) = split_audio_packet(&plaintext) else {
            continue;
        };
        let samples = tracker.decode(&mut decoder, seq, payload);
        if samples.is_empty() {
            continue;
        }
        ensure_output_started(mix.clone());
        let resampled = resampler.push(&samples);
        push_capped(&mut mix.lock().unwrap().screen_demo_audio, &resampled, output_rate);
    }
}

/// Handles this student's own mic: resamples + Opus-encodes it once, then fans the
/// encoded frames out to every currently active destination (group peers, and/or the
/// teacher if listen-in is active). Also runs the receive side for group audio,
/// mixing each peer's decoded stream into its own `mix.peers` queue.
pub async fn run_outbound_and_group_audio(
    state: AppState,
    mix: SharedMix,
    mut mic_rx: mpsc::UnboundedReceiver<Vec<i16>>,
    mic_native_rate: u32,
    output_rate: u32,
) -> Result<()> {
    let socket = Arc::new(UdpSocket::bind(("0.0.0.0", PEER_PORT)).await?);
    let teacher_socket = Arc::new(UdpSocket::bind(("0.0.0.0", 0)).await?);

    // Receive side: decode incoming group-peer audio, one decoder/tracker/resampler
    // per sender so each peer's stream stays independently continuous.
    let recv_socket = socket.clone();
    let recv_mix = mix.clone();
    let recv_state = state.clone();
    tokio::spawn(async move {
        struct PeerRx {
            decoder: lingua_common::Decoder,
            tracker: SequenceTracker,
            resampler: Resampler,
        }
        let mut peers: HashMap<SocketAddr, PeerRx> = HashMap::new();
        let mut buf = [0u8; 4096];
        loop {
            match recv_socket.recv_from(&mut buf).await {
                Ok((len, from)) => {
                    // Each peer always encrypts what *they* send with their own
                    // session key (see `crypto`'s module doc comment) — so
                    // decrypting here means looking up *that peer's* key, derived
                    // from their salt when `JoinGroup` last arrived, not our own.
                    let Some(peer_key) = recv_state.lock().unwrap().peer_keys.get(&from).copied() else {
                        continue;
                    };
                    let Ok(plaintext) = crypto::decrypt(&peer_key, &buf[..len]) else {
                        continue;
                    };
                    let Some((seq, payload)) = split_audio_packet(&plaintext) else {
                        continue;
                    };
                    let peer = match peers.entry(from) {
                        std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
                        std::collections::hash_map::Entry::Vacant(e) => {
                            let Ok(decoder) = lingua_common::new_decoder() else {
                                continue;
                            };
                            e.insert(PeerRx {
                                decoder,
                                tracker: SequenceTracker::new(),
                                resampler: Resampler::new(OPUS_SAMPLE_RATE, output_rate),
                            })
                        }
                    };
                    let samples = peer.tracker.decode(&mut peer.decoder, seq, payload);
                    if samples.is_empty() {
                        continue;
                    }
                    ensure_output_started(recv_mix.clone());
                    let resampled = peer.resampler.push(&samples);
                    let mut guard = recv_mix.lock().unwrap();
                    let q = guard.peers.entry(from).or_default();
                    push_capped(q, &resampled, output_rate);
                }
                Err(_) => break,
            }
        }
    });

    // Send side: resample -> Opus encode -> fan out to whoever should hear us.
    let mut resampler = Resampler::new(mic_native_rate, OPUS_SAMPLE_RATE);
    let encoder = new_encoder()?;
    let mut pending: Vec<i16> = Vec::new();
    let mut seq: u32 = 0;

    while let Some(chunk) = mic_rx.recv().await {
        MIC_LEVEL_MILLIS.store(rms_millis(&chunk), Ordering::Relaxed);
        // Local self-recording taps the same raw, native-rate PCM already flowing
        // through this loop for the network pipeline — no second capture stream,
        // exactly the same mic feed used for the live broadcast.
        if let Some(recording) = state.lock().unwrap().recording.as_mut() {
            recording.samples.extend_from_slice(&chunk);
        }
        pending.extend(resampler.push(&chunk));
        while pending.len() >= FRAME_SAMPLES {
            let frame: Vec<i16> = pending.drain(..FRAME_SAMPLES).collect();
            let mut opus_buf = [0u8; 4000];
            let n = match encoder.encode(&frame, &mut opus_buf) {
                Ok(n) => n,
                Err(_) => continue,
            };
            let packet = encode_audio_packet(seq, &opus_buf[..n]);
            seq = seq.wrapping_add(1);

            let (peer_addrs, teacher_addr, upload_to_teacher, mic_locked, key) = {
                let guard = state.lock().unwrap();
                (
                    guard.peer_addrs.clone(),
                    guard.teacher_addr,
                    guard.uploading_to_teacher,
                    guard.mic_locked,
                    guard.session_key,
                )
            };
            // A teacher-issued mic lock silences transmission entirely — e.g. to keep
            // a test quiet — even if the student is still nominally grouped/listened to.
            if mic_locked {
                continue;
            }
            let Some(key) = key else { continue };
            // Everything we send — to group peers or to the teacher — is
            // encrypted once under our own session key (see `crypto`'s module doc
            // comment): peers derive it themselves from our relayed salt, and the
            // teacher already has it from our own handshake.
            let encrypted = crypto::encrypt(&key, &packet);
            for addr in peer_addrs {
                let _ = socket.send_to(&encrypted, addr).await;
            }
            if upload_to_teacher {
                if let Some(addr) = teacher_addr {
                    let _ = teacher_socket
                        .send_to(&encrypted, (addr, lingua_common::TEACHER_LISTEN_PORT))
                        .await;
                }
            }
        }
    }
    Ok(())
}

/// Reports the current mic level to the teacher a few times a second while connected,
/// so the class grid can show who's actually talking right now.
pub async fn run_level_telemetry(state: AppState) {
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        let sender = state.lock().unwrap().to_server.clone();
        if let Some(tx) = sender {
            let millis = MIC_LEVEL_MILLIS.load(Ordering::Relaxed);
            let _ = tx.send(lingua_common::ClientToServer::AudioLevel { millis });
        }
    }
}

fn rms_millis(samples: &[i16]) -> i32 {
    if samples.is_empty() {
        return 0;
    }
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = (sum_sq / samples.len() as f64).sqrt();
    ((rms / i16::MAX as f64) * 1000.0) as i32
}
