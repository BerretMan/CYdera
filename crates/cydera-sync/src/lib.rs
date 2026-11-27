//! Synchro pair-à-pair chiffrée entre appareils appairés, sur iroh (QUIC).
//!
//! - `peers`  : liste de confiance (allowlist) des appareils appairés ;
//! - `wire`   : protocole d'échange du journal (`cydera/sync/1`) ;
//! - `engine` : point de terminaison iroh, acceptation (filtrée) et synchro sortante.

pub mod engine;
pub mod pairing;
pub mod peers;
pub mod wire;

pub use engine::{Discovery, EngineConfig, SyncEngine, SyncReport, load_or_create_key};
pub use iroh::{EndpointAddr, EndpointId};
pub use pairing::{Credentials, Decision, PairOutcome, PairRequest, PairTicket};
pub use peers::{Peer, PeerList};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("appareil non appairé")]
    Untrusted,
    #[error("réseau : {0}")]
    Net(String),
    #[error("protocole : {0}")]
    Proto(String),
    #[error("appairage refusé : {0}")]
    Pairing(String),
    #[error("délai dépassé")]
    Timeout,
    #[error("stockage : {0}")]
    Io(#[from] std::io::Error),
}
