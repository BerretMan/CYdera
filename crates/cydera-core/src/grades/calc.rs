//! Calcul des moyennes CY Tech.
//!
//! - matière (EC) : moyenne de ses notes pondérées par leur poids, ramenées sur 20 ;
//! - UE : moyenne des EC pondérée par leur coef (EC de coef 0 = non évaluée, ignorée) ;
//! - semestre : moyenne des UE pondérée par leur coef ; annuelle : idem sur toutes les UE ;
//! - seuil : une EC sous sa note seuil (6/20) empêche de valider son UE.
//!
//! Une matière sans note ne compte pas encore : la moyenne est alors « provisoire ».

use super::model::{Grade, Profile, Ue};
use std::collections::BTreeMap;

/// Note minimale de validation d'une UE.
pub const PASS: f32 = 10.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Avg {
    pub value: Option<f32>,
    /// Toutes les matières évaluées concernées ont au moins une note.
    pub complete: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Alert {
    /// Matière sous sa note seuil.
    Seuil { ec: String, avg: f32, seuil: f32 },
    /// UE sous 10.
    Ue { ue: String, avg: f32 },
}

#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Moyenne de chaque matière (`Ec::id`), `None` sans note.
    pub ec: BTreeMap<String, Option<f32>>,
    pub ue: BTreeMap<String, Avg>,
    /// Une entrée par semestre, dans l'ordre du profil.
    pub semesters: Vec<Avg>,
    pub year: Avg,
    pub alerts: Vec<Alert>,
}

impl Report {
    pub fn ec_below(&self, ec: &str) -> bool {
        self.alerts.iter().any(|a| matches!(a, Alert::Seuil { ec: e, .. } if e == ec))
    }

    /// UE validée : moyenne ≥ 10 et aucune matière sous le seuil.
    pub fn ue_ok(&self, ue: &Ue) -> Option<bool> {
        let avg = self.ue.get(&ue.id)?.value?;
        Some(avg >= PASS && !ue.ecs.iter().any(|e| self.ec_below(&e.id)))
    }
}

/// Moyenne pondérée de couples (valeur, poids) ; `None` si aucun poids positif.
fn weighted(items: impl IntoIterator<Item = (f32, f32)>) -> Option<f32> {
    let (mut num, mut den) = (0.0f64, 0.0f64);
    for (v, w) in items {
        if w > 0.0 && v.is_finite() {
            num += f64::from(v) * f64::from(w);
            den += f64::from(w);
        }
    }
    (den > 0.0).then(|| (num / den) as f32)
}

/// Moyenne d'une matière à partir de ses notes.
pub fn ec_avg<'a>(grades: impl IntoIterator<Item = &'a Grade>) -> Option<f32> {
    weighted(grades.into_iter().map(|g| (g.on20(), g.weight)))
}

pub fn report(p: &Profile, grades: &[Grade]) -> Report {
    let mut r = Report::default();
    let mut all_ues: Vec<(f32, Avg)> = Vec::new();
    for s in &p.semesters {
        let mut sem_ues = Vec::new();
        for u in &s.ues {
            let mut complete = true;
            let mut parts = Vec::new();
            for e in &u.ecs {
                let avg = ec_avg(grades.iter().filter(|g| g.profile == p.id && g.ec == e.id));
                r.ec.insert(e.id.clone(), avg);
                if e.coef <= 0.0 {
                    continue;
                }
                match avg {
                    Some(a) => {
                        parts.push((a, e.coef));
                        if let Some(seuil) = e.seuil.filter(|s| a < *s) {
                            r.alerts.push(Alert::Seuil { ec: e.id.clone(), avg: a, seuil });
                        }
                    }
                    None => complete = false,
                }
            }
            let avg = Avg { value: weighted(parts), complete };
            if let Some(a) = avg.value.filter(|a| *a < PASS) {
                r.alerts.push(Alert::Ue { ue: u.id.clone(), avg: a });
            }
            r.ue.insert(u.id.clone(), avg);
            sem_ues.push((u.coef, avg));
            all_ues.push((u.coef, avg));
        }
        r.semesters.push(combine(&sem_ues));
    }
    r.year = combine(&all_ues);
    r
}

