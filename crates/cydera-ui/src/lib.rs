//! Interface partagée Desktop / Android. Les modules métier sont ré-exportés
//! pour que `crate::model`, `crate::config`… restent valides ici.
pub use cydera_core::{cache, celcat, config, grades, mail, mail_cache, model, resto, study};

pub mod app;
pub mod clipboard;
pub mod files;
pub mod insets;
pub mod p2p;
pub mod ui;
pub mod update;
