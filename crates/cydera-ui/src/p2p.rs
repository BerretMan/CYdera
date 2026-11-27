//! Service P2P de l'application : démarre le moteur iroh en arrière-plan, synchronise
//! silencieusement avec les appareils appairés et expose l'état à l'interface.
//! Rien ici ne bloque le thread d'interface : tout passe par `tokio` + des états partagés.
use cydera_core::journal::Journal;
use cydera_core::platform;
use cydera_sync::{
    Discovery, EndpointAddr, EndpointId, EngineConfig, PairOutcome, PairRequest, PairTicket, Peer, PeerList, SyncEngine, load_or_create_key,
};
use eframe::egui;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Durée de validité d'un ticket d'appairage.
pub const TICKET_TTL: Duration = Duration::from_secs(300);
const SYNC_EVERY: Duration = Duration::from_secs(60);
const RETRY_AFTER_ERROR: Duration = Duration::from_secs(30);
const AFTER_LOCAL_CHANGE: Duration = Duration::from_secs(3);

#[derive(Default)]
struct State {
    start_error: Option<String>,
    syncing: bool,
    /// Dernière synchro réussie : (nom du pair, instant).
    last_ok: Option<(String, Instant)>,
    last_err: Option<String>,
    pair_result: Option<Result<PairOutcome, String>>,
    pairing_busy: bool,
}

/// Ce que l'interface affiche dans la barre d'état.
#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Moteur pas encore prêt.
    Starting,
    Unavailable(String),
    NoPeers,
    Syncing,
    Synced {
        name: String,
        age: Duration,
    },
    Offline {
        name: String,
        why: String,
    },
}

pub struct P2p {
    rt: tokio::runtime::Handle,
    ctx: egui::Context,
    dir: Option<std::path::PathBuf>,
    started: AtomicBool,
    engine: Arc<Mutex<Option<Arc<SyncEngine>>>>,
    peers: Arc<Mutex<PeerList>>,
    journal: Arc<Mutex<Journal>>,
    changed: Arc<AtomicBool>,
    st: Arc<Mutex<State>>,
    requests: Arc<Mutex<VecDeque<PairRequest>>>,
    last_attempt: Option<Instant>,
    last_version: u64,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn version_sum(j: &Mutex<Journal>) -> u64 {
    lock(j).version().values().sum()
}

static STARTUP_TICKET: Mutex<Option<String>> = Mutex::new(None);

/// Ticket fourni par la plateforme au lancement (intent Android) ; l'interface le pré-remplit.
pub fn set_startup_ticket(t: String) {
    *lock(&STARTUP_TICKET) = Some(t);
}

pub fn take_startup_ticket() -> Option<String> {
    lock(&STARTUP_TICKET).take()
}

/// Nom de cet appareil, montré aux autres (« PC de Lucas » se règle plus tard dans les réglages).
pub fn default_device_name() -> String {
    if cfg!(target_os = "android") {
        return "Téléphone".to_string();
    }
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "PC".to_string())
}

impl P2p {
    /// Prépare le service. Le moteur réseau, lui, ne démarre que s'il est utile (voir `ensure_started`) :
    /// tant qu'aucun appareil n'est associé et que la fenêtre « Appareils » n'a pas été ouverte,
    /// l'appli ne publie aucune adresse et n'ouvre aucune connexion P2P.
    pub fn start(rt: tokio::runtime::Handle, ctx: egui::Context, journal: Arc<Mutex<Journal>>) -> Self {
        let dir = platform::data_dir().map(|d| d.join("cydera"));
        let peers = Arc::new(Mutex::new(match &dir {
            Some(d) => PeerList::load(d.join("peers.json")),
            None => PeerList::in_memory(),
        }));
        let me = Self {
            rt,
            ctx,
            dir,
            started: AtomicBool::new(false),
            engine: Arc::default(),
            peers,
            last_version: version_sum(&journal),
            journal,
            changed: Arc::default(),
            st: Arc::default(),
            requests: Arc::default(),
            last_attempt: None,
        };
        if !lock(&me.peers).peers.is_empty() {
            me.ensure_started();
        }
        me
    }

