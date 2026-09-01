//! Symmetric encryption for the whole network layer, keyed off the lesson PIN —
//! no certificates, no separate key exchange. The teacher's control channel Hello
//! handshake hands out a random per-connection salt in the clear (see
//! `ServerToClient::Welcome`); both sides then derive the same key locally from
//! `(pin, salt)` via [`derive_key`], since only someone who already knows the PIN
//! can do that. Everything on the wire after that point — every TCP control frame,
//! and every UDP audio packet on every port (broadcast, peer-to-peer, individual
//! listen-in, intercom) — is wrapped with [`encrypt`]/[`decrypt`].
//!
//! Group (peer-to-peer) audio never goes through the teacher, so there's no shared
//! per-pair salt for it the way there is for teacher<->student traffic: each
//! student instead always encrypts what *they* send with their own connection key,
//! and a receiving peer decrypts using the sender's key, derived from the sender's
//! salt (relayed over the already-encrypted control channel in
//! [`crate::GroupPeer::salt`]). Deriving that only requires the shared class PIN —
//! which every legitimate group member already typed in themselves — plus the
//! sender's public salt, so this never needs a direct student-to-student handshake.

use anyhow::{ensure, Context, Result};
use chacha20poly1305::aead::{Aead, Generate, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use sha2::{Digest, Sha256};

pub const SALT_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;
/// AEAD authentication tag length ChaCha20Poly1305 appends to every ciphertext —
/// used only to size-check incoming packets before bothering to decrypt them.
pub const TAG_LEN: usize = 16;

pub type Salt = [u8; SALT_LEN];
pub type SessionKey = [u8; 32];

/// Same cheap key-stretching approach as the teacher password gate
/// (`teacher::auth::hash_password`) and for the same reason: a 6-digit lesson PIN
/// is low-entropy, so anyone who captured a handshake's salt off the LAN could
/// otherwise brute-force it offline in an eyeblink. Re-hashing this many times
/// costs a connection setup nothing noticeable while raising that cost a lot. Runs
/// once per connection (or once per group peer), never per packet, so its cost
/// never touches the audio hot path.
const KDF_ROUNDS: u32 = 200_000;

pub fn generate_salt() -> Salt {
    let mut salt = [0u8; SALT_LEN];
    // Reuses the same CSPRNG this crate already pulls in for key/nonce
    // generation (`Generate`, backed by `getrandom`) instead of adding a second
    // RNG dependency just to fill 16 bytes.
    let random: [u8; 32] = Key::generate().into();
    salt.copy_from_slice(&random[..SALT_LEN]);
    salt
}

/// Derives a 256-bit session key from the lesson PIN and a salt. Deterministic:
/// the same `(pin, salt)` pair always yields the same key, which is exactly what
/// lets both sides of a handshake compute it independently.
pub fn derive_key(pin: &str, salt: &Salt) -> SessionKey {
    let mut digest: [u8; 32] = Sha256::new().chain_update(salt).chain_update(pin.as_bytes()).finalize().into();
    for _ in 1..KDF_ROUNDS {
        digest = Sha256::new().chain_update(digest).chain_update(salt).chain_update(pin.as_bytes()).finalize().into();
    }
    digest
}

/// Encrypts `plaintext` with a fresh random nonce, returning `nonce || ciphertext
/// || tag`. UDP packets can be lost or reordered with no retransmit to fall back
/// on, so the nonce rides along with every single message/packet rather than
/// being tracked as sender-side counter state — losing one packet must never
/// desynchronize the rest of the stream.
pub fn encrypt(key: &SessionKey, plaintext: &[u8]) -> Vec<u8> {
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let nonce = Nonce::generate();
    let ciphertext = cipher
        .encrypt(&nonce, plaintext)
        .expect("chacha20poly1305 encryption cannot fail for these inputs");
    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    out
}

/// Reverses [`encrypt`]. Fails both on a too-short/malformed packet and on a
/// packet that doesn't authenticate under `key` — the latter covers both a
/// wrong/absent key (e.g. a wrong PIN) and any in-transit tampering, and
/// deliberately isn't distinguished from the former: an attacker on the LAN
/// shouldn't learn anything from *how* their guess failed.
pub fn decrypt(key: &SessionKey, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(data.len() >= NONCE_LEN + TAG_LEN, "packet too short to be a valid encrypted frame");
    let (nonce_bytes, ciphertext) = data.split_at(NONCE_LEN);
    let cipher = ChaCha20Poly1305::new(&Key::from(*key));
    let nonce = Nonce::try_from(nonce_bytes).context("malformed nonce")?;
    cipher
        .decrypt(&nonce, ciphertext)
        .map_err(|_| anyhow::anyhow!("decryption failed: wrong key or corrupted/tampered packet"))
}
