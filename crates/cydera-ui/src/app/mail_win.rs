//! Mails : état (`Mail`), fenêtre (dossiers, recherche, liste, lecture) et actions
//! (lu / non lu, corbeille, spam). Les listes et les messages lus sont gardés en cache disque
//! pour la lecture hors ligne.
use super::*;
use crate::mail::MsgAction;
use crate::mail_cache::MailCache;
use std::cell::Cell;

/// Messages demandés par liste.
const PAGE: u32 = 40;
/// Au-delà, rouvrir la fenêtre relance une vérification.
const STALE_SECS: u64 = 60;

enum MailMsg {
    List { folder: Folder, query: String, r: Result<Vec<Header>, String> },
    Body(u32, Result<mail::Body, String>),
    Action(Result<(), String>),
}

pub(super) struct Mail {
    pub show: bool,
    rt: tokio::runtime::Handle,
    host: String,
    user: String,
    pwd: Option<String>,
    folder: Folder,
    list: Vec<Header>,
    loading: bool,
    error: Option<String>,
    sel: Option<u32>,
    body: Option<Result<mail::Body, String>>,
    /// Fait défiler la liste jusqu'au message sélectionné au clavier.
    scroll: Cell<bool>,
    /// Destinataires du message ouvert dépliés.
    rcpt_open: Cell<bool>,
    /// Non lus de la boîte de réception (bouton « Mails », quel que soit le dossier ouvert).
    unread: usize,
    /// Champ de recherche et recherche appliquée à la liste affichée.
    search: String,
    query: String,
    cache: MailCache,
    last_refresh: Option<Instant>,
    tx: Sender<MailMsg>,
    rx: Receiver<MailMsg>,
}

/// Ce que l'utilisateur a demandé pendant l'affichage (appliqué après, hors des emprunts).
enum Ui {
    Open(u32),
    Back,
    Folder(Folder),
    Search,
    ClearSearch,
    Refresh,
    Act(u32, MsgAction),
}

impl Mail {
    pub fn new(rt: tokio::runtime::Handle, host: &str, user: &str, pwd: Option<String>) -> Self {
        let (tx, rx) = mpsc::channel();
        let mut m = Self {
            show: false,
            rt,
            host: String::new(),
            user: String::new(),
            pwd: None,
            folder: Folder::Inbox,
            list: Vec::new(),
            loading: false,
            error: None,
            sel: None,
            body: None,
            scroll: Cell::new(false),
            rcpt_open: Cell::new(false),
            unread: 0,
            search: String::new(),
            query: String::new(),
            cache: MailCache::default(),
            last_refresh: None,
            tx,
            rx,
        };
        m.set_account(host, user, pwd);
        m
    }

    /// Compte webmail (au démarrage et après une connexion) : recharge le cache de ce compte.
    pub fn set_account(&mut self, host: &str, user: &str, pwd: Option<String>) {
        let changed = self.user != user || self.host != host;
        self.host = host.to_string();
        self.user = user.to_string();
        self.pwd = pwd;
        if changed {
            self.cache = MailCache::load(user);
            self.folder = Folder::Inbox;
            self.reset_view();
            self.unread = self.list.iter().filter(|m| m.unread).count();
            self.last_refresh = None;
        }
    }

    fn ready(&self) -> bool {
        self.pwd.is_some() && !self.user.is_empty()
    }

    pub fn unread(&self) -> usize {
        self.unread
    }

    /// Un message est ouvert.
    pub fn reading(&self) -> bool {
        self.sel.is_some()
    }

    /// Referme le message ouvert ; `false` s'il n'y en avait pas.
    pub fn close_message(&mut self) -> bool {
        let was = self.sel.is_some();
        self.sel = None;
        self.body = None;
        was
    }

    pub fn toggle(&mut self, ctx: &egui::Context) {
        self.show = !self.show;
        if self.show && (self.list.is_empty() || self.last_refresh.is_none_or(|t| t.elapsed().as_secs() >= STALE_SECS)) {
            self.refresh(ctx);
        }
    }

    /// Liste du dossier courant affichée depuis le cache, sans message ouvert ni recherche.
    fn reset_view(&mut self) {
        self.list = self.cache.headers(self.folder).map(<[Header]>::to_vec).unwrap_or_default();
        self.sel = None;
        self.body = None;
        self.error = None;
        self.search.clear();
        self.query.clear();
        self.loading = false;
    }

