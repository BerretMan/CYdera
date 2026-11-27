//! Vues des mises à jour : bandeau « nouvelle version », page « Mise à jour » et encart de
//! l'écran « À propos ». L'état et les actions sont dans `crate::update`.
use crate::ui::page;
use crate::ui::theme::{self, bold, regular};
use crate::update::{Install, Maj};
use cydera_core::update::Release;
use eframe::egui::{self, Margin, RichText};
use std::path::PathBuf;

/// Ce que l'utilisateur a demandé.
pub enum Action {
    /// Ouvre la page « Mise à jour ».
    Ouvrir,
    Installer,
    Relancer(PathBuf),
    OuvrirPage,
    Verifier,
    PlusTard,
    Ignorer,
}

fn bouton(ui: &mut egui::Ui, texte: &str, principal: bool, grand: bool) -> bool {
    let h = if grand { 44.0 } else { 30.0 };
    let txt = RichText::new(texte).font(bold(13.5)).color(if principal { theme::CARBON } else { theme::TEXT });
    let mut b = egui::Button::new(txt).min_size(egui::vec2(0.0, h));
    if principal {
        b = b.fill(theme::ACCENT);
    }
    ui.add(b).clicked()
}

fn taille(o: u64) -> String {
    format!("{:.1} Mo", o as f64 / 1_048_576.0)
}

/// Bandeau non bloquant affiché quand une nouvelle version est publiée.
pub fn bandeau(ui: &mut egui::Ui, rel: &Release, mobile: bool) -> Option<Action> {
    let mut action = None;
    theme::card(ui, Margin::symmetric(12, 8), |ui| {
        ui.set_width(ui.available_width());
        let titre = RichText::new(format!("Cydera {} est disponible", rel.version)).font(bold(14.0)).color(theme::ACCENT);
        let boutons = |ui: &mut egui::Ui, action: &mut Option<Action>| {
            if bouton(ui, "Installer", true, mobile) {
                *action = Some(Action::Ouvrir);
            }
            if bouton(ui, "Plus tard", false, mobile) {
                *action = Some(Action::PlusTard);
            }
            if bouton(ui, "Ignorer cette version", false, mobile) {
                *action = Some(Action::Ignorer);
            }
        };
        if mobile {
            ui.label(titre);
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| boutons(ui, &mut action));
        } else {
            ui.horizontal(|ui| {
                ui.label(titre);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // ordre inversé : la mise en page part de la droite
                    let mut a = None;
                    if bouton(ui, "Ignorer cette version", false, false) {
                        a = Some(Action::Ignorer);
                    }
                    if bouton(ui, "Plus tard", false, false) {
                        a = Some(Action::PlusTard);
                    }
                    if bouton(ui, "Installer", true, false) {
                        a = Some(Action::Ouvrir);
                    }
                    action = a;
                });
            });
        }
    });
    action
}

/// Avancement / résultat de l'installation et bouton d'action correspondant.
fn etat_installation(ui: &mut egui::Ui, maj: &Maj, grand: bool) -> Option<Action> {
    match &maj.install {
        Install::Rien => {
            let natif = maj.dispo.as_ref().and_then(|r| r.paquet()).is_some();
            if natif {
                if bouton(ui, "Installer la mise à jour", true, grand) {
                    return Some(Action::Installer);
                }
            } else if bouton(ui, "Ouvrir la page de téléchargement", true, grand) {
                return Some(Action::OuvrirPage);
            }
        }
        Install::EnCours(recus, total) => {
            let (frac, txt) = match total {
                Some(t) if *t > 0 => (*recus as f32 / *t as f32, format!("Téléchargement… {} / {}", taille(*recus), taille(*t))),
                _ => (0.0, format!("Téléchargement… {}", taille(*recus))),
            };
            ui.add(egui::ProgressBar::new(frac.clamp(0.0, 1.0)).desired_height(if grand { 10.0 } else { 6.0 }));
            ui.label(RichText::new(txt).font(regular(12.5)).color(theme::MUTED));
        }
        Install::Prete(exe) => {
            ui.label(RichText::new("Mise à jour installée. Redémarre Cydera pour l'utiliser.").color(theme::OK));
            if bouton(ui, "Redémarrer", true, grand) {
                return Some(Action::Relancer(exe.clone()));
            }
        }
        Install::Lancee => {
            ui.label(
                RichText::new("L'installeur Android est ouvert : valide l'installation. À la première mise à jour, autorise Cydera à installer des applis.")
                    .color(theme::MUTED),
            );
            if bouton(ui, "Relancer l'installation", false, grand) {
                return Some(Action::Installer);
            }
        }
        Install::Echec(e) => {
            ui.label(RichText::new(e).color(theme::NOW));
            ui.horizontal_wrapped(|ui| {
                if bouton(ui, "Réessayer", true, grand) {
                    Some(Action::Installer)
                } else if bouton(ui, "Ouvrir la page de téléchargement", false, grand) {
                    Some(Action::OuvrirPage)
                } else {
                    None
                }
            })
            .inner?;
        }
    }
    None
}

