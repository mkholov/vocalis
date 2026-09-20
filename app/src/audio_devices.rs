//! Shared `cpal` device enumeration/selection: lists devices for the Settings
//! screen's dropdowns, and resolves a chosen device name back to an actual
//! `cpal::Device` for every capture/playback pipeline (mic, listen-in,
//! speaker output) that needs to open one. Centralized here so all four of
//! those pipelines (teacher mic/listen-in, student mic/output) fall back to
//! the system default the same way when the configured device is missing,
//! rather than each reimplementing (and potentially disagreeing on) that
//! fallback.

use std::sync::OnceLock;

use anyhow::{Context, Result};
use cpal::traits::{DeviceTrait, HostTrait};

/// Windows only: makes sure cpal's process-wide device enumerator is created on a
/// thread that lives as long as the process — and returns the ordinary default host.
///
/// **Why.** cpal's WASAPI backend (0.15.x, and still upstream as of
/// RustAudio/cpal#1302) caches one `IMMDeviceEnumerator` in a `static`, created on
/// whichever thread touches cpal *first*, inside that thread's COM apartment (STA) —
/// and destroys that apartment (`CoUninitialize`) when the thread exits. The cached
/// pointer is then dangling for everyone else: the next enumeration from any other
/// thread reads a dead vtable and the process dies with `STATUS_ACCESS_VIOLATION
/// (0xc0000005)` — not an error, not an empty list, and independent of whether the
/// machine has any audio devices. It's invisible in the egui apps only because their
/// first cpal call happens on the never-exiting UI thread; anywhere the first call
/// lands on a short-lived thread (a libtest test thread, or the Tauri student mic
/// meter's dedicated thread) it's a crash waiting for the next caller.
///
/// **What.** Every path in this crate that can reach that static goes through this
/// module, so the first touch is made here, once, from a dedicated parked thread that
/// never exits; every later call — from any thread — then finds the enumerator
/// already created and homed in an apartment that outlives the process's use of it.
/// This is the "dedicated long-lived COM thread" option from that issue, and is
/// the same lifetime the egui apps already had by accident. A no-op everywhere but
/// Windows. (The home thread never pumps messages, which is fine here: cpal calls
/// the enumerator directly through the raw pointer, it never marshals into it.)
fn cpal_host() -> cpal::Host {
    if cfg!(windows) {
        start_cpal_home_thread();
    }
    cpal::default_host()
}

/// Not `#[cfg(windows)]` so it's compiled and unit-tested on every platform; only
/// [`cpal_host`] decides whether it runs.
fn start_cpal_home_thread() {
    static STARTED: OnceLock<()> = OnceLock::new();
    STARTED.get_or_init(|| {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let spawned = std::thread::Builder::new().name("cpal-com-home".into()).spawn(move || {
            // Any enumeration call creates cpal's global enumerator on *this* thread.
            let _ = cpal::default_host().default_input_device();
            let _ = ready_tx.send(());
            loop {
                std::thread::park();
            }
        });
        match spawned {
            // Wait until the enumerator exists, so no other thread can win the race to
            // create it first. If the home thread panicked on the way (e.g. COM init
            // failing), `ready_tx` is dropped and this returns instead of hanging.
            Ok(_) => {
                let _ = ready_rx.recv();
            }
            Err(e) => tracing::warn!("couldn't start the cpal COM home thread ({e}); audio device access may be unstable on Windows"),
        }
    });
}

/// Every input (microphone) device's name, in whatever order `cpal` reports
/// them, for the Settings screen's dropdown. Best-effort: a device whose name
/// can't be read is silently skipped rather than failing the whole list.
pub fn list_input_device_names() -> Vec<String> {
    cpal_host()
        .input_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// See [`list_input_device_names`].
pub fn list_output_device_names() -> Vec<String> {
    cpal_host()
        .output_devices()
        .map(|devices| devices.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Resolves `name` to an actual input device, falling back to the system
/// default if `name` is `None` or no longer matches any connected device
/// (unplugged, renamed by its driver, etc. since the setting was saved) —
/// a stale setting should never be the reason capture fails to start.
pub fn resolve_input_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal_host();
    if let Some(name) = name {
        match host.input_devices().ok().and_then(|mut devices| devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            Some(device) => return Ok(device),
            None => tracing::warn!("configured microphone '{name}' not found, falling back to the system default"),
        }
    }
    host.default_input_device().context("no default microphone found")
}

/// See [`resolve_input_device`].
pub fn resolve_output_device(name: Option<&str>) -> Result<cpal::Device> {
    let host = cpal_host();
    if let Some(name) = name {
        match host.output_devices().ok().and_then(|mut devices| devices.find(|d| d.name().map(|n| n == name).unwrap_or(false))) {
            Some(device) => return Ok(device),
            None => tracing::warn!("configured output device '{name}' not found, falling back to the system default"),
        }
    }
    host.default_output_device().context("no default output device found")
}

static CONFIGURED_OUTPUT_DEVICE: OnceLock<Option<String>> = OnceLock::new();

/// Configures which output device [`resolve_configured_output_device`] (and so
/// every playback pipeline that calls it — teacher listen-in, student speaker
/// output) resolves to from here on. Meant to be called exactly once, early
/// at launch right after loading `Settings` — matches
/// `default_output_sample_rate`'s existing "resolve once at startup, use for
/// the process's whole lifetime" shape, since the output stream itself is
/// also only ever lazily started once per process. A later setting change
/// takes effect the next time the app starts, not by hot-swapping a stream
/// that may already be running.
pub fn configure_output_device(name: Option<String>) {
    let _ = CONFIGURED_OUTPUT_DEVICE.set(name);
}

/// See [`configure_output_device`] and [`resolve_output_device`] (which this
/// falls back to the system default through, the same as every other
/// pipeline, if `configure_output_device` was never called or was called
/// with `None`).
pub fn resolve_configured_output_device() -> Result<cpal::Device> {
    let name = CONFIGURED_OUTPUT_DEVICE.get().cloned().flatten();
    resolve_output_device(name.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The home-thread path is Windows-only in practice (see [`cpal_host`]), but it's
    /// compiled everywhere, so this exercises it on every platform: racing first
    /// callers must all return (no deadlock on the `OnceLock`/ready handshake), the
    /// second call must be a cheap no-op, and the public API must still work after.
    /// What it can't show is the actual Windows crash fix — that's
    /// `tauri-app/src-tauri/tests/cpal_thread_lifetime.rs`, on the Windows CI runner.
    #[test]
    fn home_thread_startup_is_idempotent_and_race_free() {
        let racers: Vec<_> = (0..4).map(|_| std::thread::spawn(start_cpal_home_thread)).collect();
        for r in racers {
            r.join().expect("a racing first caller should return, not hang or panic");
        }
        start_cpal_home_thread();
        let _ = list_input_device_names();
        let _ = list_output_device_names();
    }
}
