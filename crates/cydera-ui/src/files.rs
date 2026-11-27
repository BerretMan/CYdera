//! Import de fiches ECTS, fourni par l'hôte : sélecteur de fichier PDF (PC : dialogue du système)
//! et lecteur de fiche (`cydera_core::grades::ects`, compilé seulement sur PC).
//! Sans fournisseur (Android), l'écran Notes n'affiche pas le bouton d'import : les profils
//! importés sur le PC arrivent par la synchro.
use cydera_core::grades::model::Profile;
use std::path::PathBuf;
use std::sync::OnceLock;

type Picker = Box<dyn Fn() -> Option<PathBuf> + Send + Sync>;
type Parser = Box<dyn Fn(&[u8]) -> Result<Vec<Profile>, String> + Send + Sync>;
static PICKER: OnceLock<Picker> = OnceLock::new();
static PARSER: OnceLock<Parser> = OnceLock::new();

/// Dialogue « Ouvrir un PDF » (bloquant : appelé hors du fil de l'interface).
pub fn set_pdf_picker(f: impl Fn() -> Option<PathBuf> + Send + Sync + 'static) {
    let _ = PICKER.set(Box::new(f));
}

pub fn set_ects_parser(f: impl Fn(&[u8]) -> Result<Vec<Profile>, String> + Send + Sync + 'static) {
    let _ = PARSER.set(Box::new(f));
}

/// Vrai si l'import de fiche ECTS est disponible sur cet appareil.
pub fn can_import() -> bool {
    PARSER.get().is_some()
}

pub fn can_pick() -> bool {
    PICKER.get().is_some()
}

pub fn pick_pdf() -> Option<PathBuf> {
    PICKER.get().and_then(|f| f())
}

pub fn parse_ects(bytes: &[u8]) -> Result<Vec<Profile>, String> {
    match PARSER.get() {
        Some(f) => f(bytes),
        None => Err("Import de fiche ECTS indisponible sur cet appareil.".into()),
    }
}
