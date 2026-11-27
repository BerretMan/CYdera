//! Lecture d'une fiche ECTS (« Modalités de contrôle des connaissances », PDF exporté d'Excel)
//! en profils de notes.
//!
//! Méthode : on récupère chaque caractère avec sa position (`pdf-extract`), on reforme les
//! cellules (caractères proches sur une même ligne), puis on repère les colonnes grâce aux
//! en-têtes « Autre », « ECTS », « Coef », « Note-seuil », « type de contrôle ». Chaque ligne
//! dont la première cellule est un code `UE…` décrit une UE (ECTS, coef) ; un code `EC…`/`SAE…`
//! décrit une matière (coef, seuil). « Semestre N » ouvre un nouveau tableau.
//!
//! UE à choix (UE2a / UE2b dans un même semestre) : un profil par lettre.

use super::model::{Ec, Profile, Semester, Ue, slug};
use pdf_extract::{MediaBox, OutputDev, OutputError, Transform};

/// Un caractère placé sur la page (repère PDF : y vers le haut).
#[derive(Clone, Debug)]
struct Glyph {
    page: u32,
    x: f64,
    y: f64,
    end: f64,
    size: f64,
    ch: String,
}

#[derive(Default)]
struct Collector {
    page: u32,
    glyphs: Vec<Glyph>,
}

