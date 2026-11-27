//! Structure d'un profil de notes (tiré d'une fiche ECTS) et des notes saisies.

use serde::{Deserialize, Serialize};

/// Un cursus d'une année : semestres → UE → matières (EC / SAE).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    /// Identifiant aléatoire (un même template peut être instancié deux fois).
    pub id: String,
    pub name: String,
    /// Année universitaire, ex. « 2026-2027 ».
    #[serde(default)]
    pub year: String,
    pub semesters: Vec<Semester>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Semester {
    pub name: String,
    pub ues: Vec<Ue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ue {
    /// Identifiant stable dans le profil (`s1.ue2`) : les notes y font référence via leurs EC.
    pub id: String,
    /// Code de la fiche : « UE1 », « UE2a »…
    pub code: String,
    pub name: String,
    pub ects: f32,
    /// Poids de l'UE dans les moyennes de semestre et annuelle.
    pub coef: f32,
    pub ecs: Vec<Ec>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ec {
    /// Identifiant stable dans le profil (`s1.ue2.ec3`, `s2.ue3.sae1`).
    pub id: String,
    /// « EC1 », « SAE1 »…
    pub code: String,
    pub name: String,
    /// Poids dans la moyenne de l'UE ; 0 = non évalué.
    pub coef: f32,
    /// Note seuil (6/20 en général) : en dessous, l'UE n'est pas validée.
    #[serde(default)]
    pub seuil: Option<f32>,
}

fn vingt() -> f32 {
    20.0
}

fn un() -> f32 {
    1.0
}

/// Une note obtenue dans une matière (partiel, CC, projet…).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Grade {
    pub id: String,
    /// Profil auquel la note appartient.
    pub profile: String,
    /// Matière (`Ec::id`).
    pub ec: String,
    #[serde(default)]
    pub label: String,
    pub value: f32,
    /// Barème (« sur 20 » par défaut).
    #[serde(default = "vingt")]
    pub out_of: f32,
    /// Poids de la note dans la moyenne de la matière.
    #[serde(default = "un")]
    pub weight: f32,
}

impl Grade {
    /// Valeur ramenée sur 20.
    pub fn on20(&self) -> f32 {
        if self.out_of > 0.0 { self.value / self.out_of * 20.0 } else { 0.0 }
    }
}

impl Profile {
    pub fn ec(&self, id: &str) -> Option<&Ec> {
        self.semesters.iter().flat_map(|s| &s.ues).flat_map(|u| &u.ecs).find(|e| e.id == id)
    }

    pub fn ects_total(&self) -> f32 {
        self.semesters.iter().flat_map(|s| &s.ues).map(|u| u.ects).sum()
    }

    /// Recalcule des identifiants d'UE/EC uniques après une modification de la structure
    /// (ajout d'une matière…), sans toucher à ceux qui existent déjà.
    pub fn ensure_ids(&mut self) {
        let mut seen = std::collections::BTreeSet::new();
        for (si, s) in self.semesters.iter_mut().enumerate() {
            for u in &mut s.ues {
                if u.id.is_empty() || !seen.insert(u.id.clone()) {
                    u.id = unique(&mut seen, &format!("s{}.{}", si + 1, slug(&u.code)));
                }
                for e in &mut u.ecs {
                    if e.id.is_empty() || !seen.insert(e.id.clone()) {
                        e.id = unique(&mut seen, &format!("{}.{}", u.id, slug(&e.code)));
                    }
                }
            }
        }
    }
}

fn unique(seen: &mut std::collections::BTreeSet<String>, base: &str) -> String {
    let mut id = base.to_string();
    let mut n = 2;
    while !seen.insert(id.clone()) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// « SAE1*** » → « sae1 », « UE2a » → « ue2a ».
pub fn slug(code: &str) -> String {
    let s: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_lowercase();
    if s.is_empty() { "x".into() } else { s }
}
