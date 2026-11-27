//! Écran « À propos » et actions de mise à jour.
use super::*;

impl App {
    /// Photo GitHub (0) et logo Claude (1), intégrés à l'exécutable.
    pub(super) fn credit_tex(&mut self, ctx: &egui::Context, idx: usize) -> egui::TextureHandle {
        if self.credit_tex[idx].is_none() {
            let bytes: &[u8] =
                if idx == 0 { include_bytes!("../../assets/avatar.rgba") } else { include_bytes!("../../assets/claude.rgba") };
            let img = egui::ColorImage::from_rgba_unmultiplied([128, 128], bytes);
            self.credit_tex[idx] =
                Some(ctx.load_texture(if idx == 0 { "credit-github" } else { "credit-claude" }, img, egui::TextureOptions::LINEAR));
        }
        self.credit_tex[idx].clone().unwrap()
    }

    pub(super) fn about_window(&mut self, ctx: &egui::Context) {
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
            ui.add_space(12.0);
            ui.label(RichText::new("STACK TECHNIQUE").font(bold(11.5)).color(theme::DIM));
            theme::card(ui, Margin::symmetric(14, 12), |ui| {
                ui.set_width(ui.available_width());
                let lignes = [
                    ("Langage", "Rust (édition 2024), workspace multi-crates : core, sync, ui, desktop, android"),
                    ("Interface", "egui / eframe, rendu GPU, même code d'interface sur PC et mobile"),
                    ("Android", "NativeActivity + JNI (jni, android-activity), APK téléchargé via DownloadManager"),
                    ("Réseau", "tokio (async), reqwest + rustls (HTTPS sans OpenSSL), cookies de session"),
                    ("Emploi du temps", "Celcat (CY Cergy Paris Université), interrogé et mis en cache localement"),
                    ("Mails", "Webmail Zimbra (SSO Shibboleth/CAS, API SOAP JSON), décodage MIME avec mailparse, cache hors ligne"),
                    ("Synchro entre appareils", "iroh : pair à pair chiffré, découverte locale mDNS, appairage par QR code (qrcode)"),
                    ("Données", "serde / serde_json, stockage local dans le dossier de config de l'utilisateur (dirs)"),
                    ("Relevés ECTS", "Lecture des PDF (pdf-extract, optionnel)"),
                    ("Mises à jour", "GitHub Releases, version comparée avec semver"),
                    ("Build & CI", "cargo, GitHub Actions, APK signé"),
                ];
                for (i, (k, v)) in lignes.into_iter().enumerate() {
                    if i > 0 {
                        ui.add_space(6.0);
                    }
                    ui.label(RichText::new(k).font(bold(12.5)));
                    ui.label(RichText::new(v).font(regular(12.5)).color(theme::MUTED));
                }
            });
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

    pub(super) fn maj_action(&mut self, ctx: &egui::Context, a: ui::update::Action) {
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
}
