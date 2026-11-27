//! Fenêtre « Appareils » : liste des appareils appairés, QR / ticket pour en associer un
//! (côté PC), saisie du ticket (côté téléphone), et confirmation d'une demande d'appairage.
use super::theme::{self, bold, regular};
use crate::p2p::Status;
use cydera_sync::{PairRequest, PairTicket, Peer};
use eframe::egui::{self, Color32, Margin, RichText};
use std::time::Instant;

#[derive(Default)]
pub struct DevicesUi {
    /// Ticket affiché (PC) et son échéance.
    pub ticket: Option<(PairTicket, Instant)>,
    /// Ticket collé (téléphone).
    pub input: String,
    /// Dernier message (texte, succès ?).
    pub msg: Option<(String, bool)>,
    /// Code à comparer, affiché sur le téléphone pendant que le PC confirme.
    pub phone_code: Option<String>,
    /// Case « envoyer mes identifiants Celcat » de la fenêtre de confirmation.
    pub send_creds: bool,
}

pub enum DevAction {
    StartPairing,
    CancelPairing,
    Pair(String),
    Forget(String),
    SyncNow,
}

/// Texte et couleur de l'indicateur de synchro (barre d'état et fenêtre).
pub fn status_line(s: &Status) -> Option<(String, Color32)> {
    Some(match s {
        Status::Starting | Status::NoPeers => return None,
        Status::Unavailable(e) => (format!("P2P indisponible · {e}"), theme::WARN),
        Status::Syncing => ("◌ Synchro appareils…".to_string(), theme::ACCENT),
        Status::Synced { name, age } => {
            let when = if age.as_secs() < 90 { "à l'instant".to_string() } else { format!("il y a {} min", age.as_secs() / 60) };
            (format!("● Synchronisé • {name} · {when}"), theme::OK)
        }
        Status::Offline { name, .. } => (format!("▲ Hors-ligne • {name}"), theme::WARN),
    })
}

fn ago(ms: i64) -> String {
    let s = ((chrono::Utc::now().timestamp_millis() - ms) / 1000).max(0);
    match s {
        0..=89 => "à l'instant".into(),
        90..=5399 => format!("il y a {} min", s / 60),
        5400..=129_599 => format!("il y a {} h", s / 3600),
        _ => format!("il y a {} j", s / 86_400),
    }
}

/// Dessine un QR code (modules noirs sur fond blanc, avec marge blanche réglementaire).
pub fn draw_qr(ui: &mut egui::Ui, text: &str, size: f32) {
    use qrcode::{Color, EcLevel, QrCode};
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, egui::CornerRadius::same(6), Color32::WHITE);
    let Ok(code) = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::L) else {
        painter.text(rect.center(), egui::Align2::CENTER_CENTER, "QR trop grand", regular(13.0), Color32::BLACK);
        return;
    };
    let n = code.width();
    let quiet = 2.0;
    let cell = size / (n as f32 + 2.0 * quiet);
    let colors = code.to_colors();
    for (i, c) in colors.iter().enumerate() {
        if *c == Color::Dark {
            let (x, y) = ((i % n) as f32, (i / n) as f32);
            let min = rect.min + egui::vec2((x + quiet) * cell, (y + quiet) * cell);
            // +0.5 px : évite les fines lignes blanches entre modules après mise à l'échelle.
            painter.rect_filled(egui::Rect::from_min_size(min, egui::vec2(cell + 0.5, cell + 0.5)), 0.0, Color32::BLACK);
        }
    }
}

