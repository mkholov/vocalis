use anyhow::{Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Generous enough for a pushed worksheet/audio file in one shot, small enough to
/// bound a malicious/corrupt length prefix.
const MAX_MESSAGE_LEN: u32 = 128 * 1024 * 1024;

/// Writes a length-prefixed, bincode-encoded message to an async stream.
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

/// Reads a length-prefixed, bincode-encoded message from an async stream.
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