impl OutputDev for Collector {
    fn begin_page(&mut self, page_num: u32, _: &MediaBox, _: Option<(f64, f64, f64, f64)>) -> Result<(), OutputError> {
        self.page = page_num;
        Ok(())
    }
    fn end_page(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn output_character(&mut self, trm: &Transform, width: f64, _spacing: f64, font_size: f64, ch: &str) -> Result<(), OutputError> {
        // taille du corps après transformation (comme `pdf-extract` pour son texte brut)
        let (vx, vy) = (font_size * (trm.m11 + trm.m21), font_size * (trm.m12 + trm.m22));
        let size = (vx * vy).abs().sqrt();
        self.glyphs.push(Glyph { page: self.page, x: trm.m31, y: trm.m32, end: trm.m31 + width * size, size, ch: ch.to_string() });
        Ok(())
    }
    fn begin_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_word(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
    fn end_line(&mut self) -> Result<(), OutputError> {
        Ok(())
    }
}

/// Texte d'une cellule et son étendue horizontale.
#[derive(Clone, Debug)]
struct Cell {
    x0: f64,
    x1: f64,
    text: String,
}

impl Cell {
    fn mid(&self) -> f64 {
        (self.x0 + self.x1) / 2.0
    }
}

#[derive(Clone, Debug)]
struct Row {
    cells: Vec<Cell>,
}

/// Regroupe les caractères en lignes (même page, même ligne de base) puis en cellules.
fn rows(mut glyphs: Vec<Glyph>) -> Vec<Row> {
    glyphs.retain(|g| !g.ch.trim().is_empty() || g.ch == " ");
    // ordre de lecture : page, haut → bas, gauche → droite
    glyphs.sort_by(|a, b| a.page.cmp(&b.page).then(b.y.total_cmp(&a.y)).then(a.x.total_cmp(&b.x)));
    let mut lines: Vec<Vec<Glyph>> = Vec::new();
    for g in glyphs {
        match lines.last_mut() {
            Some(l) if l[0].page == g.page && (l[0].y - g.y).abs() <= 1.5 => l.push(g),
            _ => lines.push(vec![g]),
        }
    }
    lines
        .into_iter()
        .map(|mut l| {
            l.sort_by(|a, b| a.x.total_cmp(&b.x));
            let mut cells: Vec<Cell> = Vec::new();
            let mut last_end = f64::NEG_INFINITY;
            for g in l {
                // un vide de plus d'une demi-cadratin sépare deux cellules (une espace en fait ~0,23)
                let gap = g.x - last_end;
                match cells.last_mut() {
                    Some(c) if gap <= 0.5 * g.size => {
                        if gap > 0.15 * g.size && !c.text.ends_with(' ') && g.ch != " " {
                            c.text.push(' ');
                        }
                        c.text.push_str(&g.ch);
                        c.x1 = c.x1.max(g.end);
                    }
                    _ if g.ch == " " => {}
                    _ => cells.push(Cell { x0: g.x, x1: g.end, text: g.ch.clone() }),
                }
                last_end = last_end.max(g.end);
            }
            for c in &mut cells {
                c.text = c.text.split_whitespace().collect::<Vec<_>>().join(" ");
            }
            cells.retain(|c| !c.text.is_empty());
            Row { cells }
        })
        .filter(|r| !r.cells.is_empty())
        .collect()
}

/// Colonnes repérées sur la ligne d'en-tête (abscisse du centre de chaque titre).
#[derive(Clone, Copy, Debug)]
struct Columns {
    autre: f64,
    ects: f64,
    coef: f64,
    seuil: f64,
    controle: f64,
    /// Début de la colonne « Mutualisation » : fin des intitulés.
    name_end: f64,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Col {
    Autre,
    Ects,
    Coef,
    Seuil,
    Controle,
}

impl Columns {
    /// Colonne la plus proche du centre de la cellule (parmi celles qui nous intéressent et leurs voisines).
    fn of(&self, c: &Cell) -> Option<Col> {
        let m = c.mid();
        let cands = [
            (Col::Autre, self.autre),
            (Col::Ects, self.ects),
            (Col::Coef, self.coef),
            (Col::Seuil, self.seuil),
            (Col::Controle, self.controle),
        ];
        let (col, x) = cands.into_iter().min_by(|a, b| (a.1 - m).abs().total_cmp(&(b.1 - m).abs()))?;
        // pas plus loin que l'écart entre ECTS et Coef : au-delà, la cellule est ailleurs
        ((x - m).abs() <= (self.coef - self.ects).abs().max(8.0)).then_some(col)
    }
}

fn find(rows: &[Row], pred: impl Fn(&str) -> bool) -> Option<&Cell> {
    rows.iter().flat_map(|r| &r.cells).find(|c| pred(&c.text))
}

fn columns(rows: &[Row]) -> Result<Columns, String> {
    let need = |name: &str, c: Option<&Cell>| {
        c.map(|c| c.mid()).ok_or_else(|| format!("Colonne « {name} » introuvable : ce PDF ne ressemble pas à une fiche ECTS."))
    };
    let ects = need("ECTS", find(rows, |t| t == "ECTS"))?;
    let coef = need("Coef", find(rows, |t| t == "Coef"))?;
    let seuil = need("Note-seuil", find(rows, |t| t.starts_with("Note") || t == "seuil"))?;
    let autre = find(rows, |t| t == "Autre").map_or(ects - (coef - ects) * 2.0, |c| c.mid());
    let controle = rows
        .iter()
        .flat_map(|r| &r.cells)
        .filter(|c| c.text.starts_with("type de") && c.mid() > seuil)
        .map(|c| c.mid())
        .fold(f64::INFINITY, f64::min);
    let controle = if controle.is_finite() { controle } else { seuil + (seuil - coef) * 2.0 };
    let name_end = find(rows, |t| t.starts_with("Mutualisation")).map_or(autre - 120.0, |c| c.x0 - 2.0);
    Ok(Columns { autre, ects, coef, seuil, controle, name_end })
}

/// « 7,5 » → 7.5
fn num(s: &str) -> Option<f32> {
    s.trim().replace(',', ".").parse().ok()
}

fn is_ue(code: &str) -> bool {
    let rest = code.strip_prefix("UE").unwrap_or("");
    let digits = rest.trim_end_matches(|c: char| c.is_ascii_lowercase());
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) && rest.len() - digits.len() <= 1
}

fn is_ec(code: &str) -> bool {
    let rest = code.strip_prefix("EC").or_else(|| code.strip_prefix("SAE")).unwrap_or("");
    let digits = rest.trim_end_matches('*');
    !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit())
}

/// Lettre de variante d'une UE (« UE2a » → `Some('a')`).
fn variant(code: &str) -> Option<char> {
    code.chars().last().filter(|c| c.is_ascii_lowercase())
}

/// Valeur d'un champ d'en-tête « Libellé : valeur » (dans la même cellule ou la suivante).
fn header_value(rows: &[Row], label: &str) -> Option<String> {
    for r in rows {
        for (i, c) in r.cells.iter().enumerate() {
            if let Some(rest) = c.text.strip_prefix(label) {
                let v = rest.trim_start_matches([' ', ':']).trim();
                if !v.is_empty() {
                    return Some(v.to_string());
                }
                return r.cells.get(i + 1).map(|n| n.text.trim().to_string());
            }
        }
    }
    None
}

/// Lit une fiche ECTS. Renvoie un profil par parcours (un seul sans UE à choix).
pub fn parse_ects(pdf: &[u8]) -> Result<Vec<Profile>, String> {
    let doc = pdf_extract::Document::load_mem(pdf).map_err(|e| format!("PDF illisible : {e}"))?;
    let mut col = Collector::default();
    pdf_extract::output_doc(&doc, &mut col).map_err(|e| format!("Texte du PDF illisible : {e}"))?;
    if col.glyphs.is_empty() {
        return Err("Ce PDF ne contient pas de texte (scan ?) : impossible de lire la fiche.".into());
    }
    parse_rows(&rows(col.glyphs))
}

fn parse_rows(rows: &[Row]) -> Result<Vec<Profile>, String> {
    let cols = columns(rows)?;
    let mut semesters: Vec<Semester> = Vec::new();
    for r in rows {
        let first = &r.cells[0];
        if let Some(sem) = r.cells.iter().find(|c| c.text.starts_with("Semestre ")) {
            let n: String = sem.text["Semestre ".len()..].chars().take_while(|c| c.is_ascii_digit()).collect();
            if !n.is_empty() {
                semesters.push(Semester { name: format!("Semestre {n}"), ues: Vec::new() });
            }
        }
        let code = first.text.split_whitespace().next().unwrap_or("");
        let (ue, ec) = (is_ue(code), is_ec(code));
        if !ue && !ec {
            continue;
        }
        let Some(sem) = semesters.last_mut() else { continue };
        // intitulé : texte après le code, jusqu'à la colonne « Mutualisation »
        let mut name: Vec<&str> = Vec::new();
        let after_code = first.text[code.len()..].trim();
        if !after_code.is_empty() {
            name.push(after_code);
        }
        name.extend(r.cells[1..].iter().filter(|c| c.x0 < cols.name_end).map(|c| c.text.as_str()));
        let name = name.join(" ");
        let mut ects = None;
        let mut coef = None;
        let mut seuil = None;
        let mut not_evaluated = false;
        for c in &r.cells[1..] {
            if c.text.contains("Non évalué") {
                not_evaluated = true;
            }
            if c.x0 < cols.name_end {
                continue;
            }
            match cols.of(c) {
                Some(Col::Ects) => ects = ects.or(num(&c.text)),
                Some(Col::Coef) => coef = coef.or(num(&c.text)),
                Some(Col::Seuil) => seuil = seuil.or(num(&c.text)),
                _ => {}
            }
        }
        if ue {
            let ects = ects.or(coef).unwrap_or(0.0);
            let id = format!("s{}.{}", semesters.len(), slug(code));
            let sem = semesters.last_mut().unwrap();
            sem.ues.push(Ue { id, code: code.to_string(), name, ects, coef: coef.unwrap_or(ects), ecs: Vec::new() });
        } else if let Some(u) = sem.ues.last_mut() {
            let coef = if not_evaluated && coef.is_none() { 0.0 } else { coef.unwrap_or(1.0) };
            let id = format!("{}.{}", u.id, slug(code));
            u.ecs.push(Ec { id, code: code.trim_end_matches('*').to_string(), name, coef, seuil });
        }
    }
    semesters.retain(|s| !s.ues.is_empty());
    if semesters.is_empty() {
        return Err("Aucune UE trouvée dans le PDF : ce n'est pas une fiche ECTS reconnue.".into());
    }

    let niveau = header_value(rows, "Niveau").unwrap_or_default();
    let year = header_value(rows, "Année universitaire").unwrap_or_default();
    let mention = header_value(rows, "Type de diplôme et intitulé mention et parcours").unwrap_or_default();
    // « FISE Informatique Cergy et Pau » → « Informatique »
    let mention = mention
        .split_whitespace()
        .find(|w| w.chars().next().is_some_and(char::is_uppercase) && !w.chars().all(|c| c.is_uppercase()))
        .unwrap_or("")
        .to_string();
    let base = [niveau.as_str(), mention.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" ");
    let base = if base.is_empty() { "Fiche ECTS".to_string() } else { base };

    let mut letters: Vec<char> = semesters.iter().flat_map(|s| &s.ues).filter_map(|u| variant(&u.code)).collect();
    letters.sort_unstable();
    letters.dedup();
    let profile = |name: String, keep: &dyn Fn(&Ue) -> bool| Profile {
        id: String::new(),
        name,
        year: year.clone(),
        semesters: semesters
            .iter()
            .map(|s| Semester { name: s.name.clone(), ues: s.ues.iter().filter(|u| keep(u)).cloned().collect() })
            .collect(),
    };
    if letters.is_empty() {
        return Ok(vec![profile(base, &|_| true)]);
    }
    Ok(letters.into_iter().map(|l| profile(format!("{base}-{l}"), &move |u: &Ue| variant(&u.code).is_none_or(|v| v == l))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fiche ECTS de l'école : non publiée dans le dépôt (document de CY Tech), donc lue à
    /// l'exécution. Absente (CI, clone public) → les tests qui en dépendent sont sautés.
    fn fiche() -> Option<Vec<u8>> {
        let p = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/ects-ing2-info-2026.pdf");
        let r = std::fs::read(p).ok();
        if r.is_none() {
            eprintln!("fixture ECTS absente ({p}) : test ignoré");
        }
        r
    }

    /// Régénère les templates : `cargo test -p cydera-core --features ects-pdf dump_templates -- --ignored`
    #[test]
    #[ignore]
    fn dump_templates() {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/src/grades/templates");
        let fiche = fiche().expect("fixture ECTS requise pour régénérer les templates");
        for (p, (file, name)) in
            parse_ects(&fiche).unwrap().into_iter().zip([("ing2-gi-a.json", "ING2 GI-a"), ("ing2-gi-b.json", "ING2 GI-b")])
        {
            let p = Profile { name: name.into(), ..p };
            std::fs::write(format!("{dir}/{file}"), serde_json::to_string_pretty(&p).unwrap() + "\n").unwrap();
        }
    }

    #[test]
    fn codes() {
        assert!(is_ue("UE1") && is_ue("UE2a") && !is_ue("UE") && !is_ue("UEx1"));
        assert!(is_ec("EC5") && is_ec("SAE1***") && !is_ec("ECTS"));
    }

    #[test]
    fn fiche_ing2_info_donne_les_deux_parcours() {
        let Some(fiche) = fiche() else { return };
        let ps = parse_ects(&fiche).unwrap();
        let names: Vec<&str> = ps.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["ING2 Informatique-a", "ING2 Informatique-b"]);
        for p in &ps {
            assert_eq!(p.year, "2026-2027");
            assert_eq!(p.semesters.len(), 2);
            for s in &p.semesters {
                assert_eq!(s.ues.iter().map(|u| u.ects).sum::<f32>(), 30.0, "{} de {}", s.name, p.name);
            }
        }
        let a = &ps[0];
        let codes: Vec<&str> = a.semesters[1].ues.iter().map(|u| u.code.as_str()).collect();
        assert_eq!(codes, ["UE1", "UE2a", "UE3", "UE4"]);
        assert_eq!(ps[1].semesters[1].ues[1].code, "UE2b");
        // S1, UE1 : coefs 4 3 4 3 2, seuil 6
        let ue1 = &a.semesters[0].ues[0];
        assert_eq!((ue1.ects, ue1.coef), (10.0, 10.0));
        assert_eq!(ue1.ecs.iter().map(|e| e.coef).collect::<Vec<_>>(), [4.0, 3.0, 4.0, 3.0, 2.0]);
        assert!(ue1.ecs.iter().all(|e| e.seuil == Some(6.0)));
        assert_eq!(ue1.ecs[0].name, "Statistiques appliquées et modélisation");
        // LV2 non évaluée, engagement sans seuil, stage sans coef → 1
        let ue3 = &a.semesters[0].ues[2];
        assert_eq!(ue3.ecs[1].coef, 0.0);
        assert_eq!(a.semesters[0].ues[3].ecs[0].seuil, None);
        let stage = &a.semesters[1].ues[3].ecs[0];
        assert_eq!((stage.name.as_str(), stage.coef, stage.seuil), ("Stage", 1.0, None));
    }

    #[test]
    fn templates_identiques_a_la_fiche() {
        let Some(fiche) = fiche() else { return };
        let ps = parse_ects(&fiche).unwrap();
        let ts = super::super::templates::templates();
        assert_eq!(ts.len(), ps.len());
        for ((_, t), p) in ts.iter().zip(&ps) {
            assert_eq!(t.semesters, p.semesters, "template {}", t.name);
        }
    }
}
