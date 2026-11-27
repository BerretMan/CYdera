//! TLS et clients HTTP.
//!
//! Le programme embarque deux moteurs de chiffrement : `aws-lc-rs` (reqwest) et `ring` (iroh).
//! Rustls ne peut alors plus choisir seul un fournisseur par défaut et *panique* au premier
//! `ClientConfig::builder()` ; avec `panic = "abort"` (release) l'appli se ferme aussitôt.
//! On impose donc explicitement `aws-lc-rs`, ici, pour tout le monde.
use std::sync::{Arc, Once};
use tokio_rustls::rustls;

/// Installe `aws-lc-rs` comme fournisseur par défaut du processus (idempotent).
/// À appeler au démarrage ; les fonctions de ce module l'appellent aussi par sécurité.
pub fn crypto_init() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        // `Err` = un autre code a déjà installé un fournisseur : très bien.
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    });
}

/// Configuration TLS cliente avec les racines `webpki-roots` (IMAP, et HTTP sur Android où
/// le vérificateur « plateforme » exigerait du code Java absent de l'APK).
pub fn tls_client_config() -> rustls::ClientConfig {
    crypto_init();
    let roots = rustls::RootCertStore { roots: webpki_roots::TLS_SERVER_ROOTS.to_vec() };
    rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::aws_lc_rs::default_provider()))
        .with_safe_default_protocol_versions()
        .expect("versions TLS par défaut")
        .with_root_certificates(roots)
        .with_no_client_auth()
}

pub fn client_builder() -> reqwest::ClientBuilder {
    crypto_init();
    let b = reqwest::Client::builder();
    #[cfg(target_os = "android")]
    let b = b.use_preconfigured_tls(tls_client_config());
    b
}
