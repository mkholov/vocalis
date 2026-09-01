use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::crypto::{self, SessionKey};

/// Generous enough for a pushed worksheet/audio file in one shot, small enough to
/// bound a malicious/corrupt length prefix.
const MAX_MESSAGE_LEN: u32 = 128 * 1024 * 1024;

/// Writes a length-prefixed, bincode-encoded message to an async stream. Used only
/// for the two messages exchanged before a session key exists — `Hello` and the
/// `Welcome`/`Rejected` reply to it (see [`write_message_encrypted`] for
/// everything after).
pub async fn write_message<T, W>(writer: &mut W, msg: &T) -> Result<()>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let bytes = bincode::serialize(msg).context("serializing message")?;
    let len = u32::try_from(bytes.len()).context("message too large")?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&bytes).await?;
    writer.flush().await?;
    Ok(())
}

/// Reads a length-prefixed, bincode-encoded message from an async stream. See
/// [`write_message`].
pub async fn read_message<T, R>(reader: &mut R) -> Result<T>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    anyhow::ensure!(len <= MAX_MESSAGE_LEN, "message length {len} exceeds max");
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    bincode::deserialize(&buf).context("deserializing message")
}

/// Same framing as [`write_message`], but the bincode payload is wrapped in
/// ChaCha20Poly1305 (`crypto::encrypt`) under the connection's session key before
/// the length prefix goes out — the length prefix itself covers the encrypted
/// bytes, so no framing detail leaks which plaintext message type/size this was
/// beyond what the ciphertext length already reveals. Used for every control-
/// channel message once the PIN handshake has established a shared key.
pub async fn write_message_encrypted<T, W>(writer: &mut W, msg: &T, key: &SessionKey) -> Result<()>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let bytes = bincode::serialize(msg).context("serializing message")?;
    let ciphertext = crypto::encrypt(key, &bytes);
    let len = u32::try_from(ciphertext.len()).context("message too large")?;
    writer.write_all(&len.to_be_bytes()).await?;
    writer.write_all(&ciphertext).await?;
    writer.flush().await?;
    Ok(())
}

/// Reverses [`write_message_encrypted`].
pub async fn read_message_encrypted<T, R>(reader: &mut R, key: &SessionKey) -> Result<T>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf);
    anyhow::ensure!(len <= MAX_MESSAGE_LEN, "message length {len} exceeds max");
    let mut buf = vec![0u8; len as usize];
    reader.read_exact(&mut buf).await?;
    let plaintext = crypto::decrypt(key, &buf).context("decrypting message")?;
    bincode::deserialize(&plaintext).context("deserializing message")
}
