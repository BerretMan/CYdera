use crate::cache::{Store, monday};
use crate::config::{Config, load_mail_password, load_password};
use crate::grades::Book;
use crate::mail::{self, Header};
use crate::model::{Cours, Kind, fmt_duration, now};
use crate::p2p::P2p;
use crate::resto::{self, Gauge, Live, RestoData};
use crate::study::{Chrono, fmt_hms};
use crate::ui::devices::{self, DevAction, DevicesUi};
use crate::ui::grades::{self as grades_ui, GradesUi};
use crate::ui::revisions::{self, ExamRef, RevUi};
use crate::ui::theme::{self, bold, regular};
use crate::ui::{self, Action, MOIS, capitalize, date_longue};
use chrono::{Datelike, Duration, NaiveDate};
use cydera_sync::{Credentials, Decision, PairRequest};
use eframe::egui::{self, Color32, CornerRadius, Key, Margin, RichText};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Instant;

/// Ouvre la jauge officielle du Crous dans une petite fenêtre « application » du navigateur
/// (secours quand la lecture directe de la jauge échoue).
fn open_gauge(ctx: &egui::Context, url: &str) {
    #[cfg(not(target_os = "android"))]
    for bin in ["google-chrome", "brave-browser", "chromium", "chromium-browser"] {
        let ok = std::process::Command::new(bin)
            .arg(format!("--app={url}"))
            .arg("--window-size=440,260")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok();
        if ok {
            return;
        }
    }
    ctx.open_url(egui::OpenUrl::new_tab(url));
}

/// « 1500 » → « 1 500 » (espace insécable comme séparateur de milliers).
fn milliers(n: u32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push('\u{00A0}');
        }
        out.push(c);
    }
    out
}

/// Contenu de la carte « fréquentation en direct » d'un resto.
fn live_gauge(ui: &mut egui::Ui, ctx: &egui::Context, r: &resto::Resto, live: Option<&Result<Live, String>>, page: bool) {
    let note = |ui: &mut egui::Ui, t: &str| {
        ui.add(egui::Label::new(RichText::new(t).font(regular(15.0)).color(theme::MUTED)).wrap());
    };
    match live {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Lecture de la jauge…").color(theme::MUTED));
            });
        }
        Some(Err(e)) => {
            ui.add(egui::Label::new(RichText::new(format!("Jauge indisponible : {e}")).color(theme::NOW)).wrap());
            ui.add_space(6.0);
            let b = egui::Button::new("PAGE OFFICIELLE ↗").min_size(egui::vec2(0.0, if page { 44.0 } else { 0.0 }));
            if ui.add(b).clicked() {
                open_gauge(ctx, r.gauge);
            }
        }
        Some(Ok(l)) => {
            match &l.gauge {
                Gauge::Closed => note(ui, "Restaurant fermé en ce moment : la fréquentation s'affichera à la prochaine ouverture."),
                Gauge::Transition => note(ui, "Fin du service : le prochain s'affiche bientôt."),
                Gauge::Open { meals, capacity, color } => {
                    let full = meals >= capacity;
                    let frac = (*meals as f32 / *capacity as f32).clamp(0.0, 1.0);
                    let col = if full {
                        theme::NOW
                    } else {
                        match color.as_str() {
                            "rouge" => theme::NOW,
                            "orange" => theme::CM,
                            "jaune" => theme::WARN,
                            "vert" => theme::OK,
                            _ => theme::ACCENT,
                        }
                    };
                    ui.label(RichText::new("REPAS DÉJÀ SERVIS").font(theme::mono_bold(10.5)).color(theme::DIM));
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        ui.label(RichText::new(milliers((*meals).min(*capacity))).font(bold(34.0)).color(theme::TEXT));
                        ui.label(RichText::new(format!("/ {}", milliers(*capacity))).font(regular(17.0)).color(theme::MUTED));
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(format!("{:.0} %", frac * 100.0)).font(theme::mono_bold(15.0)).color(col));
                        });
                    });
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 10.0), egui::Sense::hover());
                    let p = ui.painter();
                    p.rect_filled(rect, 2.0, theme::LINE);
                    if frac > 0.0 {
                        let mut f = rect;
                        f.set_width((rect.width() * frac).max(3.0));
                        p.rect_filled(f, 2.0, col);
                    }
                    if full {
                        ui.add_space(4.0);
                        ui.label(RichText::new("Capacité maximale atteinte.").font(bold(14.0)).color(theme::NOW));
                    }
                }
            }
            ui.add_space(6.0);
            let mut t = match &l.updated {
                Some(h) => format!("Mis à jour à {h}"),
                None => "Données du Crous".to_string(),
            };
            if !l.fresh {
                t.push_str(" · données en retard");
            }
            ui.label(RichText::new(t).font(theme::mono(10.5)).color(if l.fresh { theme::DIM } else { theme::WARN }));
        }
    }
}

/// Points de rupture : mobile < 768 px, moyen < 1280 px, large au-delà.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Bp {
    Mobile,
    Medium,
    Wide,
}

fn breakpoint(ctx: &egui::Context) -> Bp {
    let w = ctx.content_rect().width();
    if w < 768.0 {
        Bp::Mobile
    } else if w < 1280.0 {
        Bp::Medium
    } else {
        Bp::Wide
    }
}

#[derive(Clone, Copy)]
enum Act {
    Devices,
    Grades,
    Resto,
    Exams,
    Mails,
    Rev,
    About,
    Refresh,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Week,
    Day,
}

struct SyncMsg {
    from: NaiveDate,
    to: NaiveDate,
    result: Result<Vec<Cours>, String>,
}

enum MailMsg {
    List(Result<Vec<Header>, String>),
    Body(u32, Result<String, String>),
}

pub struct App {
    rt: tokio::runtime::Handle,
    cfg: Config,
    password: Option<String>,
    store: Store,
    view: View,
    selected: NaiveDate,
    show_side: bool,
    last_bp: Option<Bp>,
    tx: Sender<SyncMsg>,
    rx: Receiver<SyncMsg>,
    syncing: bool,
    last_attempt: Option<Instant>,
    error: Option<String>,
    detail: Option<Cours>,
    show_exams: bool,
    chrono: Chrono,
    rev_ui: RevUi,
    show_rev: bool,
    show_about: bool,
    show_resto: bool,
    resto_tab: usize,
    resto_data: [Option<Result<RestoData, String>>; 2],
    resto_pending: u8,
    resto_day: Option<NaiveDate>,
    resto_last: Option<Instant>,
    resto_tx: Sender<(usize, Result<RestoData, String>)>,
    resto_rx: Receiver<(usize, Result<RestoData, String>)>,
    /// Fréquentation en direct (jauge « repas servis »), relue chaque minute fenêtre Resto ouverte.
    live: [Option<Result<Live, String>>; 2],
    live_pending: u8,
    live_last: Option<Instant>,
    live_tx: Sender<(usize, Result<Live, String>)>,
    live_rx: Receiver<(usize, Result<Live, String>)>,
    credit_tex: [Option<egui::TextureHandle>; 2],
    mail_pwd: Option<String>,
    show_mail: bool,
    mails: Vec<Header>,
    mail_loading: bool,
    mail_error: Option<String>,
    mail_sel: Option<u32>,
    mail_body: Option<Result<String, String>>,
    mail_tx: Sender<MailMsg>,
    mail_rx: Receiver<MailMsg>,
    login: LoginForm,
    p2p: P2p,
    show_devices: bool,
    dev_ui: DevicesUi,
    pair_req: Option<PairRequest>,
    /// Texte à coller au prochain tour (pastille « COLLER » d'Android).
    pending_paste: Option<String>,
    /// Dernier champ texte focalisé et l'instant où il l'était encore.
    text_focus: Option<(egui::Id, f64)>,
    /// Feuille « Plus » de la barre d'onglets mobile.
    show_more: bool,
    /// Dernières marges système vues et l'instant de leur dernier changement (clavier qui s'ouvre…).
    last_insets: ([f32; 4], f64),
    /// Profils et notes (synchronisés via le journal partagé avec le chrono).
    grades: Book,
    grades_ui: GradesUi,
    show_grades: bool,
    /// Mises à jour publiées sur GitHub (bandeau, page « Mise à jour », « À propos »).
    maj: crate::update::Maj,
}

/// Formulaire de connexion (affiché tant qu'aucun mot de passe n'est enregistré).
#[derive(Default)]
struct LoginForm {
    user: String,
    federation: String,
    pwd: String,
    error: Option<String>,
    init: bool,
}

impl App {
    pub fn new(rt: tokio::runtime::Handle, ctx: egui::Context) -> Self {
        let (tx, rx) = mpsc::channel();
        let (mail_tx, mail_rx) = mpsc::channel();
        let (resto_tx, resto_rx) = mpsc::channel();
        let (live_tx, live_rx) = mpsc::channel();
        let today = now().date();
        let chrono = Chrono::load();
        let grades = Book::new(chrono.journal_handle());
        let p2p = P2p::start(rt.clone(), ctx, chrono.journal_handle());
        let cfg = Config::load();
        // Sans identifiant, le mot de passe seul ne sert à rien : on repasse par l'écran de connexion.
        let password = if cfg.username.trim().is_empty() { None } else { load_password() };
        Self {
            rt,
            cfg,
            password,
            store: Store::load(),
            view: View::Week,
            selected: today,
            show_side: true,
            last_bp: None,
            tx,
            rx,
            syncing: false,
            last_attempt: None,
            error: None,
            detail: None,
            show_exams: false,
            chrono,
            rev_ui: RevUi::default(),
            show_rev: false,
            show_about: false,
            show_resto: false,
            resto_tab: 0,
            resto_data: [None, None],
            resto_pending: 0,
            resto_day: None,
            resto_last: None,
            resto_tx,
            resto_rx,
            live: [None, None],
            live_pending: 0,
            live_last: None,
            live_tx,
            live_rx,
            credit_tex: [None, None],
            mail_pwd: load_mail_password(),
            show_mail: false,
            mails: Vec::new(),
            mail_loading: false,
            mail_error: None,
            mail_sel: None,
            mail_body: None,
            mail_tx,
            mail_rx,
            login: LoginForm::default(),
            p2p,
            show_devices: false,
            dev_ui: DevicesUi::default(),
            pending_paste: None,
            text_focus: None,
            show_more: false,
            last_insets: ([0.0; 4], 0.0),
            pair_req: None,
            grades,
            grades_ui: GradesUi::default(),
            show_grades: false,
            maj: Default::default(),
        }
    }

    fn device_name(&self) -> String {
        let n = self.cfg.device_name.trim();
        if n.is_empty() { crate::p2p::default_device_name() } else { n.to_string() }
    }

    /// Identifiants reçus du PC à l'appairage : enregistrés comme à la saisie manuelle.
    fn apply_credentials(&mut self, c: Credentials) {
        self.cfg.username = c.username;
        self.cfg.federation_id = c.federation_id;
        self.cfg.mail_user = c.mail_user;
        self.cfg.save();
        if crate::config::save_password(&c.password).is_ok() {
            self.password = load_password();
            self.mail_pwd = load_mail_password();
            self.error = None;
            self.last_attempt = None;
        }
    }