/// Page (mobile) ou fenêtre (PC) « Mise à jour » : notes de version et installation.
pub fn page(ctx: &egui::Context, maj: &mut Maj) -> Option<Action> {
    if !maj.show {
        return None;
    }
    let rel = maj.dispo.clone()?;
    let mut open = true;
    let mut action = None;
    let win = egui::Window::new(RichText::new("Mise à jour").font(bold(18.0)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(440.0_f32.min(ctx.content_rect().width() - 24.0));
    page::show(ctx, egui::Id::new("update"), "Mise à jour", win, Some(&mut open), true, |ui| {
        let grand = page::is_page(ctx);
        ui.label(RichText::new(format!("Cydera {}", rel.version)).font(bold(24.0)).color(theme::ACCENT));
        let mut infos = format!("Version installée : {}", env!("CARGO_PKG_VERSION"));
        if let Some(a) = rel.paquet() {
            infos.push_str(&format!(" · téléchargement : {}", taille(a.taille)));
        }
        ui.label(RichText::new(infos).font(regular(12.5)).color(theme::DIM));
        ui.add_space(10.0);
        if !rel.notes.trim().is_empty() {
            ui.label(RichText::new("NOUVEAUTÉS").font(bold(11.5)).color(theme::DIM));
            theme::card(ui, Margin::symmetric(12, 10), |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical().max_height(if grand { f32::INFINITY } else { 220.0 }).show(ui, |ui| {
                    ui.label(RichText::new(rel.notes.trim()).font(regular(13.0)).color(theme::MUTED));
                });
            });
            ui.add_space(10.0);
        }
        action = etat_installation(ui, maj, grand);
        ui.add_space(6.0);
        ui.hyperlink_to(RichText::new("Voir la release sur GitHub").font(regular(12.5)), &rel.page);
    });
    if !open {
        maj.show = false;
    }
    action
}

/// Encart « Mises à jour » de l'écran « À propos ».
pub fn apropos(ui: &mut egui::Ui, maj: &Maj, auto: &mut bool) -> Option<Action> {
    let grand = page::is_page(ui.ctx());
    let mut action = None;
    ui.label(RichText::new("MISES À JOUR").font(bold(11.5)).color(theme::DIM));
    theme::card(ui, Margin::symmetric(14, 12), |ui| {
        ui.set_width(ui.available_width());
        if maj.verif_en_cours {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new("Vérification…").color(theme::MUTED));
            });
        } else if let Some(r) = &maj.dispo {
            ui.label(RichText::new(format!("Version {} disponible", r.version)).font(bold(14.0)).color(theme::ACCENT));
            ui.add_space(4.0);
            if matches!(maj.install, Install::Rien) {
                if bouton(ui, "Installer la mise à jour", true, grand) {
                    action = Some(Action::Ouvrir);
                }
            } else {
                action = etat_installation(ui, maj, grand);
            }
        } else if let Some(m) = &maj.message {
            ui.label(RichText::new(m).color(if m.starts_with("Cydera est à jour") { theme::OK } else { theme::NOW }));
        } else {
            ui.label(RichText::new("Aucune nouvelle version connue.").color(theme::MUTED));
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled_ui(!maj.verif_en_cours, |ui| bouton(ui, "Vérifier maintenant", false, grand)).inner {
                action = Some(Action::Verifier);
            }
        });
        ui.checkbox(auto, "Vérifier automatiquement les mises à jour");
    });
    action
}