    fn account(&self) -> (String, String, String) {
        (self.host.clone(), self.user.clone(), self.pwd.clone().unwrap_or_default())
    }

    pub fn refresh(&mut self, ctx: &egui::Context) {
        if !self.ready() || self.loading {
            return;
        }
        self.loading = true;
        let (host, user, pwd) = self.account();
        let (tx, ctx, folder, query) = (self.tx.clone(), ctx.clone(), self.folder, self.query.clone());
        self.rt.spawn(async move {
            let r = mail::list(&host, &user, &pwd, folder, &query, PAGE).await.map_err(|e| e.to_string());
            let _ = tx.send(MailMsg::List { folder, query, r });
            ctx.request_repaint();
        });
    }

    fn open(&mut self, ctx: &egui::Context, uid: u32) {
        self.sel = Some(uid);
        self.rcpt_open.set(false);
        self.body = self.cache.body(uid).cloned().map(Ok);
        if self.body.is_none() {
            let (host, user, pwd) = self.account();
            let (tx, ctx) = (self.tx.clone(), ctx.clone());
            self.rt.spawn(async move {
                let r = mail::body(&host, &user, &pwd, uid).await.map_err(|e| e.to_string());
                let _ = tx.send(MailMsg::Body(uid, r));
                ctx.request_repaint();
            });
        }
        // Ouvrir un message non lu le marque comme lu, comme dans le webmail.
        if self.list.iter().any(|m| m.uid == uid && m.unread) {
            self.act(ctx, uid, MsgAction::Read);
        }
    }

    /// Message précédent / suivant de la liste (flèches haut / bas).
    pub fn step(&mut self, ctx: &egui::Context, dir: i32) {
        if self.list.is_empty() {
            return;
        }
        let cur = self.sel.and_then(|u| self.list.iter().position(|m| m.uid == u));
        let next = match cur {
            None => 0,
            Some(i) => (i as i32 + dir).clamp(0, self.list.len() as i32 - 1) as usize,
        };
        let uid = self.list[next].uid;
        if self.sel != Some(uid) {
            self.scroll.set(true);
            self.open(ctx, uid);
        }
    }

    /// Touche Suppr : met le message ouvert à la corbeille.
    pub fn trash_selected(&mut self, ctx: &egui::Context) {
        if let Some(uid) = self.sel
            && self.folder != Folder::Trash
        {
            self.act(ctx, uid, MsgAction::Trash);
        }
    }

