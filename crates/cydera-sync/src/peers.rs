//! Liste de confiance : seuls ces appareils peuvent se connecter et synchroniser.
use iroh::EndpointId;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Peer {
    /// Clé publique (EndpointId) en texte.
    pub id: String,
    /// Nom affiché (« PC de Lucas »).
    pub name: String,
    /// Date d'appairage (ms depuis l'epoch).
    pub added_ms: i64,
    /// Dernière synchro réussie (ms depuis l'epoch).
    #[serde(default)]
    pub last_sync_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct PeerList {
    pub peers: Vec<Peer>,
    #[serde(skip)]
    path: Option<PathBuf>,
}

impl PeerList {
    /// Charge la liste (vide si le fichier n'existe pas) ; `save` y réécrira.
    pub fn load(path: PathBuf) -> Self {
        let mut l: PeerList = std::fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        l.path = Some(path);
        l
    }

    /// Liste en mémoire seulement (tests).
    pub fn in_memory() -> Self {
        Self::default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(p) = &self.path else { return Ok(()) };
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, p)
    }

    pub fn is_trusted(&self, id: &EndpointId) -> bool {
        let id = id.to_string();
        self.peers.iter().any(|p| p.id == id)
    }

    /// Ajoute (ou renomme) un appareil.
    pub fn add(&mut self, id: &EndpointId, name: &str) {
        let (idt, now) = (id.to_string(), chrono::Utc::now().timestamp_millis());
        match self.peers.iter_mut().find(|p| p.id == idt) {
            Some(p) => p.name = name.to_string(),
            None => self.peers.push(Peer { id: idt, name: name.to_string(), added_ms: now, last_sync_ms: None }),
        }
    }

    pub fn remove(&mut self, id: &str) -> bool {
        let n = self.peers.len();
        self.peers.retain(|p| p.id != id);
        self.peers.len() != n
    }

    pub fn mark_synced(&mut self, id: &EndpointId) {
        let idt = id.to_string();
        if let Some(p) = self.peers.iter_mut().find(|p| p.id == idt) {
            p.last_sync_ms = Some(chrono::Utc::now().timestamp_millis());
        }
    }
}
