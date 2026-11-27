//! Habillage de l'écran principal : en-tête, navigation, onglets mobiles, barre d'état,
//! feuille « Plus » et fiche d'un cours.
use super::*;

impl App {
    pub(super) fn apply(&mut self, action: Option<Action>) {
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

    pub(super) fn title_period(&self) -> String {
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

    pub(super) fn title_block(&self, ui: &mut egui::Ui, size: f32) {
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

    pub(super) fn rev_label(&self) -> (String, egui::Color32) {
        match (&self.chrono.subject, self.chrono.running()) {
            (Some(s), true) => (format!("⏱ {} · {}", fmt_hms(self.chrono.run_secs()), s.to_uppercase()), theme::OK),
            _ => ("⏱ RÉVISIONS".to_string(), theme::TEXT),
        }
    }

    /// Une action secondaire de l'en-tête : bouton direct (écran large) ou entrée du menu.
    pub(super) fn action(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, which: Act, menu: bool) {
        let btn = |ui: &mut egui::Ui, text: String, col: egui::Color32, sel: bool, tip: &str, enabled: bool| -> bool {
            let mut b = egui::Button::new(RichText::new(text.to_uppercase()).color(col)).selected(sel);
            if menu {
                // Cibles généreuses (HIG accessibility.md › Controls) + raccourci clavier affiché à droite.
                let key = match which {
                    Act::Resto => "U",
                    Act::Exams => "E",
                    Act::Mails => "M",
                    Act::Rev => "C",
                    Act::Stats => "G",
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
                let unread = self.mail.unread();
                let label = if unread > 0 { format!("Mails ({unread})") } else { "Mails".to_string() };
                let c = btn(ui, label, theme::TEXT, self.mail.show, "Boîte de réception (M)", true);
                if c {
                    self.mail.toggle(ctx);
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
            Act::Stats => {
                let c = btn(ui, "Stats".to_string(), theme::TEXT, self.show_stats, "Statistique de révision (G)", true);
                if c {
                    self.show_stats = !self.show_stats;
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
    pub(super) fn view_toggle(&mut self, ui: &mut egui::Ui, rtl: bool) {
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

    pub(super) fn nav(&mut self, ui: &mut egui::Ui, rtl: bool) {
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

    pub(super) fn burger(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let unread = self.mail.unread();
        let label = if unread > 0 { format!("Menu ({unread})") } else { "Menu".to_string() };
        ui.menu_button(label, |ui| {
            ui.set_min_width(240.0);
            for a in [Act::Resto, Act::Exams, Act::Grades, Act::Mails, Act::Rev, Act::Stats, Act::Devices, Act::About, Act::Refresh] {
                self.action(ui, ctx, a, true);
            }
        });
    }

    pub(super) fn header(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, bp: Bp) {
        let rtl = egui::Layout::right_to_left(egui::Align::Center);
        match bp {
            Bp::Wide => {
                ui.horizontal(|ui| {
                    self.title_block(ui, 28.0);
                    ui.with_layout(rtl, |ui| {
                        for a in
                            [Act::Refresh, Act::About, Act::Devices, Act::Stats, Act::Rev, Act::Mails, Act::Resto, Act::Grades, Act::Exams]
                        {
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
                        // Rien à signaler quand tout va bien : « à jour » prêtait à confusion avec la version de l'appli.
                        let calme = c == theme::MUTED && t.starts_with("Synchro ");
                        if !calme {
                            ui.label(RichText::new(t).font(regular(12.5)).color(c));
                            // pastille d'état dessinée (les glyphes ▲ ◌ manquent dans la police proportionnelle)
                            let (r, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
                            if self.syncing {
                                ui.painter().circle_stroke(
                                    r.center(),
                                    3.5,
                                    egui::Stroke::new(1.5, c.gamma_multiply(theme::blink(ui.ctx()))),
                                );
                            } else {
                                ui.painter().circle_filled(r.center(), 4.0, c);
                            }
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
    pub(super) fn sync_short(&self) -> (String, egui::Color32) {
        if self.password.is_none() {
            ("Connexion requise".into(), theme::NOW)
        } else if self.error.is_some() {
            ("Hors ligne".into(), theme::WARN)
        } else if self.syncing {
            ("Synchro…".into(), theme::ACCENT)
        } else if let Some(a) = self.store.last_sync_min() {
            let when = if a < 1 { "à l'instant".to_string() } else { format!("il y a {}", fmt_duration(a)) };
            (format!("Synchro {when}"), theme::MUTED)
        } else {
            ("En attente".into(), theme::MUTED)
        }
    }

    /// État de la synchro, version longue (barre d'état sur PC, feuille « Plus » sur mobile).
    pub(super) fn sync_long(&self) -> (String, egui::Color32) {
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

    pub(super) fn active_tab(&self) -> Option<ui::mobile::Tab> {
        use ui::mobile::Tab;
        if self.show_more {
            Some(Tab::More)
        } else if self.mail.show {
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
    pub(super) fn top_page(&self) -> Option<&'static str> {
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
        } else if self.mail.show {
            Some("mail")
        } else if self.show_resto {
            Some("resto")
        } else if self.show_stats {
            Some("stats")
        } else if self.show_rev {
            Some("revisions")
        } else if self.show_exams {
            Some("exams")
        } else {
            None
        }
    }

    /// Ferme la page du dessus (retour Android). Renvoie faux s'il n'y en avait aucune.
    pub(super) fn close_top_page(&mut self) -> bool {
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
            Some("mail") if self.mail.reading() => {
                self.mail.close_message();
            }
            Some("mail") => self.mail.show = false,
            Some("resto") => self.show_resto = false,
            Some("stats") => self.show_stats = false,
            Some("revisions") => self.show_rev = false,
            Some("exams") => self.show_exams = false,
            _ => return false,
        }
        true
    }

    pub(super) fn on_tab(&mut self, ctx: &egui::Context, t: ui::mobile::Tab) {
        use ui::mobile::Tab;
        let was = self.active_tab();
        // un seul onglet ouvert à la fois ; toucher l'onglet actif le referme (retour à l'emploi du temps)
        self.show_more = false;
        self.show_exams = false;
        self.show_rev = false;
        self.show_resto = false;
        self.mail.show = false;
        // les pages ouvertes depuis « Plus » ou une carte de cours ne restent pas devant le nouvel onglet
        self.show_about = false;
        self.show_devices = false;
        self.show_grades = false;
        self.show_stats = false;
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
            Tab::Mails => self.mail.toggle(ctx),
            Tab::More => self.show_more = true,
        }
    }

    /// Feuille « Plus » : actions secondaires en grandes lignes, posée au-dessus de la barre d'onglets.
    pub(super) fn more_sheet(&mut self, ctx: &egui::Context, bottom: f32) {
        let vp = ctx.viewport_rect();
        let area = egui::Rect::from_min_max(vp.min, egui::pos2(vp.right(), bottom));
        let id = egui::Id::new("more_sheet");
        let peers = self.p2p.peers().len();
        let dev_detail = if peers == 0 { String::new() } else { format!("{peers} associé{}", if peers > 1 { "s" } else { "" }) };
        let (sync_t, sync_c) = self.sync_long();
        let p2p = devices::status_line(&self.p2p.status());
        let (short, short_c) = self.sync_short();
        let can_sync = !self.syncing && self.password.is_some();
        let rows_h = 5.0 * 52.0;
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
            let stats_detail = format!("{} cette semaine", crate::study::fmt_short(self.chrono.week(None)));
            if ui::mobile::sheet_row(ui, "Statistique", &stats_detail, theme::MUTED, true) {
                pick = Some(6);
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
                6 => self.show_stats = true,
                _ => self.show_about = true,
            }
        }
        if close {
            self.show_more = false;
        }
    }

    pub(super) fn status_bar(&self, ui: &mut egui::Ui, hints: bool) {
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

    pub(super) fn detail_window(&mut self, ctx: &egui::Context) {
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
