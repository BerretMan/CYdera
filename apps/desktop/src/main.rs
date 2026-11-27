use chrono::Duration;
use cydera_core::{cache, celcat, config, mail, model};
use cydera_ui::{app, ui};

fn main() -> eframe::Result {
    cydera_core::net::crypto_init();
    config::migrate_legacy();
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime tokio");

    // `cydera --dump` : vérifie le scraping sans interface.
    if std::env::args().any(|a| a == "--dump") {
        let cfg = config::Config::load();
        let Some(pwd) = config::load_password() else {
            eprintln!("Mot de passe introuvable (CELCAT_PASSWORD ou ~/CELCAT_PASSWORD)");
            std::process::exit(1);
        };
        let today = model::now().date();
        let days: i64 = std::env::args().skip_while(|a| a != "--dump").nth(1).and_then(|s| s.parse().ok()).unwrap_or(14);
        let from = cache::monday(today);
        match rt.block_on(celcat::fetch(&cfg, &pwd, from, from + Duration::days(days))) {
            Ok(cours) => {
                for c in &cours {
                    println!(
                        "{} {}-{}  {:<4} {:<40} {:<28} {}",
                        c.date(),
                        c.start.format("%H:%M"),
                        c.end.format("%H:%M"),
                        c.category,
                        c.matiere,
                        c.prof,
                        c.salle_courte
                    );
                }
                println!("{} cours", cours.len());
            }
            Err(e) => {
                eprintln!("Erreur : {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // `cydera --mail-body <id>` : affiche le texte d'un message (sans le marquer comme lu).
    if let Some(id) = std::env::args().skip_while(|a| a != "--mail-body").nth(1).and_then(|s| s.parse::<u32>().ok()) {
        let cfg = config::Config::load();
        let Some(pwd) = config::load_mail_password() else {
            eprintln!("Mot de passe introuvable (CELCAT_PASSWORD ou ~/CELCAT_PASSWORD)");
            std::process::exit(1);
        };
        match rt.block_on(mail::body(&cfg.mail_host, &cfg.mail_user, &pwd, id)) {
            Ok(t) => println!("{t}"),
            Err(e) => {
                eprintln!("Erreur : {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // `cydera --mail` : liste la boîte de réception sans interface.
    if std::env::args().any(|a| a == "--mail") {
        let cfg = config::Config::load();
        let Some(pwd) = config::load_mail_password() else {
            eprintln!("Mot de passe mail introuvable (CYU_MAIL_PASSWORD ou ~/CYU_MAIL_PASSWORD)");
            std::process::exit(1);
        };
        match rt.block_on(mail::list(&cfg.mail_host, &cfg.mail_user, &pwd, 20)) {
            Ok(l) => l
                .iter()
                .for_each(|m| println!("{} {:>7} {:<12} {:<24} {}", if m.unread { "●" } else { " " }, m.uid, m.date, m.from, m.subject)),
            Err(e) => {
                eprintln!("Erreur : {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

    // Import des fiches ECTS (PDF) : dialogue du système + lecteur, propres au PC.
    cydera_ui::files::set_pdf_picker(|| {
        rfd::FileDialog::new().set_title("Fiche ECTS (PDF)").add_filter("PDF", &["pdf", "PDF"]).pick_file()
    });
    cydera_ui::files::set_ects_parser(cydera_core::grades::ects::parse_ects);

    let handle = rt.handle().clone();
    eframe::run_native(
        "Cydera",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default()
                .with_title("Cydera")
                .with_app_id("cydera")
                .with_inner_size([1360.0, 820.0])
                .with_min_inner_size([340.0, 460.0])
                .with_icon(eframe::egui::IconData {
                    rgba: include_bytes!("../../../crates/cydera-ui/assets/icon128.rgba").to_vec(),
                    width: 128,
                    height: 128,
                }),
            ..Default::default()
        },
        Box::new(move |cc| {
            ui::theme::apply(&cc.egui_ctx);
            Ok(Box::new(app::App::new(handle, cc.egui_ctx.clone())))
        }),
    )
}
