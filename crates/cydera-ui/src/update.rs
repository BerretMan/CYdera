//! Mises à jour côté interface : vérification périodique en arrière-plan, état affiché par le
//! bandeau, la page « Mise à jour » et l'écran « À propos », et installation.
//!
//! Sur PC, l'installation est faite ici (`cydera_core::update::installer_pc`). Sur Android,
//! l'hôte fournit l'installeur (`set_installer`, via `DownloadManager` + installeur système).
use cydera_core::update::{self as core_maj, Release};
use eframe::egui;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

/// Intervalle entre deux vérifications automatiques.
const PERIODE: Duration = Duration::from_secs(6 * 3600);

/// Avancement d'une installation déléguée à l'hôte.
pub enum Etat {
    Progres(u64, Option<u64>),
    /// L'installeur système est ouvert : la suite se passe hors de l'appli.
    Lance,
    Echec(String),
}

pub type Rappel = Box<dyn Fn(Etat) + Send>;
type Installer = Box<dyn Fn(String, Rappel) + Send + Sync>;
static INSTALLER: OnceLock<Installer> = OnceLock::new();

/// L'hôte (Android) installe lui-même le paquet à l'adresse donnée, en rendant compte via le rappel.
pub fn set_installer(f: impl Fn(String, Rappel) + Send + Sync + 'static) {
    let _ = INSTALLER.set(Box::new(f));
}

/// Avancement de l'installation, pour l'affichage.
#[derive(Clone, Debug, Default)]
pub enum Install {
    #[default]
    Rien,
    EnCours(u64, Option<u64>),
    /// PC : nouvel exécutable en place, à relancer.
    Prete(PathBuf),
    /// Android : installeur système ouvert.
    Lancee,
    Echec(String),
}

enum Msg {
    Verif(Result<Option<Release>, String>),
    Progres(u64, Option<u64>),
    #[cfg_attr(target_os = "android", allow(dead_code))] // installation PC seulement
    Prete(PathBuf),
    Lancee,
    Echec(String),
}

pub struct Maj {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    /// Version plus récente publiée (si connue).
    pub dispo: Option<Release>,
    pub verif_en_cours: bool,
    /// Résultat de la dernière vérification manuelle (« À jour », erreur…).
    pub message: Option<String>,
    manuel: bool,
    derniere: Option<Instant>,
    /// « Plus tard » : bandeau masqué jusqu'au prochain lancement.
    pub masque: bool,
    /// Page « Mise à jour » ouverte.
    pub show: bool,
    pub install: Install,
}

impl Default for Maj {
    fn default() -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            tx,
            rx,
            dispo: None,
            verif_en_cours: false,
            message: None,
            manuel: false,
            derniere: None,
            masque: false,
            show: false,
            install: Install::Rien,
        }
    }
}

impl Maj {
    /// Lance une vérification (`manuel` = demandée par l'utilisateur : le résultat est affiché).
    pub fn verifier(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context, manuel: bool) {
        if self.verif_en_cours {
            return;
        }
        self.verif_en_cours = true;
        self.manuel = manuel;
        self.derniere = Some(Instant::now());
        if manuel {
            self.message = None;
        }
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        rt.spawn(async move {
            let r = core_maj::derniere(&core_maj::version_actuelle()).await;
            let _ = tx.send(Msg::Verif(r));
            ctx.request_repaint();
        });
    }

    /// Vérification automatique : au lancement puis toutes les 6 h.
    pub fn auto(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context, actif: bool) {
        if actif && self.derniere.is_none_or(|t| t.elapsed() >= PERIODE) {
            self.verifier(rt, ctx, false);
        }
    }

    pub fn poll(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Verif(r) => {
                    self.verif_en_cours = false;
                    match r {
                        Ok(Some(rel)) => {
                            self.message = None;
                            self.dispo = Some(rel);
                        }
                        Ok(None) => {
                            self.dispo = None;
                            self.message = self.manuel.then(|| "Cydera est à jour.".to_string());
                        }
                        // En automatique, une erreur réseau reste silencieuse.
                        Err(e) => self.message = self.manuel.then_some(e),
                    }
                }
                Msg::Progres(a, b) => self.install = Install::EnCours(a, b),
                Msg::Prete(p) => self.install = Install::Prete(p),
                Msg::Lancee => self.install = Install::Lancee,
                Msg::Echec(e) => self.install = Install::Echec(e),
            }
        }
    }

    /// Le bandeau « nouvelle version » doit-il s'afficher ?
    pub fn bandeau(&self, ignoree: &str) -> Option<&Release> {
        let r = self.dispo.as_ref()?;
        (!self.masque && !self.show && r.version.to_string() != ignoree && matches!(self.install, Install::Rien)).then_some(r)
    }

    pub fn installation_en_cours(&self) -> bool {
        matches!(self.install, Install::EnCours(..))
    }

    /// Télécharge et installe la version disponible. Repli : page de la release dans le navigateur.
    pub fn installer(&mut self, rt: &tokio::runtime::Handle, ctx: &egui::Context) {
        let Some(rel) = self.dispo.clone() else { return };
        if self.installation_en_cours() {
            return;
        }
        self.install = Install::EnCours(0, rel.paquet().map(|a| a.taille));
        let (tx, ctx2) = (self.tx.clone(), ctx.clone());
        if let Some(inst) = INSTALLER.get() {
            let Some(apk) = rel.paquet() else {
                self.install = Install::Echec("Cette release ne contient pas d'APK.".into());
                return;
            };
            inst(
                apk.url.clone(),
                Box::new(move |e| {
                    let _ = tx.send(match e {
                        Etat::Progres(a, b) => Msg::Progres(a, b),
                        Etat::Lance => Msg::Lancee,
                        Etat::Echec(e) => Msg::Echec(e),
                    });
                    ctx2.request_repaint();
                }),
            );
            return;
        }
        #[cfg(not(target_os = "android"))]
        rt.spawn(async move {
            // Un message par pour-mille suffit à animer la barre.
            let dernier = std::sync::atomic::AtomicU64::new(u64::MAX);
            let (txp, ctxp) = (tx.clone(), ctx2.clone());
            let progres = move |a: u64, b: Option<u64>| {
                let pm = b.map_or(a >> 16, |t| a * 1000 / t.max(1));
                if dernier.swap(pm, std::sync::atomic::Ordering::Relaxed) != pm {
                    let _ = txp.send(Msg::Progres(a, b));
                    ctxp.request_repaint();
                }
            };
            let _ = tx.send(match core_maj::installer_pc(&rel, progres).await {
                Ok(p) => Msg::Prete(p),
                Err(e) => Msg::Echec(e),
            });
            ctx2.request_repaint();
        });
        #[cfg(target_os = "android")]
        {
            let _ = rt;
            self.install = Install::Echec("Installation indisponible sur cet appareil.".into());
        }
    }

    /// Page de la release dans le navigateur (repli manuel).
    pub fn ouvrir_page(&self, ctx: &egui::Context) {
        let url = self.dispo.as_ref().map_or_else(|| format!("https://github.com/{}/releases", core_maj::DEPOT), |r| r.page.clone());
        ctx.open_url(egui::OpenUrl::new_tab(url));
    }
}

/// PC : relance le nouvel exécutable puis ferme celui-ci.
pub fn relancer(ctx: &egui::Context, exe: &std::path::Path) {
    match std::process::Command::new(exe).spawn() {
        Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        Err(e) => eprintln!("relance de {} impossible : {e}", exe.display()),
    }
}
