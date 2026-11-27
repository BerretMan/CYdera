//! Profils prêts à l'emploi, tirés des fiches ECTS officielles (régénérés par le test
//! `ects::tests::dump_templates` à partir de `tests/fixtures/`).

use super::model::Profile;

const SOURCES: [&str; 2] = [include_str!("templates/ing2-gi-a.json"), include_str!("templates/ing2-gi-b.json")];

/// (libellé, profil sans identifiant) ; `Book::add_profile` lui en donne un.
pub fn templates() -> Vec<(String, Profile)> {
    SOURCES
        .iter()
        .filter_map(|s| serde_json::from_str::<Profile>(s).ok())
        .map(|p| {
            let label = if p.year.is_empty() { p.name.clone() } else { format!("{} · {}", p.name, p.year) };
            (label, p)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn templates_valides() {
        let ts = super::templates();
        assert_eq!(ts.len(), super::SOURCES.len(), "un template JSON ne se lit plus");
        assert!(ts.iter().all(|(_, p)| p.ects_total() == 60.0));
    }
}