    /// Démarre le moteur (une seule fois, en arrière-plan).
    pub fn ensure_started(&self) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let (engine, st, changed, requests) = (self.engine.clone(), self.st.clone(), self.changed.clone(), self.requests.clone());
        let (ctx, journal, peers, dir) = (self.ctx.clone(), self.journal.clone(), self.peers.clone(), self.dir.clone());
        self.rt.spawn(async move {
            let started = async {
                let dir = dir.ok_or("dossier de données introuvable")?;
                let secret_key = load_or_create_key(&dir.join("iroh.key")).map_err(|e| e.to_string())?;
                let on_change: Arc<dyn Fn() + Send + Sync> = {
                    let (changed, ctx) = (changed.clone(), ctx.clone());
                    Arc::new(move || {
                        changed.store(true, Ordering::SeqCst);
                        ctx.request_repaint();
                    })
                };
                SyncEngine::start(EngineConfig { secret_key, journal, peers, on_change, discovery: Discovery::Internet })
                    .await
                    .map_err(|e| e.to_string())
            };
            match started.await {
                Ok(e) => {
                    let mut rx = e.take_pair_requests();
                    *lock(&engine) = Some(Arc::new(e));
                    // Relais vers l'interface : une demande entrante réveille la fenêtre même au repos.
                    let ctx2 = ctx.clone();
                    tokio::spawn(async move {
                        while let Some(r) = rx.recv().await {
                            lock(&requests).push_back(r);
                            ctx2.request_repaint();
                        }
                    });
                }
                Err(e) => lock(&st).start_error = Some(e),
            }
            ctx.request_repaint();
        });
    }

    fn engine(&self) -> Option<Arc<SyncEngine>> {
        lock(&self.engine).clone()
    }

    pub fn ready(&self) -> bool {
        lock(&self.engine).is_some()
    }

    /// `true` (une seule fois) quand des événements distants ont été intégrés au journal.
    pub fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::SeqCst)
    }

    pub fn peers(&self) -> Vec<Peer> {
        lock(&self.peers).peers.clone()
    }

    pub fn forget(&self, id: &str) {
        let mut p = lock(&self.peers);
        if p.remove(id) {
            let _ = p.save();
        }
    }

    // ---------- synchro silencieuse ----------

    /// À appeler à chaque frame : lance une synchro en arrière-plan quand c'est utile
    /// (démarrage, modification locale, ou périodiquement), sans jamais bloquer l'interface.
    pub fn tick(&mut self) {
        if !self.ready() || lock(&self.st).syncing || lock(&self.peers).peers.is_empty() {
            return;
        }
        let v = version_sum(&self.journal);
        let local_change = v != self.last_version;
        let (since, had_err) = (self.last_attempt.map(|t| t.elapsed()), lock(&self.st).last_err.is_some());
        let due = match since {
            None => true,
            Some(s) if local_change => s >= AFTER_LOCAL_CHANGE,
            Some(s) => s >= if had_err { RETRY_AFTER_ERROR } else { SYNC_EVERY },
        };
        if due {
            self.sync_now();
        }
    }

    pub fn sync_now(&mut self) {
        let Some(engine) = self.engine() else { return };
        if std::mem::replace(&mut lock(&self.st).syncing, true) {
            return;
        }
        self.last_attempt = Some(Instant::now());
        self.last_version = version_sum(&self.journal);
        let (st, peers, ctx) = (self.st.clone(), self.peers.clone(), self.ctx.clone());
        self.rt.spawn(async move {
            let list = lock(&peers).peers.clone();
            let (mut ok, mut err) = (None, None);
            for p in list {
                let Ok(id) = p.id.parse::<EndpointId>() else { continue };
                match engine.sync_with(EndpointAddr::new(id)).await {
                    Ok(_) => ok = Some(p.name.clone()),
                    Err(e) => err = Some(format!("{} : {e}", p.name)),
                }
            }
            let mut s = lock(&st);
            s.syncing = false;
            match (ok, err) {
                (Some(name), _) => {
                    s.last_ok = Some((name, Instant::now()));
                    s.last_err = None;
                }
                (None, e) => s.last_err = e,
            }
            drop(s);
            ctx.request_repaint();
        });
    }

    pub fn status(&self) -> Status {
        let s = lock(&self.st);
        if let Some(e) = &s.start_error {
            return Status::Unavailable(e.clone());
        }
        if !self.started.load(Ordering::SeqCst) {
            return Status::NoPeers;
        }
        if !self.ready() {
            return Status::Starting;
        }
        let peers = lock(&self.peers);
        let Some(first) = peers.peers.first() else { return Status::NoPeers };
        if s.syncing {
            return Status::Syncing;
        }
        if let Some(why) = &s.last_err {
            return Status::Offline { name: first.name.clone(), why: why.clone() };
        }
        match &s.last_ok {
            Some((name, at)) => Status::Synced { name: name.clone(), age: at.elapsed() },
            None => Status::Syncing,
        }
    }

    // ---------- appairage ----------

    /// Côté PC : ouvre une fenêtre d'appairage de 5 minutes.
    pub fn start_pairing(&self, device_name: &str) -> Option<(PairTicket, Instant)> {
        let t = self.engine()?.start_pairing(device_name, TICKET_TTL);
        Some((t, Instant::now() + TICKET_TTL))
    }

    pub fn cancel_pairing(&self) {
        if let Some(e) = self.engine() {
            e.cancel_pairing();
        }
    }

    /// Côté PC : prochaine demande d'appairage à confirmer par l'utilisateur.
    pub fn poll_pair_request(&self) -> Option<PairRequest> {
        lock(&self.requests).pop_front()
    }

    /// Côté téléphone : appaire avec le PC décrit par le ticket collé ou scanné.
    pub fn pair_with_text(&self, text: &str, my_name: &str) {
        let (Some(engine), st, ctx) = (self.engine(), self.st.clone(), self.ctx.clone()) else {
            lock(&self.st).pair_result = Some(Err("moteur P2P pas encore prêt".into()));
            return;
        };
        let ticket = match PairTicket::from_text(text) {
            Ok(t) => t,
            Err(e) => {
                lock(&st).pair_result = Some(Err(e.to_string()));
                return;
            }
        };
        lock(&st).pairing_busy = true;
        let name = my_name.to_string();
        self.rt.spawn(async move {
            let r = engine.pair_with(&ticket, &name).await.map_err(|e| e.to_string());
            let mut s = lock(&st);
            s.pairing_busy = false;
            s.pair_result = Some(r);
            drop(s);
            ctx.request_repaint();
        });
    }

    pub fn pairing_busy(&self) -> bool {
        lock(&self.st).pairing_busy
    }

    /// Résultat d'un appairage lancé par `pair_with_text` (consommé une fois).
    pub fn take_pair_result(&self) -> Option<Result<PairOutcome, String>> {
        lock(&self.st).pair_result.take()
    }

    /// Code de comparaison que le téléphone affiche pendant que le PC confirme.
    pub fn code_for(&self, ticket_text: &str) -> Option<String> {
        let t = PairTicket::from_text(ticket_text).ok()?;
        t.confirmation_code(&self.engine()?.id()).ok()
    }
}