/// Moyenne des UE pondérée par leur coef.
fn combine(ues: &[(f32, Avg)]) -> Avg {
    let value = weighted(ues.iter().filter_map(|(c, a)| a.value.map(|v| (v, *c))));
    let complete = ues.iter().filter(|(c, _)| *c > 0.0).all(|(_, a)| a.complete && a.value.is_some());
    Avg { value, complete }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grades::model::{Ec, Semester};

    fn ec(id: &str, coef: f32, seuil: Option<f32>) -> Ec {
        Ec { id: id.into(), code: id.to_uppercase(), name: id.into(), coef, seuil }
    }

    fn g(ec: &str, value: f32, out_of: f32, weight: f32) -> Grade {
        Grade { id: format!("{ec}-{value}"), profile: "p".into(), ec: ec.into(), label: String::new(), value, out_of, weight }
    }

    fn profile() -> Profile {
        Profile {
            id: "p".into(),
            name: "Test".into(),
            year: String::new(),
            semesters: vec![
                Semester {
                    name: "S1".into(),
                    ues: vec![
                        Ue {
                            id: "u1".into(),
                            code: "UE1".into(),
                            name: "".into(),
                            ects: 10.0,
                            coef: 10.0,
                            ecs: vec![ec("a", 4.0, Some(6.0)), ec("b", 2.0, Some(6.0))],
                        },
                        Ue {
                            id: "u2".into(),
                            code: "UE2".into(),
                            name: "".into(),
                            ects: 6.0,
                            coef: 6.0,
                            ecs: vec![ec("c", 1.0, Some(6.0)), ec("lv2", 0.0, None)],
                        },
                    ],
                },
                Semester {
                    name: "S2".into(),
                    ues: vec![Ue {
                        id: "u3".into(),
                        code: "UE1".into(),
                        name: "".into(),
                        ects: 4.0,
                        coef: 4.0,
                        ecs: vec![ec("d", 1.0, None)],
                    }],
                },
            ],
        }
    }

    fn close(a: Option<f32>, b: f32) {
        let a = a.expect("moyenne attendue");
        assert!((a - b).abs() < 1e-4, "{a} ≠ {b}");
    }

    #[test]
    fn notes_ponderees_et_baremes() {
        // 15/20 (poids 1) et 8/10 = 16/20 (poids 3) → (15 + 48) / 4 = 15,75
        let gs = [g("a", 15.0, 20.0, 1.0), g("a", 8.0, 10.0, 3.0)];
        close(ec_avg(&gs), 15.75);
        assert_eq!(ec_avg(&[]), None);
    }

    #[test]
    fn moyennes_ue_semestre_annee() {
        let p = profile();
        let gs = vec![g("a", 12.0, 20.0, 1.0), g("b", 9.0, 20.0, 1.0), g("c", 14.0, 20.0, 1.0), g("d", 10.0, 20.0, 1.0)];
        let r = report(&p, &gs);
        close(r.ue["u1"].value, (12.0 * 4.0 + 9.0 * 2.0) / 6.0); // 11
        close(r.ue["u2"].value, 14.0); // LV2 coef 0 ignorée
        close(r.semesters[0].value, (11.0 * 10.0 + 14.0 * 6.0) / 16.0);
        close(r.semesters[1].value, 10.0);
        close(r.year.value, (11.0 * 10.0 + 14.0 * 6.0 + 10.0 * 4.0) / 20.0);
        assert!(r.year.complete);
        assert!(r.alerts.is_empty());
    }

    #[test]
    fn provisoire_tant_qu_une_matiere_n_a_pas_de_note() {
        let p = profile();
        let r = report(&p, &[g("a", 12.0, 20.0, 1.0)]);
        close(r.ue["u1"].value, 12.0);
        assert!(!r.ue["u1"].complete);
        assert!(!r.year.complete);
        assert_eq!(r.semesters[1].value, None);
    }

    #[test]
    fn seuil_et_ue_non_validee() {
        let p = profile();
        let r = report(&p, &[g("a", 16.0, 20.0, 1.0), g("b", 5.0, 20.0, 1.0)]);
        // moyenne d'UE ≥ 10 mais une matière sous 6 : UE non validée
        close(r.ue["u1"].value, (16.0 * 4.0 + 5.0 * 2.0) / 6.0);
        assert!(r.ec_below("b"));
        assert_eq!(r.ue_ok(&p.semesters[0].ues[0]), Some(false));
        let r = report(&p, &[g("c", 8.0, 20.0, 1.0)]);
        assert!(r.alerts.contains(&Alert::Ue { ue: "u2".into(), avg: 8.0 }));
    }

    #[test]
    fn notes_d_un_autre_profil_ignorees() {
        let p = profile();
        let mut autre = g("a", 2.0, 20.0, 1.0);
        autre.profile = "x".into();
        assert_eq!(report(&p, &[autre]).ec["a"], None);
    }
}