    fn my_credentials(&self) -> Option<Credentials> {
        Some(Credentials {
            username: self.cfg.username.clone(),
            federation_id: self.cfg.federation_id.clone(),
            password: self.password.clone()?,
            mail_user: self.cfg.mail_user.clone(),
        })
    }

    /// Appairage et synchro entre appareils : résultats, demandes entrantes, fenêtre « Appareils ».
    fn devices_window(&mut self, ctx: &egui::Context) {
        if let Some(t) = crate::p2p::take_startup_ticket() {
            self.dev_ui.input = t;
            self.show_devices = true;
        }
        // Côté téléphone : issue de l'appairage lancé avec un ticket.
        if let Some(r) = self.p2p.take_pair_result() {
            self.dev_ui.phone_code = None;
            match r {
                Ok(out) => {
                    if let Some(c) = out.credentials {
                        self.apply_credentials(c);
                    }
                    self.dev_ui.msg = Some((format!("Associé à « {} ». Synchronisation…", out.peer_name), true));
                    self.dev_ui.input.clear();
                    self.p2p.sync_now();
                }
                Err(e) => self.dev_ui.msg = Some((e, false)),
            }
        }
        // Côté PC : demande entrante à confirmer.
        if self.pair_req.is_none()
            && let Some(r) = self.p2p.poll_pair_request()
        {
            self.dev_ui.send_creds = false;
            self.pair_req = Some(r);
            ctx.request_repaint(); // la page « Nouvel appareil » passe devant à l'image suivante
        }
        if let Some(req) = self.pair_req.take() {
            match devices::request_dialog(ctx, &req, &mut self.dev_ui.send_creds, self.password.is_some()) {
                Some(accept) => {
                    let name = req.name.clone();
                    let credentials = if accept && self.dev_ui.send_creds { self.my_credentials() } else { None };
                    req.answer(Decision { accept, credentials });
                    self.dev_ui.ticket = None; // le secret est brûlé dans tous les cas
                    self.dev_ui.msg =
                        Some(if accept { (format!("« {name} » est associé."), true) } else { ("Demande refusée.".to_string(), false) });
                    self.show_devices = true;
                    self.p2p.sync_now();
                }
                None => self.pair_req = Some(req),
            }
        }
        if self.dev_ui.ticket.as_ref().is_some_and(|(_, d)| *d <= Instant::now()) {
            self.dev_ui.ticket = None;
            self.p2p.cancel_pairing();
        }
        if !self.show_devices {
            return;
        }
        self.p2p.ensure_started(); // l'utilisateur s'intéresse aux appareils : on lance le moteur réseau
        let mut open = true;
        let (peers, status, busy) = (self.p2p.peers(), self.p2p.status(), self.p2p.pairing_busy());
        let mobile = breakpoint(ctx) == Bp::Mobile;
        let acts = devices::show(ctx, &mut open, &mut self.dev_ui, &peers, &status, busy, mobile);
        for a in acts {
            match a {
                DevAction::StartPairing => {
                    let name = self.device_name();
                    match self.p2p.start_pairing(&name) {
                        Some(t) => {
                            self.dev_ui.ticket = Some(t);
                            self.dev_ui.msg = None;
                        }
                        None => self.dev_ui.msg = Some(("Moteur P2P pas encore prêt, réessaie dans un instant.".to_string(), false)),
                    }
                }
                DevAction::CancelPairing => {
                    self.p2p.cancel_pairing();
                    self.dev_ui.ticket = None;
                }
                DevAction::Pair(text) => {
                    let name = self.device_name();
                    self.dev_ui.msg = None;
                    self.dev_ui.phone_code = self.p2p.code_for(&text);
                    self.p2p.pair_with_text(&text, &name);
                }
                DevAction::Forget(id) => self.p2p.forget(&id),
                DevAction::SyncNow => self.p2p.sync_now(),
            }
        }
        if !open {
            self.show_devices = false;
        }
        if self.dev_ui.ticket.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(500)); // compte à rebours
        }
    }

    /// Demande l'identifiant et le mot de passe Celcat au premier lancement
    /// (indispensable sur Android, où il n'y a ni variable d'environnement ni `~/CELCAT_PASSWORD`).
    fn login_window(&mut self, ctx: &egui::Context) {
        if self.password.is_some() {
            return;
        }
        if !self.login.init {
            self.login.user = self.cfg.username.clone();
            self.login.federation = self.cfg.federation_id.clone();
            self.login.init = true;
        }
        let mut submit = false;
        let win = egui::Window::new(RichText::new("Connexion Celcat").font(bold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 125.0])
            .default_width((ctx.content_rect().width() - 32.0).clamp(260.0, 380.0));
        ui::page::show(ctx, egui::Id::new("login"), "Connexion Celcat", win, None, true, |ui| {
            ui.label(RichText::new("Identifiants CY Tech. Ils restent sur cet appareil.").font(regular(13.0)).color(theme::DIM));
            ui.label(
                RichText::new("Cydera est déjà sur ton PC ? Menu → Appareils : associe-les et les identifiants suivent.")
                    .font(regular(12.0))
                    .color(theme::MUTED),
            );
            ui.add_space(8.0);
            let w = ui.available_width();
            ui.label(RichText::new("Identifiant").font(regular(12.0)));
            ui.add(egui::TextEdit::singleline(&mut self.login.user).desired_width(w));
            ui.label(RichText::new("Numéro étudiant").font(regular(12.0)));
            ui.add(egui::TextEdit::singleline(&mut self.login.federation).desired_width(w));
            ui.label(RichText::new("Mot de passe").font(regular(12.0)));
            let r = ui.add(egui::TextEdit::singleline(&mut self.login.pwd).password(true).desired_width(w));
            // Le numéro étudiant n'est requis que s'il n'est pas déjà connu.
            let federation_ok = !self.login.federation.trim().is_empty() || !self.cfg.federation_id.is_empty();
            let ok = !self.login.user.trim().is_empty() && !self.login.pwd.trim().is_empty() && federation_ok;
            if ok && r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                submit = true;
            }
            if let Some(e) = &self.login.error {
                ui.colored_label(theme::NOW, e);
            }
            ui.add_space(8.0);
            if ui.add_enabled(ok, egui::Button::new(RichText::new("Enregistrer").font(bold(14.0))).min_size(egui::vec2(w, 48.0))).clicked()
            {
                submit = true;
            }
        });
        if submit {
            let f = &mut self.login;
            let same_mail = self.cfg.mail_user == self.cfg.username;
            self.cfg.username = f.user.trim().to_string();
            if same_mail {
                self.cfg.mail_user = self.cfg.username.clone();
            }
            if !f.federation.trim().is_empty() {
                self.cfg.federation_id = f.federation.trim().to_string();
            }
            match crate::config::save_password(&f.pwd) {
                Ok(()) => {
                    self.cfg.save();
                    self.password = load_password();
                    self.mail_pwd = load_mail_password();
                    f.pwd.clear();
                    f.error = None;
                    self.error = None;
                    self.last_attempt = None;
                    ctx.request_repaint();
                }
                Err(e) => f.error = Some(format!("Enregistrement impossible : {e}")),
            }
        }
    }

    /// Examens (catégorie Celcat « Examens ») pas encore terminés, triés.
    fn upcoming_exams(&self) -> Vec<&Cours> {
        let now = now();
        self.store.cours.iter().filter(|c| c.kind == Kind::Exam && c.end > now).collect()
    }

    /// Une entrée par matière : son prochain examen.
    fn exam_refs(&self) -> Vec<ExamRef> {
        let mut v: Vec<ExamRef> = Vec::new();
        for c in self.upcoming_exams() {
            if !v.iter().any(|e| e.subject == c.matiere) {
                v.push(ExamRef { subject: c.matiere.clone(), date: c.date() });
            }
        }
        v
    }

    /// Photo GitHub (0) et logo Claude (1), intégrés à l'exécutable.
    fn credit_tex(&mut self, ctx: &egui::Context, idx: usize) -> egui::TextureHandle {
        if self.credit_tex[idx].is_none() {
            let bytes: &[u8] = if idx == 0 { include_bytes!("../assets/avatar.rgba") } else { include_bytes!("../assets/claude.rgba") };
            let img = egui::ColorImage::from_rgba_unmultiplied([128, 128], bytes);
            self.credit_tex[idx] =
                Some(ctx.load_texture(if idx == 0 { "credit-github" } else { "credit-claude" }, img, egui::TextureOptions::LINEAR));
        }
        self.credit_tex[idx].clone().unwrap()
    }

    fn about_window(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }
        let mut open = true;
        let (mut maj_action, mut maj_auto) = (None, self.cfg.maj_auto);
        let win = egui::Window::new(RichText::new("À propos").font(bold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(420.0_f32.min(ctx.content_rect().width() - 24.0));
        ui::page::show(ctx, egui::Id::new("about"), "À propos", win, Some(&mut open), true, |ui| {
            ui.label(RichText::new("CYDERA").font(bold(30.0)));
            ui.label(RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION"))).color(theme::DIM));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Emploi du temps CY Tech (Celcat), mails étudiants et chrono de révisions, dans une seule appli de bureau.")
                    .color(theme::MUTED),
            );
            ui.add_space(12.0);
            ui.label(RichText::new("AUTEURS").font(bold(11.5)).color(theme::DIM));
            for (idx, (nom, role, lien, url)) in [
                ("Lucas", "Idée, conception et tests", "github.com/BerretMan", "https://github.com/BerretMan"),
                ("Claude", "Assistant IA d'Anthropic : développement", "claude.ai", "https://claude.ai"),
            ]
            .into_iter()
            .enumerate()
            {
                let tex = self.credit_tex(ctx, idx);
                let resp = theme::card(ui, Margin::symmetric(14, 12), |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Image::new(&tex)
                                .fit_to_exact_size(egui::vec2(56.0, 56.0))
                                .corner_radius(egui::CornerRadius::same(if idx == 0 { 28 } else { 14 })),
                        );
                        ui.vertical(|ui| {
                            ui.label(RichText::new(nom).font(bold(16.0)));
                            ui.label(RichText::new(role).color(theme::MUTED));
                            ui.hyperlink_to(lien, url);
                        });
                    });
                });
                // toute la carte est cliquable
                let click = ui.interact(resp.response.rect, ui.id().with(("credit", idx)), egui::Sense::click());
                if click.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(url));
                }
            }
            ui.add_space(8.0);
            maj_action = ui::update::apropos(ui, &self.maj, &mut maj_auto);
            ui.add_space(8.0);
            ui.label(
                RichText::new("Écrit en Rust avec egui · Données Celcat © CY Cergy Paris Université").font(regular(12.0)).color(theme::DIM),
            );
        });
        if !open {
            self.show_about = false;
        }
        if maj_auto != self.cfg.maj_auto {
            self.cfg.maj_auto = maj_auto;
            self.cfg.save();
        }
        if let Some(a) = maj_action {
            self.maj_action(ctx, a);
        }
    }

    fn maj_action(&mut self, ctx: &egui::Context, a: ui::update::Action) {
        use ui::update::Action;
        match a {
            Action::Ouvrir => self.maj.show = true,
            Action::Installer => self.maj.installer(&self.rt, ctx),
            Action::Relancer(exe) => crate::update::relancer(ctx, &exe),
            Action::OuvrirPage => self.maj.ouvrir_page(ctx),
            Action::Verifier => self.maj.verifier(&self.rt, ctx, true),
            Action::PlusTard => self.maj.masque = true,
            Action::Ignorer => {
                if let Some(r) = &self.maj.dispo {
                    self.cfg.maj_ignoree = r.version.to_string();
                    self.cfg.save();
                }
            }
        }
        ctx.request_repaint();
    }

    fn exams_window(&mut self, ctx: &egui::Context) {
        if !self.show_exams {
            return;
        }
        let mut open = true;
        let mut picked = None;
        let mut revise: Option<String> = None;
        let today = now().date();
        let win_w = 460.0_f32.min(ctx.content_rect().width() - 24.0).max(280.0);
        let page = ui::page::is_page(ctx);
        let win = egui::Window::new(RichText::new("Examens à venir").font(bold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::RIGHT_TOP, [-24.0, 70.0])
            .default_width(win_w);
        ui::page::show(ctx, egui::Id::new("exams"), "Examens à venir", win, Some(&mut open), false, |ui| {
            // Largeur explicite : sans elle, le texte vertical d'une ligne (layout droite→gauche)
            // recevait une largeur nulle et passait à la ligne lettre par lettre, en élargissant la fenêtre.
            ui.set_width(if page { ui.available_width() } else { win_w - 32.0 });
            let exams = self.upcoming_exams();
            if exams.is_empty() {
                ui.label(RichText::new("Aucun examen prévu dans l'emploi du temps synchronisé.").color(theme::MUTED));
                return;
            }
            let max_h = if page { ui.available_height() } else { 520.0 };
            egui::ScrollArea::vertical().max_height(max_h).auto_shrink([false, true]).show(ui, |ui| {
                // largeur figée une fois : sinon chaque carte gagnait quelques px sur la précédente
                let row_w = ui.available_width();
                ui.set_width(row_w);
                for c in exams {
                    let days = (c.date() - today).num_days();
                    let col = match days {
                        d if d <= 7 => theme::NOW,
                        d if d <= 21 => theme::EXAM,
                        _ => theme::ACCENT,
                    };
                    let mut rev_btn = egui::Rect::NOTHING;
                    let resp = theme::card(ui, Margin::symmetric(12, 10), |ui| {
                        let inner_w = row_w - 26.0; // marges 2×12 + bordures
                        ui.set_width(inner_w);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 12.0;
                            let badge = if days == 0 { "AUJ.".to_string() } else { format!("J-{days}") };
                            ui.add_sized([62.0, 40.0], egui::Label::new(RichText::new(badge).font(theme::mono_bold(20.0)).color(col)));
                            let btn_w = 92.0;
                            let text_w = (inner_w - 62.0 - btn_w - 24.0).max(80.0);
                            ui.vertical(|ui| {
                                ui.set_width(text_w);
                                ui.add(egui::Label::new(RichText::new(c.matiere.to_uppercase()).font(bold(14.0))).truncate());
                                let meta = format!(
                                    "{} {:02} {} · {}–{} · {}",
                                    ui::jour_court(c.date()),
                                    c.date().day(),
                                    ui::mois(c.date()).chars().take(3).collect::<String>().to_uppercase(),
                                    c.start.format("%H:%M"),
                                    c.end.format("%H:%M"),
                                    c.salle_courte
                                );
                                ui.add(egui::Label::new(RichText::new(meta).font(theme::mono(11.0)).color(theme::MUTED)).truncate());
                            });
                            let b = ui.add_sized([btn_w, if page { 44.0 } else { 28.0 }], egui::Button::new("⏱ RÉVISER"));
                            rev_btn = b.rect;
                        });
                    })
                    .response;
                    let click = ui
                        .interact(resp.rect, ui.id().with(("exam", c.start, &c.matiere)), egui::Sense::click())
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if click.clicked() {
                        if click.interact_pointer_pos().is_some_and(|p| rev_btn.contains(p)) {
                            revise = Some(c.matiere.clone());
                        } else {
                            picked = Some(c.clone());
                        }
                    }
                    ui.add_space(4.0);
                }
            });
        });
        if let Some(c) = picked {
            self.selected = c.date();
            self.detail = Some(c);
        }
        if let Some(subject) = revise {
            let part = self.chrono.part.clone();
            self.chrono.select(&subject, &part, true);
            self.show_rev = true;
        }
        if !open {
            self.show_exams = false;
        }
    }

    fn mail_ready(&self) -> bool {
        self.mail_pwd.is_some() && !self.cfg.mail_user.is_empty()
    }

    fn mail_refresh(&mut self, ctx: &egui::Context) {
        if !self.mail_ready() || self.mail_loading {
            return;
        }
        self.mail_loading = true;
        let (host, user, pwd) = (self.cfg.mail_host.clone(), self.cfg.mail_user.clone(), self.mail_pwd.clone().unwrap_or_default());
        let (tx, ctx) = (self.mail_tx.clone(), ctx.clone());
        self.rt.spawn(async move {
            let r = mail::list(&host, &user, &pwd, 40).await.map_err(|e| e.to_string());
            let _ = tx.send(MailMsg::List(r));
            ctx.request_repaint();
        });
    }

    fn mail_open(&mut self, ctx: &egui::Context, uid: u32) {
        self.mail_sel = Some(uid);
        self.mail_body = None;
        let (host, user, pwd) = (self.cfg.mail_host.clone(), self.cfg.mail_user.clone(), self.mail_pwd.clone().unwrap_or_default());
        let (tx, ctx) = (self.mail_tx.clone(), ctx.clone());
        self.rt.spawn(async move {
            let r = mail::body(&host, &user, &pwd, uid).await.map_err(|e| e.to_string());
            let _ = tx.send(MailMsg::Body(uid, r));
            ctx.request_repaint();
        });
    }

    fn mail_window(&mut self, ctx: &egui::Context) {
        if !self.show_mail {
            return;
        }
        let mut open = true;
        let mut pick = None;
        let mut back = false;
        let page = ui::page::is_page(ctx);
        let win = egui::Window::new(RichText::new("Mails").font(bold(18.0)))
            .collapsible(false)
            .default_size([900.0_f32.min(ctx.content_rect().width() - 24.0), 560.0_f32.min(ctx.content_rect().height() - 48.0)]);
        // Page mobile : le retour ramène d'abord du message à la boîte de réception.
        let reading = page && self.mail_sel.is_some();
        let mut keep = true;
        let shown =
            ui::page::show(ctx, egui::Id::new("mail"), "Mails", win, Some(if reading { &mut keep } else { &mut open }), false, |ui| {
                if !self.mail_ready() {
                    ui.label(
                        RichText::new("Connecte-toi d'abord avec tes identifiants ENT (les mêmes que pour l'emploi du temps).")
                            .color(theme::WARN),
                    );
                    return;
                }
                ui.horizontal(|ui| {
                    let b = egui::Button::new("Actualiser").min_size(egui::vec2(0.0, if page { 44.0 } else { 0.0 }));
                    if ui.add_enabled(!self.mail_loading, b).clicked() {
                        self.mail_refresh(ctx);
                    }
                    if self.mail_loading {
                        ui.spinner();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.hyperlink_to("Ouvrir le webmail ↗", format!("https://{}/", self.cfg.mail_host))
                            .on_hover_text("Pour répondre ou écrire un mail");
                    });
                });
                if let Some(e) = &self.mail_error {
                    ui.add(egui::Label::new(RichText::new(e).color(theme::NOW)).wrap());
                }
                ui.add_space(6.0);
                // Écran étroit (téléphone) : la liste, puis le message seul avec un retour.
                if ui.available_width() < 600.0 {
                    if self.mail_sel.is_some() {
                        // (en page mobile, c'est le bouton retour de l'en-tête qui ramène à la liste)
                        if !page && ui.button("← Boîte de réception").clicked() {
                            back = true;
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().id_salt("mail_body").show(ui, |ui| self.mail_body_ui(ui));
                    } else {
                        egui::ScrollArea::vertical().id_salt("mail_list").show(ui, |ui| self.mail_list_ui(ui, &mut pick));
                    }
                } else {
                    ui.columns(2, |cols| {
                        egui::ScrollArea::vertical().id_salt("mail_list").show(&mut cols[0], |ui| self.mail_list_ui(ui, &mut pick));
                        egui::ScrollArea::vertical().id_salt("mail_body").show(&mut cols[1], |ui| self.mail_body_ui(ui));
                    });
                }
            });
        if let Some(uid) = pick {
            self.mail_open(ctx, uid);
        }
        if reading && shown.back {
            back = true;
        }
        if back {
            self.mail_sel = None;
            self.mail_body = None;
        }
        if !open {
            self.show_mail = false;
        }
    }

    fn mail_list_ui(&self, ui: &mut egui::Ui, pick: &mut Option<u32>) {
        if self.mails.is_empty() && !self.mail_loading && self.mail_error.is_none() {
            ui.label(RichText::new("Aucun message.").color(theme::MUTED));
        }
        for m in &self.mails {
            let sel = self.mail_sel == Some(m.uid);
            let title = if m.unread { bold(14.0) } else { regular(14.0) };
            let resp = theme::card_ex(
                ui,
                if sel { theme::CARD_HOVER } else { theme::CARD },
                egui::Stroke::new(1.0, theme::LINE),
                8,
                Margin::symmetric(10, 6),
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if m.unread {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.0, theme::ACCENT);
                        }
                        ui.add(egui::Label::new(RichText::new(&m.from).font(title.clone())).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&m.date).font(regular(12.0)).color(theme::DIM));
                        });
                    });
                    ui.add(egui::Label::new(RichText::new(&m.subject).color(theme::MUTED)).truncate());
                },
            )
            .response;
            if ui
                .interact(resp.rect, ui.id().with(("mail", m.uid)), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                *pick = Some(m.uid);
            }
            ui.add_space(3.0);
        }
    }

    fn mail_body_ui(&self, ui: &mut egui::Ui) {
        let Some(uid) = self.mail_sel else {
            ui.label(RichText::new("Sélectionne un message.").color(theme::MUTED));
            return;
        };
        if let Some(m) = self.mails.iter().find(|m| m.uid == uid) {
            ui.add(egui::Label::new(RichText::new(&m.subject).font(bold(17.0))).wrap());
            ui.label(RichText::new(format!("{} · {}", m.from, m.date)).color(theme::MUTED));
            ui.separator();
        }
        match &self.mail_body {
            None => {
                ui.spinner();
            }
            Some(Ok(t)) => {
                ui.add(egui::Label::new(t.as_str()).wrap());
            }
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::NOW));
            }
        }
    }

    fn resto_refresh(&mut self, ctx: &egui::Context) {
        if self.resto_pending > 0 {
            return;
        }
        self.resto_pending = 2;
        self.resto_last = Some(Instant::now());
        for i in 0..2 {
            let (tx, ctx) = (self.resto_tx.clone(), ctx.clone());
            self.rt.spawn(async move {
                let r = resto::fetch(&resto::RESTOS[i]).await;
                let _ = tx.send((i, r));
                ctx.request_repaint();
            });
        }
    }

    fn live_refresh(&mut self, ctx: &egui::Context) {
        if self.live_pending > 0 {
            return;
        }
        self.live_pending = 2;
        self.live_last = Some(Instant::now());
        for i in 0..2 {
            let (tx, ctx) = (self.live_tx.clone(), ctx.clone());
            self.rt.spawn(async move {
                let r = resto::fetch_live(&resto::RESTOS[i]).await;
                let _ = tx.send((i, r));
                ctx.request_repaint();
            });
        }
    }

    fn toggle_resto(&mut self, ctx: &egui::Context) {
        self.show_resto = !self.show_resto;
        if self.show_resto && self.resto_last.is_none_or(|t| t.elapsed().as_secs() > 1800) {
            self.resto_refresh(ctx);
        }
    }

    fn resto_window(&mut self, ctx: &egui::Context) {
        if !self.show_resto {
            return;
        }
        if self.resto_last.is_none() {
            self.resto_refresh(ctx);
        }
        // Jauge en direct : une requête par minute et par resto, seulement fenêtre ouverte.
        const LIVE_EVERY: std::time::Duration = std::time::Duration::from_secs(60);
        if self.live_last.is_none_or(|t| t.elapsed() >= LIVE_EVERY) {
            self.live_refresh(ctx);
        }
        ctx.request_repaint_after(LIVE_EVERY);
        let mut open = true;
        let today = now().date();
        let page = ui::page::is_page(ctx);
        let size = [580.0_f32.min(ctx.content_rect().width() - 24.0), 660.0_f32.min(ctx.content_rect().height() - 48.0)];
        // cibles tactiles : 44 pt en page mobile
        let tap = if page { 44.0 } else { 0.0 };
        let win = egui::Window::new(RichText::new("Restos U · Pau").font(bold(18.0))).collapsible(false).default_size(size);
        ui::page::show(ctx, egui::Id::new("resto"), "Restos U · Pau", win, Some(&mut open), false, |ui| {
            // Onglets + actualiser
            ui.horizontal(|ui| {
                for (i, r) in resto::RESTOS.iter().enumerate() {
                    let b = egui::Button::selectable(self.resto_tab == i, r.name).min_size(egui::vec2(if page { 96.0 } else { 0.0 }, tap));
                    if ui.add(b).clicked() && self.resto_tab != i {
                        self.resto_tab = i;
                        self.resto_day = None;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = egui::Button::new("ACTUALISER").min_size(egui::vec2(0.0, tap));
                    if ui.add_enabled(self.resto_pending == 0, b).clicked() {
                        self.resto_refresh(ctx);
                        self.live_refresh(ctx);
                    }
                    if self.resto_pending > 0 || self.live_pending > 0 {
                        ui.spinner();
                    }
                });
            });
            ui.add_space(6.0);
            let tab = self.resto_tab;
            let r = &resto::RESTOS[tab];
            let mut day_pick = self.resto_day;
            let max_h = if page { ui.available_height() } else { size[1] - 70.0 };
            egui::ScrollArea::vertical().id_salt("resto_scroll").auto_shrink([false, false]).max_height(max_h).show(ui, |ui| {
                ui.set_width(ui.available_width());
                // Fréquentation en direct : repas déjà servis / capacité, lus sur l'API de jauge du Crous.
                theme::card_ex(ui, theme::CARD, egui::Stroke::new(1.0, theme::LINE), 3, Margin::ZERO, |ui| {
                    ui.set_width(ui.available_width());
                    ui::telemetry::module_header(ui, "LIVE://FRÉQUENTATION", r.name);
                    egui::Frame::new().inner_margin(Margin::same(12)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        live_gauge(ui, ctx, r, self.live[tab].as_ref(), page);
                    });
                });
                ui.add_space(8.0);
                match &self.resto_data[tab] {
                    None => {
                        ui.add_space(20.0);
                        ui.label(RichText::new("Chargement des menus…").color(theme::MUTED));
                    }
                    Some(Err(e)) => {
                        ui.add(egui::Label::new(RichText::new(format!("Impossible de lire les menus : {e}")).color(theme::NOW)).wrap());
                    }
                    Some(Ok(d)) => {
                        if d.days.is_empty() {
                            ui.label(RichText::new("Aucun menu publié pour le moment.").color(theme::MUTED));
                        } else {
                            let idx = day_pick
                                .and_then(|x| d.days.iter().position(|m| m.date == x))
                                .or_else(|| d.days.iter().position(|m| m.date >= today))
                                .unwrap_or(d.days.len() - 1);
                            let day = &d.days[idx];
                            let tag = match (day.date - today).num_days() {
                                0 => " · AUJOURD'HUI",
                                1 => " · DEMAIN",
                                _ => "",
                            };
                            let label = RichText::new(format!("{}{}", date_longue(day.date).to_uppercase(), tag)).font(bold(15.0));
                            ui.horizontal(|ui| {
                                let nb = egui::vec2(if page { 48.0 } else { 0.0 }, tap);
                                if ui.add_enabled(idx > 0, egui::Button::new("⏴").min_size(nb)).clicked() {
                                    day_pick = Some(d.days[idx - 1].date);
                                }
                                if page {
                                    // date centrée entre les deux flèches
                                    let w = ui.available_width() - nb.x - ui.spacing().item_spacing.x;
                                    ui.allocate_ui_with_layout(egui::vec2(w, tap), egui::Layout::centered_and_justified(egui::Direction::LeftToRight), |ui| {
                                        ui.add(egui::Label::new(label.clone()).truncate());
                                    });
                                }
                                if ui.add_enabled(idx + 1 < d.days.len(), egui::Button::new("⏵").min_size(nb)).clicked() {
                                    day_pick = Some(d.days[idx + 1].date);
                                }
                                if !page {
                                    ui.label(label);
                                }
                            });
                            ui.add_space(6.0);
                            if day.meals.is_empty() {
                                ui.label(RichText::new("Pas de menu ce jour-là.").color(theme::MUTED));
                            }
                            for meal in &day.meals {
                                theme::card_ex(ui, theme::CARD, egui::Stroke::new(1.0, theme::LINE), 3, Margin::ZERO, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui::telemetry::module_header(ui, &format!("[{}]", meal.name.to_uppercase()), r.name);
                                    egui::Frame::new().inner_margin(Margin::same(14)).show(ui, |ui| {
                                        ui.set_width(ui.available_width());
                                        ui.label(RichText::new("PLAT DU JOUR").font(theme::mono_bold(11.5)).color(theme::CM));
                                        ui.add_space(4.0);
                                        match meal.plat_du_jour() {
                                            Some(items) => {
                                                for it in items {
                                                    ui.add(egui::Label::new(RichText::new(format!("·  {it}")).font(bold(20.0))).wrap());
                                                    ui.add_space(2.0);
                                                }
                                            }
                                            None => {
                                                ui.label(RichText::new("Pas de plat du jour annoncé.").font(regular(15.0)).color(theme::MUTED));
                                            }
                                        }
                                        // Autres chaînes (grillade + frites, entrées / desserts…), un cran plus petites.
                                        for (cat, items) in meal.autres() {
                                            ui.add_space(10.0);
                                            let titre = if cat.is_empty() { "AUTRE".to_string() } else { cat.to_uppercase() };
                                            ui.label(RichText::new(titre).font(theme::mono_bold(11.5)).color(theme::ACCENT));
                                            ui.add_space(2.0);
                                            for it in items {
                                                ui.add(egui::Label::new(RichText::new(format!("·  {it}")).font(regular(16.0))).wrap());
                                            }
                                        }
                                    });
                                });
                                ui.add_space(8.0);
                            }
                        }
                    }
                }
                ui.separator();
                let b = egui::Button::new("SITE DU CROUS ↗").min_size(egui::vec2(if page { ui.available_width() } else { 0.0 }, tap));
                if ui.add(b).clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(r.page));
                }
                ui.add_space(4.0);
                ui.add(
                    egui::Label::new(
                        RichText::new("Menus et fréquentation viennent du Crous (site du Crous Bordeaux-Aquitaine et jauge restos.lescrous.fr, relue chaque minute).")
                            .font(regular(11.5))
                            .color(theme::DIM),
                    )
                    .wrap(),
                );
                ui.add_space(8.0);
            });
            self.resto_day = day_pick;
        });
        if !open {
            self.show_resto = false;
        }
    }

    fn poll(&mut self) {
        self.maj.poll();
        while let Ok((i, r)) = self.resto_rx.try_recv() {
            self.resto_pending = self.resto_pending.saturating_sub(1);
            self.resto_data[i] = Some(r);
        }
        while let Ok((i, r)) = self.live_rx.try_recv() {
            self.live_pending = self.live_pending.saturating_sub(1);
            // Une erreur passagère ne remplace pas une jauge déjà affichée.
            if r.is_ok() || !matches!(self.live[i], Some(Ok(_))) {
                self.live[i] = Some(r);
            }
        }
        while let Ok(msg) = self.mail_rx.try_recv() {
            match msg {
                MailMsg::List(r) => {
                    self.mail_loading = false;
                    match r {
                        Ok(l) => {
                            self.mails = l;
                            self.mail_error = None;
                        }
                        Err(e) => self.mail_error = Some(e),
                    }
                }
                MailMsg::Body(uid, r) if self.mail_sel == Some(uid) => self.mail_body = Some(r),
                MailMsg::Body(..) => {}
            }
        }
        while let Ok(msg) = self.rx.try_recv() {
            self.syncing = false;
            match msg.result {
                Ok(cours) => {
                    self.store.merge(msg.from, msg.to, cours);
                    self.store.save();
                    self.error = None;
                }
                Err(e) => self.error = Some(e),
            }
        }
    }

    fn maybe_sync(&mut self, ctx: &egui::Context, force: bool) {
        if self.syncing || self.password.is_none() {
            return;
        }
        let refresh = self.cfg.refresh_minutes;
        let today = now().date();
        let stale = [monday(self.selected), monday(today)].into_iter().find(|w| self.store.week_age_min(*w).is_none_or(|a| a >= refresh));
        let cooldown = if self.error.is_some() { 120 } else { 15 };
        let can_retry = self.last_attempt.is_none_or(|t| t.elapsed().as_secs() >= cooldown);
        let anchor = match (force, stale) {
            (true, _) => monday(self.selected),
            (false, Some(w)) if can_retry => w,
            _ => return,
        };
        let from = anchor - Duration::days(7);
        // ~6 mois : couvre la période d'examens (une seule requête, légère).
        let to = anchor + Duration::days(182);
        self.syncing = true;
        self.last_attempt = Some(Instant::now());
        let (cfg, pwd, tx, ctx) = (self.cfg.clone(), self.password.clone().unwrap_or_default(), self.tx.clone(), ctx.clone());
        self.rt.spawn(async move {
            let result = crate::celcat::fetch(&cfg, &pwd, from, to).await.map_err(|e| e.to_string());
            let _ = tx.send(SyncMsg { from, to, result });
            ctx.request_repaint();
        });
    }

    fn step(&mut self, dir: i64) {
        if self.view == View::Week {
            self.selected += Duration::days(7 * dir);
            return;
        }
        // Vue jour : on saute les week-ends sans cours (vendredi → lundi d'un seul geste).
        let mut d = self.selected + Duration::days(dir);
        for _ in 0..2 {
            let weekend = matches!(d.weekday(), chrono::Weekday::Sat | chrono::Weekday::Sun);
            if !weekend || self.store.has_day(d) {
                break;
            }
            d += Duration::days(dir);
        }
        self.selected = d;
    }

    fn handle_keys(&mut self, ctx: &egui::Context) -> bool {
        let mut force = false;
        let mut quit = false;
        let mut toggle_mail = false;
        let mut toggle_resto = false;
        let mut back = false;
        let typing = ctx.egui_wants_keyboard_input();
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(Key::Q) {
                quit = true;
                return;
            }
            if typing || (i.modifiers.any() && !i.modifiers.shift_only()) {
                return;
            }
            if i.key_pressed(Key::ArrowLeft) || i.key_pressed(Key::H) {
                self.step(-1);
            }
            if i.key_pressed(Key::ArrowRight) || i.key_pressed(Key::L) {
                self.step(1);
            }
            if i.key_pressed(Key::ArrowUp) {
                self.selected -= Duration::days(7);
            }
            if i.key_pressed(Key::ArrowDown) {
                self.selected += Duration::days(7);
            }
            if i.key_pressed(Key::T) || i.key_pressed(Key::Home) {
                self.selected = now().date();
            }
            if i.key_pressed(Key::S) || i.key_pressed(Key::W) {
                self.view = View::Week;
            }
            if i.key_pressed(Key::J) || i.key_pressed(Key::D) {
                self.view = View::Day;
            }
            if i.key_pressed(Key::Tab) {
                self.view = if self.view == View::Week { View::Day } else { View::Week };
            }
            if i.key_pressed(Key::P) {
                self.show_side = !self.show_side;
            }
            if i.key_pressed(Key::R) || i.key_pressed(Key::F5) {
                force = true;
            }
            if i.key_pressed(Key::M) {
                toggle_mail = true;
            }
            if i.key_pressed(Key::U) {
                toggle_resto = true;
            }
            if i.key_pressed(Key::C) {
                self.show_rev = !self.show_rev;
            }
            if i.key_pressed(Key::Space) && self.chrono.subject.is_some() {
                self.chrono.toggle();
            }
            if i.key_pressed(Key::E) {
                self.show_exams = !self.show_exams;
            }
            if i.key_pressed(Key::N) {
                self.show_grades = !self.show_grades;
            }
            if i.key_pressed(Key::BrowserBack) {
                back = true;
            }
            if i.key_pressed(Key::Escape) {
                if self.detail.is_some() {
                    self.detail = None;
                } else {
                    self.show_exams = false;
                    self.show_rev = false;
                    self.show_about = false;
                    self.show_resto = false;
                    if !self.grades_ui.back() {
                        self.show_grades = false;
                    }
                }
            }
        });
        // Bouton « retour » d'Android : ferme la feuille ou la page du dessus, sinon revient à aujourd'hui.
        if back {
            if self.show_more {
                self.show_more = false;
            } else if !self.close_top_page() {
                self.selected = now().date();
            }
            ctx.request_repaint();
        }
        if toggle_mail {
            self.toggle_mail(ctx);
        }
        if toggle_resto {
            self.toggle_resto(ctx);
        }
        if quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        force
    }

    /// Android : copies d'egui → presse-papiers système, et pastille « COLLER » tant qu'un champ texte
    /// est actif (le « Coller » du clavier n'arrive pas jusqu'à une `NativeActivity`).
    fn clipboard_bridge(&mut self, ctx: &egui::Context) {
        if !crate::clipboard::native() {
            return;
        }
        let copies: Vec<String> = ctx.output(|o| {
            o.commands.iter().filter_map(|c| if let egui::OutputCommand::CopyText(t) = c { Some(t.clone()) } else { None }).collect()
        });
        for t in copies {
            crate::clipboard::set(&t);
        }
        let t = ctx.input(|i| i.time);
        if ctx.egui_wants_keyboard_input()
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            self.text_focus = Some((id, t));
        }
        let Some((id, seen)) = self.text_focus else { return };
        // petit délai de grâce : appuyer sur la pastille peut retirer le focus au champ
        if t - seen > 0.8 {
            self.text_focus = None;
            return;
        }
        let resp = egui::Area::new(egui::Id::new("paste_chip"))
            .order(egui::Order::Foreground)
            .anchor(egui::Align2::RIGHT_BOTTOM, [-14.0, -14.0])
            .show(ctx, |ui| {
                let b = egui::Button::new(RichText::new("COLLER").font(theme::mono_bold(13.0)).color(Color32::BLACK))
                    .fill(theme::ACCENT)
                    .corner_radius(22)
                    .min_size(egui::vec2(124.0, 44.0));
                ui.add(b)
            })
            .inner;
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Coller"));
        // Déclenché à l'appui (le clavier peut se replier et déplacer la pastille avant le relâchement).
        if resp.contains_pointer() && ctx.input(|i| i.pointer.any_pressed()) {
            if let Some(txt) = crate::clipboard::get() {
                self.pending_paste = Some(txt);
            }
            ctx.memory_mut(|m| m.request_focus(id));
            self.text_focus = Some((id, t));
            ctx.request_repaint();
        }
    }

    fn toggle_mail(&mut self, ctx: &egui::Context) {
        self.show_mail = !self.show_mail;
        if self.show_mail && self.mails.is_empty() {
            self.mail_refresh(ctx);
        }
    }

    fn apply(&mut self, action: Option<Action>) {
        match action {
            Some(Action::Detail(c)) => self.detail = Some(c),
            Some(Action::OpenDay(d)) => {
                self.selected = d;
                self.view = View::Day;
            }
            Some(Action::Select(d)) => self.selected = d,
            Some(Action::Restos) => self.show_resto = true,
            None => {}
        }
    }

    fn title_period(&self) -> String {
        let d = self.selected;
        if self.view == View::Day {
            return format!("{} {}", MOIS[d.month0() as usize], d.year()).to_uppercase();
        }
        let (a, b) = (monday(d), monday(d) + Duration::days(6));
        if a.month() == b.month() {
            format!("{} {}", MOIS[a.month0() as usize], a.year()).to_uppercase()
        } else {
            format!("{} – {} {}", MOIS[a.month0() as usize], MOIS[b.month0() as usize], b.year()).to_uppercase()
        }
    }

    fn title_block(&self, ui: &mut egui::Ui, size: f32) {
        let sq = size * 0.34;
        let (r, _) = ui.allocate_exact_size(egui::vec2(sq, sq), egui::Sense::hover());
        ui.painter().rect_filled(r, egui::CornerRadius::ZERO, theme::CM);
        ui.label(RichText::new("CYDERA").font(bold(size)));
        // Curseur de terminal : fixe au repos, il bat uniquement pendant une synchronisation (il porte une info).
        let cur_a = if self.syncing { theme::blink(ui.ctx()) } else { 1.0 };
        ui.label(RichText::new("▌").font(theme::mono_bold(size * 0.8)).color(theme::ACCENT.gamma_multiply(cur_a)));
        ui.add_space(if size > 24.0 { 8.0 } else { 2.0 });
        ui.label(RichText::new(format!("// {}", self.title_period())).font(theme::mono(12.0)).color(theme::MUTED));
        if self.view == View::Week {
            let iso = monday(self.selected).iso_week().week();
            ui.label(RichText::new(format!("· S{iso}")).font(theme::mono(11.5)).color(theme::DIM));
        }
    }

    fn rev_label(&self) -> (String, egui::Color32) {
        match (&self.chrono.subject, self.chrono.running()) {
            (Some(s), true) => (format!("⏱ {} · {}", fmt_hms(self.chrono.run_secs()), s.to_uppercase()), theme::OK),
            _ => ("⏱ RÉVISIONS".to_string(), theme::TEXT),
        }
    }

    /// Une action secondaire de l'en-tête : bouton direct (écran large) ou entrée du menu.
    fn action(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, which: Act, menu: bool) {
        let btn = |ui: &mut egui::Ui, text: String, col: egui::Color32, sel: bool, tip: &str, enabled: bool| -> bool {
            let mut b = egui::Button::new(RichText::new(text.to_uppercase()).color(col)).selected(sel);
            if menu {
                // Cibles généreuses (HIG accessibility.md › Controls) + raccourci clavier affiché à droite.
                let key = match which {
                    Act::Resto => "U",
                    Act::Exams => "E",
                    Act::Mails => "M",
                    Act::Rev => "C",
                    Act::Refresh => "R",
                    Act::Grades => "N",
                    Act::About | Act::Devices => "",
                };
                b = b.min_size(egui::vec2(260.0, 40.0)).shortcut_text(RichText::new(key).font(theme::mono(11.0)).color(theme::DIM));
            }
            ui.add_enabled(enabled, b).on_hover_text(tip).clicked()
        };
        let clicked = match which {
            Act::Resto => {
                let c = btn(ui, "Resto U".to_string(), theme::TEXT, self.show_resto, "Menus des restos U (U)", true);
                if c {
                    self.toggle_resto(ctx);
                }
                c
            }
            Act::Exams => {
                let n = self.upcoming_exams().len();
                let next = self.upcoming_exams().first().map(|c| (c.date() - now().date()).num_days());
                let txt = match next {
                    Some(0) => format!("Examens · aujourd'hui ({n})"),
                    Some(d) => format!("Examens · J-{d} ({n})"),
                    None => "Examens".to_string(),
                };
                let col = match next {
                    Some(d) if d <= 7 => theme::NOW,
                    Some(d) if d <= 21 => theme::EXAM,
                    _ => theme::TEXT,
                };
                let c = btn(ui, txt, col, self.show_exams, "Compte à rebours des examens (E)", true);
                if c {
                    self.show_exams = !self.show_exams;
                }
                c
            }
            Act::Mails => {
                let unread = self.mails.iter().filter(|m| m.unread).count();
                let label = if unread > 0 { format!("Mails ({unread})") } else { "Mails".to_string() };
                let c = btn(ui, label, theme::TEXT, self.show_mail, "Boîte de réception (M)", true);
                if c {
                    self.toggle_mail(ctx);
                }
                c
            }
            Act::Rev => {
                let (t, col) = self.rev_label();
                let c = btn(ui, t, col, self.show_rev, "Chrono de révisions (C · Espace = start/pause)", true);
                if c {
                    self.show_rev = !self.show_rev;
                }
                c
            }
            Act::Grades => {
                let c = btn(ui, "Notes".to_string(), theme::TEXT, self.show_grades, "Moyennes et notes (N)", true);
                if c {
                    self.show_grades = !self.show_grades;
                }
                c
            }
            Act::Devices => {
                let n = self.p2p.peers().len();
                // En-tête de bureau très serré : version compacte (le menu garde le libellé complet).
                let label = match (menu, n) {
                    (true, 0) => "Appareils".to_string(),
                    (true, n) => format!("Appareils ({n})"),
                    (false, 0) => "P2P".to_string(),
                    (false, n) => format!("P2P · {n}"),
                };
                let c = btn(ui, label, theme::TEXT, self.show_devices, "Appareils associés et synchro", true);
                if c {
                    self.show_devices = !self.show_devices;
                }
                c
            }
            Act::About => {
                let c = btn(ui, "Infos".to_string(), theme::TEXT, self.show_about, "À propos", true);
                if c {
                    self.show_about = !self.show_about;
                }
                c
            }
            Act::Refresh => {
                let label = if self.syncing { "Synchro…" } else { "Actualiser" };
                let c = btn(ui, label.to_string(), theme::TEXT, false, "R / F5", !self.syncing && self.password.is_some());
                if c {
                    self.maybe_sync(ctx, true);
                }
                if self.syncing && !menu {
                    ui.spinner();
                }
                c
            }
        };
        if clicked && menu {
            ui.close();
        }
    }

    /// Sélecteur Semaine / Jour : l'indicateur glisse sur un ressort (vitesse conservée si on change
    /// d'avis en route) ; l'appui est visible dès le `pointer-down`, la bascule se fait au relâchement.
    fn view_toggle(&mut self, ui: &mut egui::Ui, rtl: bool) {
        let cell = egui::vec2(84.0, 30.0);
        let order: [(&str, View, &str); 2] = if rtl {
            [("JOUR", View::Day, "J"), ("SEMAINE", View::Week, "S")]
        } else {
            [("SEMAINE", View::Week, "S"), ("JOUR", View::Day, "J")]
        };
        let (rect, _) = ui.allocate_exact_size(egui::vec2(cell.x * 2.0 + 6.0, cell.y + 6.0), egui::Sense::hover());
        let p = ui.painter().clone();
        p.rect(rect, CornerRadius::same(3), theme::CARD, egui::Stroke::new(1.0, theme::LINE), egui::StrokeKind::Inside);
        let idx = order.iter().position(|o| o.1 == self.view).unwrap_or(0) as f32;
        let sid = ui.id().with("view_toggle");
        let pos = ui::motion::spring(ui.ctx(), sid, idx);
        let ind = egui::Rect::from_min_size(rect.min + egui::vec2(3.0 + cell.x * pos, 3.0), cell);
        p.rect_filled(ind, CornerRadius::same(2), theme::ACCENT);
        let mut chosen = None;
        for (i, (label, v, key)) in order.iter().enumerate() {
            let r = egui::Rect::from_min_size(rect.min + egui::vec2(3.0 + cell.x * i as f32, 3.0), cell);
            let resp = ui.interact(r, sid.with(i), egui::Sense::click());
            resp.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, self.view == *v, *label));
            let (hv, pr) = ui::motion::press(ui.ctx(), sid.with(("c", i)), resp.hovered(), resp.is_pointer_button_down_on());
            if pr > 0.0 && self.view != *v {
                p.rect_filled(r, CornerRadius::same(2), theme::PRESS.gamma_multiply(pr));
            }
            // texte noir sur l'indicateur cyan (9,9:1), clair ailleurs ; fondu selon la distance à l'indicateur
            let near = (1.0 - (pos - i as f32).abs()).clamp(0.0, 1.0);
            let col = ui::motion::lerp_color(ui::motion::lerp_color(theme::MUTED, theme::TEXT, hv), Color32::BLACK, near);
            p.text(r.center(), egui::Align2::CENTER_CENTER, *label, theme::mono_bold(12.0), col);
            if resp.on_hover_text(*key).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                chosen = Some(*v);
            }
        }
        if let Some(v) = chosen {
            self.view = v;
        }
    }

    fn nav(&mut self, ui: &mut egui::Ui, rtl: bool) {
        let mut go: Option<i64> = None;
        let mut today = false;
        let items: [u8; 3] = if rtl { [2, 1, 0] } else { [0, 1, 2] };
        for it in items {
            match it {
                0 => {
                    if ui.button("⏴").on_hover_text("Précédent (flèche gauche)").clicked() {
                        go = Some(-1);
                    }
                }
                1 => {
                    if ui.button("AUJOURD'HUI").on_hover_text("T").clicked() {
                        today = true;
                    }
                }
                _ => {
                    if ui.button("⏵").on_hover_text("Suivant (flèche droite)").clicked() {
                        go = Some(1);
                    }
                }
            }
        }
        if let Some(d) = go {
            self.step(d);
        }
        if today {
            self.selected = now().date();
        }
    }

    fn burger(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let unread = self.mails.iter().filter(|m| m.unread).count();
        let label = if unread > 0 { format!("Menu ({unread})") } else { "Menu".to_string() };
        ui.menu_button(label, |ui| {
            ui.set_min_width(240.0);
            for a in [Act::Resto, Act::Exams, Act::Grades, Act::Mails, Act::Rev, Act::Devices, Act::About, Act::Refresh] {
                self.action(ui, ctx, a, true);
            }
        });
    }

    fn header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, bp: Bp) {
        let rtl = egui::Layout::right_to_left(egui::Align::Center);
        match bp {
            Bp::Wide => {
                ui.horizontal(|ui| {
                    self.title_block(ui, 28.0);
                    ui.with_layout(rtl, |ui| {
                        for a in [Act::Refresh, Act::About, Act::Devices, Act::Rev, Act::Mails, Act::Resto, Act::Grades, Act::Exams] {
                            self.action(ui, ctx, a, false);
                            ui.add_space(8.0);
                        }
                        self.view_toggle(ui, true);
                        ui.add_space(8.0);
                        self.nav(ui, true);
                    });
                });
            }
            Bp::Medium => {
                ui.horizontal(|ui| {
                    self.title_block(ui, 26.0);
                    ui.with_layout(rtl, |ui| {
                        self.burger(ui, ctx);
                        if self.chrono.running() {
                            ui.add_space(8.0);
                            self.action(ui, ctx, Act::Rev, false);
                        }
                        ui.add_space(8.0);
                        self.view_toggle(ui, true);
                        ui.add_space(8.0);
                        self.nav(ui, true);
                    });
                });
            }
            Bp::Mobile => {
                ui.horizontal(|ui| {
                    // logo centré sur toute la largeur, dessiné au pinceau (indépendant des boutons à droite)
                    let row = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 34.0));
                    ui.allocate_exact_size(egui::vec2(0.0, 34.0), egui::Sense::hover());
                    let p = ui.painter();
                    let g = p.layout_no_wrap("CYDERA".into(), bold(20.0), theme::TEXT);
                    let cur_a = if self.syncing { theme::blink(ui.ctx()) } else { 1.0 };
                    let (sq, gap, bar) = (7.0, 7.0, 4.0);
                    let total = sq + gap + g.size().x + gap + bar;
                    let x = row.center().x - total / 2.0;
                    let cy = row.center().y;
                    p.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(x, cy - sq / 2.0), egui::vec2(sq, sq)),
                        egui::CornerRadius::same(2),
                        theme::CM,
                    );
                    let gh = g.size().y;
                    p.galley(egui::pos2(x + sq + gap, cy - gh / 2.0), g, theme::TEXT);
                    let bx = x + total - bar;
                    p.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(bx, cy - 10.0), egui::vec2(bar, 20.0)),
                        egui::CornerRadius::ZERO,
                        theme::ACCENT.gamma_multiply(cur_a),
                    );
                    ui.with_layout(rtl, |ui| {
                        let today = now().date();
                        let on_today = match self.view {
                            View::Day => self.selected == today,
                            View::Week => monday(self.selected) == monday(today),
                        };
                        if !on_today && ui::mobile::pill_button(ui, "AUJOURD'HUI").on_hover_text("T").clicked() {
                            self.selected = today;
                        }
                    });
                });
                ui.horizontal(|ui| {
                    let mut period = capitalize_first(&self.title_period().to_lowercase());
                    if self.view == View::Week {
                        period += &format!(" · semaine {}", monday(self.selected).iso_week().week());
                    }
                    ui.label(RichText::new(period).font(regular(13.0)).color(theme::MUTED));
                    ui.with_layout(rtl, |ui| {
                        let (t, c) = self.sync_short();
                        ui.label(RichText::new(t).font(regular(12.5)).color(c));
                        // pastille d'état dessinée (les glyphes ▲ ◌ manquent dans la police proportionnelle)
                        let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                        if self.syncing {
                            ui.painter().circle_stroke(r.center(), 3.5, egui::Stroke::new(1.5, c.gamma_multiply(theme::blink(ui.ctx()))));
                        } else {
                            ui.painter().circle_filled(r.center(), 4.0, c);
                        }
                    });
                });
                if self.chrono.running() {
                    self.action(ui, ctx, Act::Rev, false);
                }
            }
        }
    }

    /// État de la synchro, version courte (en-tête mobile). Le détail est dans la feuille « Plus ».
    fn sync_short(&self) -> (String, egui::Color32) {
        if self.password.is_none() {
            ("Connexion requise".into(), theme::NOW)
        } else if self.error.is_some() {
            ("Hors ligne".into(), theme::WARN)
        } else if self.syncing {
            ("Synchro…".into(), theme::ACCENT)
        } else if let Some(a) = self.store.last_sync_min() {
            let when = if a < 1 { "à l'instant".to_string() } else { format!("il y a {}", fmt_duration(a)) };
            (format!("À jour · {when}"), theme::OK)
        } else {
            ("En attente".into(), theme::MUTED)
        }
    }

    /// État de la synchro, version longue (barre d'état sur PC, feuille « Plus » sur mobile).
    fn sync_long(&self) -> (String, egui::Color32) {
        let age = self.store.last_sync_min();
        if self.password.is_none() {
            ("Mot de passe introuvable : exporter CELCAT_PASSWORD ou créer ~/CELCAT_PASSWORD".to_string(), theme::NOW)
        } else if let Some(e) = &self.error {
            let cache = age.map(|a| format!(" — cache d'il y a {}", fmt_duration(a))).unwrap_or_default();
            (format!("▲ HORS LIGNE · {e}{cache}"), theme::WARN)
        } else if self.syncing {
            ("◌ SYNC EN COURS · Celcat…".to_string(), theme::ACCENT)
        } else if let Some(a) = age {
            let when = if a < 1 { "à l'instant".to_string() } else { format!("il y a {}", fmt_duration(a)) };
            (format!("● SYNC OK · {when}"), theme::OK)
        } else {
            ("○ EN ATTENTE · première synchro…".to_string(), theme::MUTED)
        }
    }

    fn active_tab(&self) -> Option<ui::mobile::Tab> {
        use ui::mobile::Tab;
        if self.show_more {
            Some(Tab::More)
        } else if self.show_mail {
            Some(Tab::Mails)
        } else if self.show_grades {
            Some(Tab::Grades)
        } else if self.show_rev {
            Some(Tab::Rev)
        } else if self.show_exams {
            Some(Tab::Exams)
        } else {
            None
        }
    }

    /// Page mobile affichée par-dessus les autres (pile de navigation : la plus « profonde » d'abord).
    fn top_page(&self) -> Option<&'static str> {
        if self.password.is_none() {
            Some("login")
        } else if self.pair_req.is_some() {
            Some("pair-request")
        } else if self.detail.is_some() {
            Some("detail")
        } else if self.maj.show {
            Some("update")
        } else if self.show_devices {
            Some("devices")
        } else if self.show_about {
            Some("about")
        } else if self.show_grades {
            Some("grades")
        } else if self.show_mail {
            Some("mail")
        } else if self.show_resto {
            Some("resto")
        } else if self.show_rev {
            Some("revisions")
        } else if self.show_exams {
            Some("exams")
        } else {
            None
        }
    }

    /// Ferme la page du dessus (retour Android). Renvoie faux s'il n'y en avait aucune.
    fn close_top_page(&mut self) -> bool {
        match self.top_page() {
            Some("detail") => self.detail = None,
            Some("devices") => self.show_devices = false,
            Some("update") => self.maj.show = false,
            Some("about") => self.show_about = false,
            Some("grades") => {
                if !self.grades_ui.back() {
                    self.show_grades = false;
                }
            }
            Some("mail") if self.mail_sel.is_some() => {
                self.mail_sel = None;
                self.mail_body = None;
            }
            Some("mail") => self.show_mail = false,
            Some("resto") => self.show_resto = false,
            Some("revisions") => self.show_rev = false,
            Some("exams") => self.show_exams = false,
            _ => return false,
        }
        true
    }

    fn on_tab(&mut self, ctx: &egui::Context, t: ui::mobile::Tab) {
        use ui::mobile::Tab;
        let was = self.active_tab();
        // un seul onglet ouvert à la fois ; toucher l'onglet actif le referme (retour à l'emploi du temps)
        self.show_more = false;
        self.show_exams = false;
        self.show_rev = false;
        self.show_resto = false;
        self.show_mail = false;
        // les pages ouvertes depuis « Plus » ou une carte de cours ne restent pas devant le nouvel onglet
        self.show_about = false;
        self.show_devices = false;
        self.show_grades = false;
        self.maj.show = false;
        self.detail = None;
        ctx.request_repaint(); // l'en-tête a déjà été dessiné avec l'ancien état
        if was == Some(t) {
            return;
        }
        match t {
            Tab::Exams => self.show_exams = true,
            Tab::Rev => self.show_rev = true,
            Tab::Grades => self.show_grades = true,
            Tab::Mails => self.toggle_mail(ctx),
            Tab::More => self.show_more = true,
        }
    }

    /// Feuille « Plus » : actions secondaires en grandes lignes, posée au-dessus de la barre d'onglets.
    fn more_sheet(&mut self, ctx: &egui::Context, bottom: f32) {
        let vp = ctx.viewport_rect();
        let area = egui::Rect::from_min_max(vp.min, egui::pos2(vp.right(), bottom));
        let id = egui::Id::new("more_sheet");
        let peers = self.p2p.peers().len();
        let dev_detail = if peers == 0 { String::new() } else { format!("{peers} associé{}", if peers > 1 { "s" } else { "" }) };
        let (sync_t, sync_c) = self.sync_long();
        let p2p = devices::status_line(&self.p2p.status());
        let (short, short_c) = self.sync_short();
        let can_sync = !self.syncing && self.password.is_some();
        let rows_h = 4.0 * 52.0;
        let h = 22.0 + rows_h + 14.0 + if p2p.is_some() { 40.0 } else { 24.0 } + 12.0;
        let mut pick: Option<u8> = None;
        let mut close = false;
        egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(area.min).movable(false).show(ctx, |ui| {
            ui.set_min_size(area.size());
            let bg = ui.interact(area, id.with("bg"), egui::Sense::click());
            ui.painter().rect_filled(area, 0.0, Color32::from_black_alpha(150));
            if bg.clicked() {
                close = true;
            }
            let sheet =
                egui::Rect::from_min_max(egui::pos2(area.left() + 8.0, bottom - h - 8.0), egui::pos2(area.right() - 8.0, bottom - 8.0));
            // bloque les appuis sous la feuille
            let _ = ui.interact(sheet, id.with("sheet"), egui::Sense::click());
            ui.painter().rect(
                sheet,
                egui::CornerRadius::same(20),
                theme::PANEL,
                egui::Stroke::new(1.0, theme::LINE),
                egui::StrokeKind::Inside,
            );
            let grab = egui::Rect::from_center_size(egui::pos2(sheet.center().x, sheet.top() + 10.0), egui::vec2(36.0, 4.0));
            ui.painter().rect_filled(grab, egui::CornerRadius::same(2), theme::LINE);
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(sheet.shrink2(egui::vec2(8.0, 0.0)).with_min_y(sheet.top() + 22.0))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            );
            let ui = &mut child;
            ui.spacing_mut().item_spacing.y = 0.0;
            // Resto U : l'onglet a laissé sa place aux notes (accessible aussi depuis la pause déjeuner)
            if ui::mobile::sheet_row(ui, "Resto U", "Menus du jour", theme::MUTED, true) {
                pick = Some(5);
            }
            if ui::mobile::sheet_row(ui, "Appareils", &dev_detail, theme::MUTED, true) {
                pick = Some(2);
            }
            if ui::mobile::sheet_row(ui, "Actualiser", &short, short_c, can_sync) {
                pick = Some(3);
            }
            if ui::mobile::sheet_row(ui, "À propos", "", theme::MUTED, true) {
                pick = Some(4);
            }
            ui.add_space(10.0);
            ui.add(egui::Label::new(RichText::new(&sync_t).font(theme::mono(11.0)).color(sync_c)).truncate());
            if let Some((t, c)) = &p2p {
                ui.add(egui::Label::new(RichText::new(t).font(theme::mono(11.0)).color(*c)).truncate());
            }
        });
        if let Some(k) = pick {
            close = true;
            match k {
                2 => self.show_devices = true,
                3 => self.maybe_sync(ctx, true),
                5 => self.toggle_resto(ctx),
                _ => self.show_about = true,
            }
        }
        if close {
            self.show_more = false;
        }
    }

    fn status_bar(&self, ui: &mut egui::Ui, hints: bool) {
        let p2p = devices::status_line(&self.p2p.status());
        if let (Some((t, c)), false) = (&p2p, hints) {
            // Écran étroit : l'état de la synchro entre appareils sur sa propre ligne.
            ui.label(RichText::new(t).font(theme::mono(11.0)).color(*c));
        }
        ui.horizontal(|ui| {
            let (txt, col) = self.sync_long();
            ui.label(RichText::new("$").font(theme::mono_bold(11.0)).color(theme::DIM));
            ui.label(RichText::new(txt).font(theme::mono(11.0)).color(col));
            if !hints {
                return;
            }
            if let Some((t, c)) = &p2p {
                ui.label(RichText::new("·").font(theme::mono(11.0)).color(theme::DIM));
                ui.label(RichText::new(t).font(theme::mono(11.0)).color(*c));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("⏴ ⏵ naviguer · T aujourd'hui · S/J vue · E examens · N notes · U resto · C révisions · M mails · P panneau · R actualiser")
                        .font(theme::mono(10.5))
                        .color(theme::DIM),
                );
            });
        });
    }

    fn detail_window(&mut self, ctx: &egui::Context) {
        let Some(c) = self.detail.clone() else { return };
        let mut open = true;
        let col = theme::kind_color(c.kind);
        let page = ui::page::is_page(ctx);
        let win = egui::Window::new(RichText::new(&c.matiere).font(bold(18.0)))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(420.0_f32.min(ctx.content_rect().width() - 24.0));
        ui::page::show(ctx, egui::Id::new("detail"), "Cours", win, Some(&mut open), true, |ui| {
            if page {
                ui.add(egui::Label::new(RichText::new(&c.matiere).font(bold(20.0))).wrap());
                ui.add_space(6.0);
            }
            ui.horizontal(|ui| {
                ui::card::pill(ui, &c.category, col);
                ui.label(RichText::new(fmt_duration(c.minutes())).color(theme::MUTED));
            });
            ui.add_space(8.0);
            egui::Grid::new("detail_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(theme::DIM));
                    ui.add(egui::Label::new(v).wrap());
                    ui.end_row();
                };
                row(ui, "Date", capitalize(&date_longue(c.date())) + &format!(" {}", c.date().year()));
                row(ui, "Horaire", format!("{} – {}", c.start.format("%H:%M"), c.end.format("%H:%M")));
                row(ui, "Enseignant", c.prof.clone());
                row(ui, "Salle", c.salle.clone());
                row(ui, "Type", c.category.clone());
            });
            ui.add_space(6.0);
            let copy = egui::Button::new("Copier")
                .min_size(egui::vec2(if page { ui.available_width() } else { 0.0 }, if page { 48.0 } else { 0.0 }));
            if ui.add(copy).on_hover_text("Copier le résumé dans le presse-papiers").clicked() {
                ui.ctx().copy_text(format!(
                    "{} — {} {}–{} — {} — {}",
                    c.label(),
                    date_longue(c.date()),
                    c.start.format("%H:%M"),
                    c.end.format("%H:%M"),
                    c.salle,
                    c.prof
                ));
            }
        });
        if !open {
            self.detail = None;
        }
    }
}