    /// Applique l'action tout de suite à l'écran et au cache, puis l'envoie au serveur.
    fn act(&mut self, ctx: &egui::Context, uid: u32, a: MsgAction) {
        let inbox = self.folder == Folder::Inbox;
        let was_unread = self.list.iter().any(|m| m.uid == uid && m.unread);
        match a {
            MsgAction::Read | MsgAction::Unread => {
                let unread = a == MsgAction::Unread;
                self.list.iter_mut().filter(|m| m.uid == uid).for_each(|m| m.unread = unread);
                self.cache.update_header(uid, |m| m.unread = unread);
                if inbox && was_unread != unread {
                    self.unread = if unread { self.unread + 1 } else { self.unread.saturating_sub(1) };
                }
            }
            _ => {
                // déplacé hors du dossier affiché
                self.list.retain(|m| m.uid != uid);
                self.cache.remove_header(self.folder, uid);
                if inbox && was_unread {
                    self.unread = self.unread.saturating_sub(1);
                }
                if self.sel == Some(uid) {
                    self.close_message();
                }
            }
        }
        self.cache.save();
        let (host, user, pwd) = self.account();
        let (tx, ctx) = (self.tx.clone(), ctx.clone());
        self.rt.spawn(async move {
            let r = mail::action(&host, &user, &pwd, uid, a).await.map_err(|e| e.to_string());
            let _ = tx.send(MailMsg::Action(r));
            ctx.request_repaint();
        });
    }

    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut resync = false;
        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                MailMsg::List { folder, query, r } => {
                    if let Ok(l) = &r
                        && query.is_empty()
                    {
                        if folder == Folder::Inbox {
                            self.unread = l.iter().filter(|m| m.unread).count();
                        }
                        self.cache.set_headers(folder, l.clone());
                        self.cache.save();
                    }
                    // réponse d'un dossier ou d'une recherche quittés entre-temps : ignorée
                    if folder == self.folder && query == self.query {
                        self.loading = false;
                        match r {
                            Ok(l) => {
                                self.list = l;
                                self.error = None;
                                self.last_refresh = Some(Instant::now());
                            }
                            Err(e) => self.error = Some(e),
                        }
                    }
                }
                MailMsg::Body(uid, r) => {
                    if let Ok(b) = &r {
                        self.cache.put_body(uid, b.clone());
                        self.cache.save();
                    }
                    if self.sel == Some(uid) {
                        self.body = Some(r);
                    }
                }
                MailMsg::Action(Err(e)) => {
                    self.error = Some(format!("Action impossible : {e}"));
                    resync = true;
                }
                MailMsg::Action(Ok(())) => {}
            }
        }
        if resync {
            self.loading = false;
            self.refresh(ctx);
        }
    }

    fn apply(&mut self, ctx: &egui::Context, u: Ui) {
        match u {
            Ui::Open(uid) => self.open(ctx, uid),
            Ui::Back => {
                self.close_message();
            }
            Ui::Folder(f) => {
                self.folder = f;
                self.reset_view();
                self.refresh(ctx);
            }
            Ui::Search => {
                let q = self.search.trim().to_string();
                if q != self.query {
                    self.query = q;
                    self.sel = None;
                    self.body = None;
                    if self.query.is_empty() {
                        self.list = self.cache.headers(self.folder).map(<[Header]>::to_vec).unwrap_or_default();
                    }
                    self.loading = false;
                    self.refresh(ctx);
                }
            }
            Ui::ClearSearch => {
                self.search.clear();
                if !self.query.is_empty() {
                    self.query.clear();
                    self.list = self.cache.headers(self.folder).map(<[Header]>::to_vec).unwrap_or_default();
                    self.loading = false;
                    self.refresh(ctx);
                }
            }
            Ui::Refresh => self.refresh(ctx),
            Ui::Act(uid, a) => self.act(ctx, uid, a),
        }
    }

    pub fn window(&mut self, ctx: &egui::Context) {
        if !self.show {
            return;
        }
        let mut open = true;
        let mut todo: Vec<Ui> = Vec::new();
        let page = ui::page::is_page(ctx);
        let win = egui::Window::new(RichText::new("Mails").font(bold(18.0)))
            .collapsible(false)
            .default_size([900.0_f32.min(ctx.content_rect().width() - 24.0), 560.0_f32.min(ctx.content_rect().height() - 48.0)]);
        // Page mobile : le retour ramène d'abord du message à la liste.
        let reading = page && self.sel.is_some();
        let mut keep = true;
        let shown =
            ui::page::show(ctx, egui::Id::new("mail"), "Mails", win, Some(if reading { &mut keep } else { &mut open }), false, |ui| {
                if !self.ready() {
                    ui.label(
                        RichText::new("Connecte-toi d'abord avec tes identifiants ENT (les mêmes que pour l'emploi du temps).")
                            .color(theme::WARN),
                    );
                    return;
                }
                // Mobile : un message ouvert occupe tout l'écran (ni barre d'outils ni dossiers).
                if !reading {
                    self.toolbar(ui, page, &mut todo);
                    ui.add_space(4.0);
                    self.folder_bar(ui, page, &mut todo);
                    ui.add_space(6.0);
                }
                // Écran étroit (téléphone) : la liste, puis le message seul avec un retour.
                if page || ui.available_width() < 600.0 {
                    if self.sel.is_some() {
                        // (en page mobile, c'est le bouton retour de l'en-tête qui ramène à la liste)
                        if !page && ui.button(format!("← {}", self.folder.label())).clicked() {
                            todo.push(Ui::Back);
                        }
                        ui.add_space(4.0);
                        egui::ScrollArea::vertical().id_salt("mail_body").show(ui, |ui| self.body_ui(ui, page, &mut todo));
                    } else {
                        egui::ScrollArea::vertical().id_salt("mail_list").show(ui, |ui| self.list_ui(ui, &mut todo));
                    }
                } else {
                    ui.columns(2, |cols| {
                        egui::ScrollArea::vertical().id_salt("mail_list").show(&mut cols[0], |ui| self.list_ui(ui, &mut todo));
                        egui::ScrollArea::vertical().id_salt("mail_body").show(&mut cols[1], |ui| self.body_ui(ui, page, &mut todo));
                    });
                }
            });
        if reading && shown.back {
            todo.push(Ui::Back);
        }
        for u in todo {
            self.apply(ctx, u);
        }
        if !open {
            self.show = false;
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, page: bool, todo: &mut Vec<Ui>) {
        let h = if page { 44.0 } else { 0.0 };
        ui.horizontal(|ui| {
            let b = egui::Button::new("Actualiser").min_size(egui::vec2(0.0, h));
            if ui.add_enabled(!self.loading, b).clicked() {
                todo.push(Ui::Refresh);
            }
            let w = if page { (ui.available_width() - 80.0).max(120.0) } else { 220.0 };
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.search)
                    .hint_text(format!("Rechercher dans {}…", self.folder.label().to_lowercase()))
                    .desired_width(w)
                    .min_size(egui::vec2(0.0, h)),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                todo.push(Ui::Search);
            }
            if (!self.search.is_empty() || !self.query.is_empty()) && ui.button("×").on_hover_text("Effacer la recherche").clicked() {
                todo.push(Ui::ClearSearch);
            }
            if self.loading {
                ui.spinner();
            }
            if !page {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.hyperlink_to("Ouvrir le webmail ↗", format!("https://{}/", self.host))
                        .on_hover_text("Pour répondre ou écrire un mail");
                });
            }
        });
        if let Some(e) = &self.error {
            let txt = match (self.list.is_empty(), self.cache.age_min(self.folder)) {
                (false, Some(a)) if self.query.is_empty() => {
                    format!("Hors ligne : messages en cache (mis à jour il y a {}) · {e}", fmt_duration(a))
                }
                _ => e.clone(),
            };
            ui.add(egui::Label::new(RichText::new(txt).color(theme::NOW)).wrap());
        }
    }

    fn folder_bar(&self, ui: &mut egui::Ui, page: bool, todo: &mut Vec<Ui>) {
        ui.horizontal_wrapped(|ui| {
            for f in Folder::ALL {
                let mut label = f.label().to_string();
                if f == Folder::Inbox && self.unread > 0 {
                    label += &format!(" ({})", self.unread);
                }
                let on = self.folder == f;
                let txt = RichText::new(label).font(if on { bold(13.5) } else { regular(13.5) }).color(if on {
                    theme::CARBON
                } else {
                    theme::TEXT
                });
                let mut b = egui::Button::new(txt).min_size(egui::vec2(0.0, if page { 40.0 } else { 26.0 }));
                if on {
                    b = b.fill(theme::ACCENT);
                }
                if ui.add(b).clicked() && !on {
                    todo.push(Ui::Folder(f));
                }
            }
        });
    }

    fn list_ui(&self, ui: &mut egui::Ui, todo: &mut Vec<Ui>) {
        if self.list.is_empty() && !self.loading && self.error.is_none() {
            let txt = if self.query.is_empty() {
                self.folder.empty_label().to_string()
            } else {
                format!("Aucun résultat pour « {} ».", self.query)
            };
            ui.label(RichText::new(txt).color(theme::MUTED));
        }
        if !self.query.is_empty() && !self.list.is_empty() {
            ui.label(
                RichText::new(format!(
                    "{} résultat{} pour « {} »",
                    self.list.len(),
                    if self.list.len() > 1 { "s" } else { "" },
                    self.query
                ))
                .font(regular(12.0))
                .color(theme::DIM),
            );
        }
        for m in &self.list {
            let sel = self.sel == Some(m.uid);
            let title = if m.unread { bold(14.0) } else { regular(14.0) };
            let resp = theme::card_ex(
                ui,
                if sel { theme::CARD_HOVER } else { theme::CARD },
                egui::Stroke::new(1.0, if sel { theme::ACCENT } else { theme::LINE }),
                8,
                Margin::symmetric(10, 6),
                |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if m.unread {
                            let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            ui.painter().circle_filled(r.center(), 4.0, theme::ACCENT);
                        }
                        let who = if m.unread { theme::VELVET_ORANGE } else { theme::VELVET_ORANGE.gamma_multiply(0.75) };
                        ui.add(egui::Label::new(RichText::new(&m.from).font(title.clone()).color(who)).truncate());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(&m.date).font(regular(12.0)).color(theme::DIM));
                        });
                    });
                    let sujet = RichText::new(&m.subject).font(if m.unread { bold(13.5) } else { regular(13.5) });
                    ui.add(egui::Label::new(sujet.color(if m.unread { theme::VELVET_TEXT } else { theme::MUTED })).truncate());
                },
            )
            .response;
            if sel && self.scroll.replace(false) {
                resp.scroll_to_me(None);
            }
            if ui
                .interact(resp.rect, ui.id().with(("mail", m.uid)), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                todo.push(Ui::Open(m.uid));
            }
            ui.add_space(3.0);
        }
    }

    /// Actions possibles sur un message selon le dossier affiché.
    fn actions(&self, m: &Header) -> Vec<(&'static str, MsgAction)> {
        let lu = if m.unread { ("Marquer comme lu", MsgAction::Read) } else { ("Marquer non lu", MsgAction::Unread) };
        match self.folder {
            Folder::Inbox => vec![lu, ("Spam", MsgAction::Spam), ("Corbeille", MsgAction::Trash)],
            Folder::Sent | Folder::Drafts => vec![("Corbeille", MsgAction::Trash)],
            Folder::Junk => vec![("Pas un spam", MsgAction::NotSpam), ("Corbeille", MsgAction::Trash)],
            Folder::Trash => vec![("Remettre en réception", MsgAction::Move(Folder::Inbox))],
        }
    }

    fn body_ui(&self, ui: &mut egui::Ui, page: bool, todo: &mut Vec<Ui>) {
        // `ui.columns` justifie le texte renvoyé à la ligne (lettres espacées) : mise en page simple.
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(ui.available_width());
            self.body_inner(ui, page, todo);
        });
    }

    fn body_inner(&self, ui: &mut egui::Ui, page: bool, todo: &mut Vec<Ui>) {
        let Some(uid) = self.sel else {
            ui.label(RichText::new("Sélectionne un message.").color(theme::MUTED));
            return;
        };
        if let Some(m) = self.list.iter().find(|m| m.uid == uid) {
            ui.add(egui::Label::new(RichText::new(&m.subject).font(bold(19.0)).color(theme::VELVET_ORCHID)).wrap());
            ui.add_space(6.0);
            envelope(ui, m, &self.rcpt_open);
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                for (label, a) in self.actions(m) {
                    let danger = matches!(a, MsgAction::Trash | MsgAction::Spam);
                    let txt = RichText::new(label).font(regular(13.0)).color(if danger { theme::NOW } else { theme::TEXT });
                    let b = egui::Button::new(txt).min_size(egui::vec2(0.0, if page { 40.0 } else { 24.0 }));
                    let hint = if a == MsgAction::Trash { "Suppr" } else { "" };
                    let r = ui.add(b);
                    let r = if hint.is_empty() || page { r } else { r.on_hover_text(hint) };
                    if r.clicked() {
                        todo.push(Ui::Act(uid, a));
                    }
                }
            });
            ui.separator();
        }
        match &self.body {
            None => {
                ui.spinner();
            }
            Some(Ok(b)) => mail_rich_ui(ui, b),
            Some(Err(e)) => {
                ui.label(RichText::new(e).color(theme::NOW));
            }
        }
    }
}

