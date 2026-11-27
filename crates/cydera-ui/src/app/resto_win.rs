//! Fenêtre « Resto » : menus du Crous et fréquentation en direct.
use super::*;

/// Ouvre la jauge officielle du Crous dans une petite fenêtre « application » du navigateur
/// (secours quand la lecture directe de la jauge échoue).
pub(super) fn open_gauge(ctx: &egui::Context, url: &str) {
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
pub(super) fn milliers(n: u32) -> String {
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
pub(super) fn live_gauge(ui: &mut egui::Ui, ctx: &egui::Context, r: &resto::Resto, live: Option<&Result<Live, String>>, page: bool) {
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

impl App {
    pub(super) fn resto_refresh(&mut self, ctx: &egui::Context) {
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

    pub(super) fn live_refresh(&mut self, ctx: &egui::Context) {
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

    pub(super) fn toggle_resto(&mut self, ctx: &egui::Context) {
        self.show_resto = !self.show_resto;
        if self.show_resto && self.resto_last.is_none_or(|t| t.elapsed().as_secs() > 1800) {
            self.resto_refresh(ctx);
        }
    }

    pub(super) fn resto_window(&mut self, ctx: &egui::Context) {
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
}
