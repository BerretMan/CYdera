//! Régression : avec `ring` (iroh) ET `aws-lc-rs` (reqwest) dans le même programme, rustls ne
//! peut plus choisir de fournisseur par défaut et panique au premier `ClientConfig::builder()`.
//! En release (`panic = "abort"`) l'appli Android se fermait dès la première requête Celcat.
use cydera_core::net;

#[test]
fn le_conflit_existe_puis_la_correction_le_leve() {
    // Témoin : sans fournisseur installé, le builder par défaut panique (les deux moteurs sont là).
    let sans_init = std::panic::catch_unwind(|| {
        let _ = rustls::ClientConfig::builder();
    });
    assert!(sans_init.is_err(), "précondition du test : le conflit ring / aws-lc-rs doit exister");

    // Correction : après `crypto_init`, tout fonctionne, y compris le builder « nu » de rustls.
    net::crypto_init();
    let _ = rustls::ClientConfig::builder();
    let _ = net::tls_client_config();
    net::client_builder().build().expect("client HTTP");
}