impl eframe::App for App {
    /// Injecte les marges système (Android) : `content_rect()` d'egui les exclut ensuite.
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(t) = self.pending_paste.take() {
            raw.events.push(egui::Event::Paste(t));
        }
        if let Some([top, right, bottom, left]) = crate::insets::current_px() {
            // Clavier en mouvement : on note l'instant pour redessiner à chaque image (cf. `ui`).
            if self.last_insets.0 != [top, right, bottom, left] {
                self.last_insets = ([top, right, bottom, left], ctx.input(|i| i.time));
            }
            let k = ctx.pixels_per_point().max(0.5);
            raw.safe_area_insets =
                Some(egui::SafeAreaInsets(egui::epaint::MarginF32 { left: left / k, right: right / k, top: top / k, bottom: bottom / k }));
        }
    }

    fn on_exit(&mut self) {
        // Enregistre la session en cours si on quitte pendant un chrono.
        self.chrono.pause();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::paint_background(&ctx);
        self.poll();
        if self.p2p.take_changed() {
            self.chrono.reload_from_journal(); // des données sont arrivées d'un autre appareil
            self.grades.reload();
        }
        // Fiche ECTS (PDF) glissée sur la fenêtre : import dans l'écran Notes.
        if crate::files::can_import() {
            let dropped: Vec<egui::DroppedFile> = ctx.input(|i| i.raw.dropped_files.clone());
            for f in dropped {
                let is_pdf = f.name.to_lowercase().ends_with(".pdf")
                    || f.path.as_ref().is_some_and(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")));
                let bytes = f.bytes.map(|b| b.to_vec()).or_else(|| f.path.as_ref().and_then(|p| std::fs::read(p).ok()));
                if let (true, Some(b)) = (is_pdf, bytes) {
                    self.show_grades = true;
                    self.grades_ui.import_bytes(&ctx, b);
                    break;
                }
            }
        }
        self.p2p.tick();
        let force = self.handle_keys(&ctx);
        self.maybe_sync(&ctx, force);
        // Horloge / ligne « maintenant » / synchro périodique
        self.chrono.tick();
        let refresh = if self.show_side || self.chrono.running() { 100 } else { 20_000 };
        ctx.request_repaint_after(std::time::Duration::from_millis(refresh));
        // Pendant l'animation du clavier, les marges changent à chaque image : on suit image par image
        // (sinon la mise en page ne se met à jour que tous les 100 ms et saute).
        let t = ctx.input(|i| i.time);
        if t - self.last_insets.1 < 0.6 || ctx.egui_wants_keyboard_input() && t - self.last_insets.1 < 1.5 {
            ctx.request_repaint();
        }
        let now = now();

        let bp = breakpoint(&ctx);
        if bp == Bp::Mobile && self.last_bp != Some(Bp::Mobile) {
            self.view = View::Day; // sur mobile, la vue « Jour » est la vue par défaut
        }
        self.last_bp = Some(bp);
        // Cibles : 28 px sur bureau, 40 px en mobile/tactile (HIG accessibility.md › Controls).
        let target = if bp == Bp::Mobile { 40.0 } else { 28.0 };
        if (ctx.global_style().spacing.interact_size.y - target).abs() > 0.1 {
            ctx.global_style_mut(|s| s.spacing.interact_size.y = target);
        }
        // Le Ui racine couvre toute la fenêtre : on applique nous-mêmes les marges système.
        let (vr, cr) = (ctx.viewport_rect(), ctx.content_rect());
        let (il, it, ir, ib) = (
            (cr.left() - vr.left()).clamp(0.0, 100.0).round() as i8,
            (cr.top() - vr.top()).clamp(0.0, 100.0).round() as i8,
            (vr.right() - cr.right()).clamp(0.0, 100.0).round() as i8,
            (vr.bottom() - cr.bottom()).clamp(0.0, 100.0).round() as i8,
        );
        let screen = ctx.content_rect();
        let (pad_x, pad_top): (i8, i8) = if bp == Bp::Mobile { (12, 10) } else { (28, 14) };
        let hints = bp != Bp::Mobile && screen.width() >= 1000.0 && screen.height() >= 640.0;

        egui::Panel::top("header")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                left: pad_x.saturating_add(il),
                right: (pad_x - 8).saturating_add(ir),
                top: pad_top.saturating_add(it),
                bottom: 6,
            }))
            .show(ui, |ui| self.header(ui, &ctx, bp));

        self.maj.auto(&self.rt, &ctx, self.cfg.maj_auto);
        if bp != Bp::Mobile
            && let Some(rel) = self.maj.bandeau(&self.cfg.maj_ignoree).cloned()
        {
            let a = egui::Panel::top("maj")
                .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                    left: pad_x.saturating_add(il),
                    right: (pad_x - 8).saturating_add(ir),
                    top: 2,
                    bottom: 6,
                }))
                .show(ui, |ui| ui::update::bandeau(ui, &rel, false))
                .inner;
            if let Some(a) = a {
                self.maj_action(&ctx, a);
            }
        }

        // Téléphone : barre d'onglets en bas (masquée pendant la saisie, le clavier occupe la place).
        let mut tab_top = None;
        if bp == Bp::Mobile {
            if !ctx.egui_wants_keyboard_input() {
                let unread = self.mails.iter().filter(|m| m.unread).count();
                let active = self.active_tab();
                let r = egui::Panel::bottom("tabs")
                    .frame(egui::Frame::new().fill(theme::PANEL).stroke(egui::Stroke::new(1.0, theme::LINE)).inner_margin(Margin {
                        left: 4i8.saturating_add(il),
                        right: 4i8.saturating_add(ir),
                        top: 2,
                        bottom: 2i8.saturating_add(ib),
                    }))
                    .show(ui, |ui| ui::mobile::tab_bar(ui, active, unread, self.chrono.running()));
                tab_top = Some(r.response.rect.top());
                if let Some(t) = r.inner {
                    self.on_tab(&ctx, t);
                }
                if let Some(rel) = self.maj.bandeau(&self.cfg.maj_ignoree).cloned() {
                    let a = egui::Panel::bottom("maj")
                        .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                            left: 10i8.saturating_add(il),
                            right: 10i8.saturating_add(ir),
                            top: 4,
                            bottom: 4,
                        }))
                        .show(ui, |ui| ui::update::bandeau(ui, &rel, true))
                        .inner;
                    if let Some(a) = a {
                        self.maj_action(&ctx, a);
                    }
                }
            }
        } else {
            egui::Panel::bottom("status")
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin {
                    left: 20i8.saturating_add(il),
                    right: 20i8.saturating_add(ir),
                    top: 6,
                    bottom: 6i8.saturating_add(ib),
                }))
                .show(ui, |ui| self.status_bar(ui, hints));
        }

        // Grand écran : panneau fixe à droite. Sinon : bandeau compact au-dessus de la grille.
        if self.show_side && bp == Bp::Wide {
            let mut action = None;
            egui::Panel::right("side")
                .resizable(true)
                .default_size(330.0)
                .size_range(260.0..=480.0)
                .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(Margin::same(20)))
                .show(ui, |ui| action = ui::side::show(ui, &self.store, now, self.selected));
            self.apply(action);
        } else if self.show_side && bp == Bp::Mobile {
            let action = egui::Panel::top("banner")
                .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                    left: 10i8.saturating_add(il),
                    right: 10i8.saturating_add(ir),
                    top: 2,
                    bottom: 4,
                }))
                .show(ui, |ui| ui::side::banner_mini(ui, &self.store, now))
                .inner;
            self.apply(action);
        } else if self.show_side {
            let mut action = None;
            egui::Panel::top("banner")
                .frame(egui::Frame::new().fill(theme::BG).inner_margin(Margin {
                    left: (pad_x - 4).saturating_add(il),
                    right: (pad_x - 8).saturating_add(ir),
                    top: 2,
                    bottom: 6,
                }))
                .show(ui, |ui| action = ui::side::banner(ui, &self.store, now, self.selected, bp == Bp::Medium));
            self.apply(action);
        }

        let mut action = None;
        let margin = if bp == Bp::Mobile {
            Margin { left: 10i8.saturating_add(il), right: 10i8.saturating_add(ir), top: 8, bottom: 4 }
        } else {
            Margin { left: 16i8.saturating_add(il), right: 20i8.saturating_add(ir), top: 8, bottom: 8 }
        };
        let mut gesture = None;
        let mut content = egui::Rect::NOTHING;
        let can_pull = self.password.is_some() && !self.syncing;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(theme::BG).inner_margin(margin)).show(ui, |ui| {
            content = ui.max_rect();
            if bp == Bp::Mobile {
                let cur = if self.view == View::Day { 0 } else { 1 };
                if let Some(i) = ui::mobile::segmented(ui, &["Jour", "Semaine"], cur) {
                    self.view = if i == 0 { View::Day } else { View::Week };
                }
                ui.add_space(10.0);
            }
            // Gestes : écran tactile ou layout mobile (sur PC, un glisser à la souris sert à sélectionner du texte).
            if bp == Bp::Mobile || ctx.input(|i| i.has_touch_screen()) {
                gesture = ui::gesture::track(&ctx, content, can_pull);
            }
            // Le contenu suit le doigt : on le dessine décalé, en coupant ce qui dépasserait en bas.
            let off = ui::gesture::offset(&ctx);
            ui.with_visual_transform(egui::emath::TSTransform::from_translation(off), |ui| {
                let mut clip = ui.clip_rect();
                clip.max.y -= off.y.max(0.0);
                ui.set_clip_rect(clip);
                action = match self.view {
                    View::Week => ui::week::show(ui, &self.store, monday(self.selected), now),
                    View::Day => ui::day::show(ui, &self.store, self.selected, now, bp == Bp::Mobile),
                };
            });
        });
        ui::gesture::paint_pull(&ctx, content);
        match gesture {
            Some(ui::gesture::Gesture::Swipe(dir)) => {
                self.step(dir);
                ctx.request_repaint();
            }
            Some(ui::gesture::Gesture::Refresh) => self.maybe_sync(&ctx, true),
            None => {}
        }
        self.apply(action);
        // Téléphone : écrans secondaires en pages plein écran, entre la barre système et les onglets.
        if bp == Bp::Mobile {
            let mut r = ctx.content_rect();
            if let Some(b) = tab_top {
                r.max.y = b;
            }
            let bg_bottom = tab_top.unwrap_or(ctx.viewport_rect().bottom());
            let top = self.top_page().map_or(egui::Id::NULL, egui::Id::new);
            ui::page::set_area(&ctx, Some((r, top, bg_bottom)));
        } else {
            ui::page::set_area(&ctx, None);
        }
        let exams = self.exam_refs();
        self.exams_window(&ctx);
        if self.show_rev {
            let mut open = true;
            revisions::show(&ctx, &mut self.chrono, &mut self.rev_ui, &exams, &mut open);
            self.show_rev = open;
        }
        self.resto_window(&ctx);
        self.about_window(&ctx);
        if let Some(a) = ui::update::page(&ctx, &mut self.maj) {
            self.maj_action(&ctx, a);
        }
        if self.show_grades {
            let mut open = true;
            grades_ui::show(&ctx, &mut self.grades, &mut self.grades_ui, &mut open);
            self.show_grades = open;
        }
        self.mail_window(&ctx);
        self.detail_window(&ctx);
        self.login_window(&ctx);
        self.devices_window(&ctx);
        if let (true, Some(bottom)) = (self.show_more, tab_top) {
            self.more_sheet(&ctx, bottom);
        }
        self.clipboard_bridge(&ctx);
    }
}

fn capitalize_first(s: &str) -> String {
    ui::capitalize(s)
}
