use crate::protocol::{DiscoveryAnnounce, DISCOVERY_MAGIC, DISCOVERY_PORT};
use anyhow::Result;
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// Broadcasts a `DiscoveryAnnounce` on the LAN every 2 seconds until cancelled.
/// Run this as a background task on the teacher console.
pub async fn run_teacher_announcer(teacher_name: String, control_port: u16) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", 0)).await?;
    socket.set_broadcast(true)?;
    let announce = DiscoveryAnnounce {
        teacher_name,
        control_port,
    };
    let mut payload = DISCOVERY_MAGIC.to_vec();
    payload.extend(bincode::serialize(&announce)?);
    let dest: SocketAddr = ([255, 255, 255, 255], DISCOVERY_PORT).into();
    loop {
        socket.send_to(&payload, dest).await?;
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// Listens for teacher announcements on the LAN and forwards each sighting
/// (teacher's IP, announced info) down `tx`. Run this as a background task
/// on the student client.
pub async fn run_discovery_listener(
    tx: mpsc::UnboundedSender<(SocketAddr, DiscoveryAnnounce)>,
) -> Result<()> {
    let socket = UdpSocket::bind(("0.0.0.0", DISCOVERY_PORT)).await?;
    let mut buf = [0u8; 2048];
    loop {
        let (len, from) = socket.recv_from(&mut buf).await?;
        if len <= DISCOVERY_MAGIC.len() || &buf[..DISCOVERY_MAGIC.len()] != DISCOVERY_MAGIC {
            continue;
        }
        if let Ok(announce) = bincode::deserialize::<DiscoveryAnnounce>(&buf[DISCOVERY_MAGIC.len()..len])
        {
            if tx.send((from, announce)).is_err() {
                return Ok(());
            }
        }
    }
}