/// Corps d'un mail : gras, italique, liens cliquables en couleur, citations et puces.
/// Destinataires affichés avant « +N autres ».
const SHOWN_RCPT: usize = 3;

/// Enveloppe d'un message : De / À / Cc / date, chaque rôle dans sa couleur.
/// `expanded` : listes de destinataires dépliées en entier.
fn envelope(ui: &mut egui::Ui, m: &Header, expanded: &Cell<bool>) {
    let label_col = |ui: &mut egui::Ui, label: &str| {
        ui.add_sized([30.0, 18.0], egui::Label::new(RichText::new(label).font(theme::mono_bold(11.0)).color(theme::DIM)));
    };
    let row = |ui: &mut egui::Ui, label: &str, people: &[mail::Contact], color: Color32, with_addr: bool| {
        if people.is_empty() {
            return;
        }
        ui.horizontal_top(|ui| {
            label_col(ui, label);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 2.0);
                let n = if expanded.get() { people.len() } else { people.len().min(SHOWN_RCPT) };
                for (i, c) in people[..n].iter().enumerate() {
                    if i > 0 {
                        ui.label(RichText::new(",  ").font(regular(13.5)).color(theme::DIM));
                    }
                    let r = ui.label(RichText::new(c.label()).font(bold(13.5)).color(color));
                    if with_addr && !c.name.is_empty() {
                        ui.label(RichText::new(format!("  {}", c.addr)).font(regular(12.0)).color(theme::DIM));
                    } else if !c.name.is_empty() {
                        r.on_hover_text(&c.addr);
                    }
                }
                if people.len() > n {
                    let more = RichText::new(format!("  +{} autre{}", people.len() - n, if people.len() - n > 1 { "s" } else { "" }))
                        .font(regular(12.5))
                        .color(theme::ACCENT);
                    if ui.add(egui::Label::new(more).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                    {
                        expanded.set(true);
                    }
                } else if expanded.get() && people.len() > SHOWN_RCPT {
                    let less = RichText::new("  réduire").font(regular(12.5)).color(theme::ACCENT);
                    if ui.add(egui::Label::new(less).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                    {
                        expanded.set(false);
                    }
                }
            });
        });
    };
    match &m.sender {
        Some(s) => row(ui, "DE", std::slice::from_ref(s), theme::VELVET_ORANGE, true),
        // en-tête d'un ancien cache : seul le résumé est connu
        None => {
            ui.label(RichText::new(&m.from).font(bold(13.5)).color(theme::VELVET_ORANGE));
        }
    }
    row(ui, "À", &m.to, theme::VELVET_GREEN, false);
    row(ui, "CC", &m.cc, theme::VELVET_PURPLE, false);
    let quand = chrono::DateTime::from_timestamp_millis(m.ts)
        .filter(|_| m.ts > 0)
        .map(|d| {
            let d = d.with_timezone(&chrono::Local);
            format!("{} {} à {}", capitalize(&date_longue(d.date_naive())), d.year(), d.format("%H:%M"))
        })
        .unwrap_or_else(|| m.date.clone());
    ui.horizontal(|ui| {
        label_col(ui, "LE");
        ui.label(RichText::new(quand).font(regular(12.5)).color(theme::MUTED));
    });
}

