use crate::cache::{Store, monday};
use crate::config::{Config, load_mail_password, load_password};
use crate::grades::Book;
use crate::mail::{self, Folder, Header};
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

mod about;
mod account;
mod chrome;
mod exams;
mod keys;
mod mail_win;
mod resto_win;

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
    mail: mail_win::Mail,
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
        let (resto_tx, resto_rx) = mpsc::channel();
        let (live_tx, live_rx) = mpsc::channel();
        let today = now().date();
        let chrono = Chrono::load();
        let grades = Book::new(chrono.journal_handle());
        let p2p = P2p::start(rt.clone(), ctx, chrono.journal_handle());
        let cfg = Config::load();
        // Sans identifiant, le mot de passe seul ne sert à rien : on repasse par l'écran de connexion.
        let password = if cfg.username.trim().is_empty() { None } else { load_password() };
        let mail = mail_win::Mail::new(rt.clone(), &cfg.mail_host, &cfg.mail_user, load_mail_password());
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
            mail,
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

    fn poll(&mut self, ctx: &egui::Context) {
        self.maj.poll();
        self.mail.poll(ctx);
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
        self.poll(&ctx);
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
                let unread = self.mail.unread();
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
        self.mail.window(&ctx);
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
