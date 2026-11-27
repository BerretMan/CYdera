//! Protocole `cydera/sync/1` : échange du journal sur un flux bidirectionnel QUIC.
//!
//! ```text
//! initiateur                         répondeur
//!   Hello{vv}        ─────────────▶
//!                    ◀─────────────  Offer{vv, events}   (ce qu'il manque à l'initiateur)
//!   Push{events}     ─────────────▶                      (ce qu'il manque au répondeur)
//!                    ◀─────────────  Ack{applied}
//! ```
//! Messages : longueur (u32, gros-boutiste) + JSON. Le canal est chiffré et authentifié
//! par iroh ; l'identité du pair est vérifiée contre la liste de confiance avant tout échange.
use crate::SyncError;
use cydera_core::journal::{Event, VersionVector};
use iroh::endpoint::{RecvStream, SendStream};
use serde::{Deserialize, Serialize};

pub const ALPN: &[u8] = b"cydera/sync/1";
/// Taille maximale d'un message (protège la mémoire contre un pair défaillant).
pub const MAX_MSG: usize = 16 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    Hello { vv: VersionVector },
    Offer { vv: VersionVector, events: Vec<Event> },
    Push { events: Vec<Event> },
    Ack { applied: usize },
}

pub async fn send_msg(send: &mut SendStream, m: &Msg) -> Result<(), SyncError> {
    send_json(send, m).await
}

pub async fn recv_msg(recv: &mut RecvStream) -> Result<Msg, SyncError> {
    recv_json(recv).await
}

pub async fn send_json<T: Serialize>(send: &mut SendStream, m: &T) -> Result<(), SyncError> {
    let body = serde_json::to_vec(m).map_err(|e| SyncError::Proto(e.to_string()))?;
    if body.len() > MAX_MSG {
        return Err(SyncError::Proto("message trop volumineux".into()));
    }
    send.write_all(&(body.len() as u32).to_be_bytes()).await.map_err(|e| SyncError::Net(e.to_string()))?;
    send.write_all(&body).await.map_err(|e| SyncError::Net(e.to_string()))
}

pub async fn recv_json<T: serde::de::DeserializeOwned>(recv: &mut RecvStream) -> Result<T, SyncError> {
    let mut len = [0u8; 4];
    recv.read_exact(&mut len).await.map_err(|e| SyncError::Net(e.to_string()))?;
    let n = u32::from_be_bytes(len) as usize;
    if n > MAX_MSG {
        return Err(SyncError::Proto(format!("message de {n} octets refusé")));
    }
    let mut buf = vec![0u8; n];
    recv.read_exact(&mut buf).await.map_err(|e| SyncError::Net(e.to_string()))?;
    serde_json::from_slice(&buf).map_err(|e| SyncError::Proto(e.to_string()))
}

/// Écarte les événements manifestement invalides avant de les confier au journal.
pub fn sanitize(events: Vec<Event>) -> Vec<Event> {
    events.into_iter().filter(|e| !e.dev.is_empty() && e.dev.len() <= 64 && e.seq > 0).collect()
}