pub(super) fn mail_rich_ui(ui: &mut egui::Ui, body: &mail::Body) {
    ui.visuals_mut().hyperlink_color = theme::ACCENT;
    const SIZE: f32 = 14.0;
    for line in &body.lines {
        if line.spans.is_empty() {
            ui.add_space(9.0);
            continue;
        }
        let quoted = line.quote > 0;
        let draw = |ui: &mut egui::Ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(0.0, 3.0);
                if line.bullet {
                    ui.label(RichText::new("•  ").font(bold(SIZE)).color(theme::VELVET_PINK));
                }
                for sp in &line.spans {
                    let mut t = RichText::new(&sp.text).font(if sp.bold { bold(SIZE) } else { regular(SIZE) });
                    if sp.italic {
                        t = t.italics();
                    }
                    // couleurs de Royal Velvet : gras rose, italique bleu, les deux rouge
                    let color = match (sp.bold, sp.italic) {
                        (true, true) => theme::VELVET_RED,
                        (true, false) => theme::VELVET_PINK,
                        (false, true) => theme::VELVET_BLUE,
                        _ if quoted => theme::MUTED,
                        _ => theme::VELVET_TEXT,
                    };
                    match &sp.link {
                        Some(url) => {
                            ui.add(egui::Hyperlink::from_label_and_url(t.color(theme::ACCENT).underline(), url)).on_hover_text(url);
                        }
                        None => {
                            ui.add(egui::Label::new(t.color(if quoted { color.gamma_multiply(0.8) } else { color })));
                        }
                    }
                }
            });
        };
        if quoted {
            let left = 12 + 10 * (line.quote as i8 - 1).min(3);
            let r = egui::Frame::new().inner_margin(Margin { left, right: 0, top: 1, bottom: 1 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                draw(ui);
            });
            let rc = r.response.rect;
            for q in 0..line.quote.min(4) {
                let x = rc.left() + 1.0 + 10.0 * q as f32;
                ui.painter().line_segment(
                    [egui::pos2(x, rc.top()), egui::pos2(x, rc.bottom())],
                    egui::Stroke::new(2.0, theme::VELVET_RED.gamma_multiply(0.6)),
                );
            }
        } else {
            draw(ui);
        }
    }
}