/// Fenêtre principale. Renvoie les actions demandées par l'utilisateur.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    st: &mut DevicesUi,
    peers: &[Peer],
    status: &Status,
    busy: bool,
    mobile: bool,
) -> Vec<DevAction> {
    let mut acts = Vec::new();
    let win = egui::Window::new(RichText::new("Appareils").font(bold(18.0)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(440.0_f32.min(ctx.content_rect().width() - 24.0));
    super::page::show(ctx, egui::Id::new("devices"), "Appareils", win, Some(open), false, |ui| {
        // Hauteur de la zone défilante : tout l'écran utile (le clavier en réduit déjà `content_rect`).
        let max_h = if super::page::is_page(ctx) { ui.available_height() } else { (ctx.content_rect().height() - 120.0).max(240.0) };
        egui::ScrollArea::vertical().auto_shrink([false, true]).max_height(max_h).show(ui, |ui| {
            let pairing = st.ticket.is_some();
            // Pendant l'affichage du QR : écran dédié, rien d'autre.
            if !pairing {
                ui.label(RichText::new("ASSOCIÉS").font(bold(11.5)).color(theme::DIM));
                if peers.is_empty() {
                    ui.label(RichText::new("Aucun appareil associé.").color(theme::MUTED));
                }
                for p in peers {
                    theme::card(ui, Margin::symmetric(12, 10), |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                ui.label(RichText::new(&p.name).font(bold(15.0)));
                                let last = p.last_sync_ms.map(ago).unwrap_or_else(|| "jamais".into());
                                ui.label(RichText::new(format!("dernière synchro : {last}")).font(regular(12.0)).color(theme::DIM));
                            });
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Oublier").clicked() {
                                    acts.push(DevAction::Forget(p.id.clone()));
                                }
                            });
                        });
                    });
                }
                if let Some((txt, col)) = status_line(status) {
                    ui.label(RichText::new(txt).font(theme::mono(11.5)).color(col));
                }
                if !peers.is_empty() && ui.button("Synchroniser maintenant").clicked() {
                    acts.push(DevAction::SyncNow);
                }
            }
            if !pairing {
                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new("ASSOCIER UN APPAREIL").font(bold(11.5)).color(theme::DIM));
            } else {
                ui.label(RichText::new("SCANNE CE QR CODE AVEC L'AUTRE APPAREIL").font(bold(11.5)).color(theme::DIM));
            }

            // ---- Côté PC : afficher le ticket ----
            match &st.ticket {
                None => {
                    ui.label(RichText::new("Affiche un QR code à scanner depuis l'autre appareil.").color(theme::MUTED));
                    let w = ui.available_width();
                    if ui
                        .add(egui::Button::new(RichText::new("Associer un appareil").font(bold(14.0))).min_size(egui::vec2(w, 40.0)))
                        .clicked()
                    {
                        acts.push(DevAction::StartPairing);
                    }
                }
                Some((t, deadline)) => {
                    let left = deadline.saturating_duration_since(Instant::now()).as_secs();
                    let text = t.to_text();
                    let size = if mobile { 210.0 } else { 280.0 };
                    ui.vertical_centered(|ui| draw_qr(ui, &text, size));
                    ui.label(
                        RichText::new(format!("Valable encore {}:{:02}", left / 60, left % 60)).font(theme::mono(11.5)).color(theme::DIM),
                    );
                    // Sur téléphone, le QR (et Annuler) suffisent ; la copie de texte sert surtout depuis un PC.
                    if !mobile {
                        ui.label(RichText::new("Ou copie ce ticket sur l'autre appareil :").color(theme::MUTED));
                        let mut shown = text.as_str();
                        ui.add(egui::TextEdit::multiline(&mut shown).desired_rows(3).desired_width(f32::INFINITY).font(theme::mono(10.5)));
                    }
                    ui.horizontal(|ui| {
                        if !mobile && ui.button("Copier le ticket").clicked() {
                            ctx.copy_text(text.clone());
                        }
                        if ui.button("Annuler").clicked() {
                            acts.push(DevAction::CancelPairing);
                        }
                    });
                }
            }

            if !pairing {
                // ---- Côté téléphone : coller le ticket ----
                ui.add_space(10.0);
                ui.separator();
                ui.label(RichText::new("J'AI UN TICKET").font(bold(11.5)).color(theme::DIM));
                // Hauteur bornée (défilement interne) : les boutons restent visibles avec le clavier.
                egui::ScrollArea::vertical().id_salt("ticket-input").max_height(76.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut st.input)
                            .hint_text("Colle ici le ticket affiché sur l'autre appareil")
                            .desired_rows(2)
                            .desired_width(f32::INFINITY)
                            .font(theme::mono(10.5)),
                    );
                });
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy && !st.input.trim().is_empty(), egui::Button::new("Associer")).clicked() {
                        acts.push(DevAction::Pair(st.input.trim().to_string()));
                    }
                    // Pas de presse-papiers côté Android pour l'instant (le QR code est le chemin normal).
                    #[cfg(not(target_os = "android"))]
                    if ui.button("Coller").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                    }
                    if busy {
                        ui.spinner();
                    }
                });
                if busy && let Some(code) = &st.phone_code {
                    ui.label(
                        RichText::new("Vérifie que ce code s'affiche aussi sur l'autre appareil, puis confirme-y :").color(theme::MUTED),
                    );
                    ui.label(RichText::new(code).font(bold(30.0)).color(theme::ACCENT));
                }
            }
            if let Some((m, ok)) = &st.msg {
                ui.label(RichText::new(m).color(if *ok { theme::OK } else { theme::NOW }));
            }
        });
    });
    acts
}

/// Fenêtre de confirmation côté PC. `Some(true)` accepter, `Some(false)` refuser.
pub fn request_dialog(ctx: &egui::Context, req: &PairRequest, send_creds: &mut bool, can_send_creds: bool) -> Option<bool> {
    let mut out = None;
    let win = egui::Window::new(RichText::new("Nouvel appareil").font(bold(18.0)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(380.0_f32.min(ctx.content_rect().width() - 24.0));
    super::page::show(ctx, egui::Id::new("pair-request"), "Nouvel appareil", win, None, true, |ui| {
        ui.label(RichText::new(format!("« {} » demande à s'associer.", req.name)).font(regular(15.0)));
        ui.add_space(6.0);
        ui.label(RichText::new("Le même code doit s'afficher sur cet appareil :").color(theme::MUTED));
        ui.vertical_centered(|ui| ui.label(RichText::new(&req.code).font(bold(34.0)).color(theme::ACCENT)));
        ui.add_space(6.0);
        if can_send_creds {
            ui.checkbox(send_creds, "Lui envoyer aussi mes identifiants Celcat");
        }
        ui.label(RichText::new("Refuse si tu ne reconnais pas cet appareil.").font(regular(12.0)).color(theme::DIM));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.add(egui::Button::new(RichText::new("Accepter").font(bold(14.0))).min_size(egui::vec2(120.0, 40.0))).clicked() {
                out = Some(true);
            }
            if ui.add(egui::Button::new("Refuser").min_size(egui::vec2(120.0, 40.0))).clicked() {
                out = Some(false);
            }
        });
    });
    out
}
