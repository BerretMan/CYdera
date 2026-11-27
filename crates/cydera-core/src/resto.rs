//! Menus des restaurants universitaires de Pau, lus sur les pages publiques du Crous Bordeaux-Aquitaine,
//! et fréquentation en direct (« repas servis ») lue sur l'API JSON de `restos.lescrous.fr`
//! (`/api/ecran/{code}`, celle qu'utilise leur page de jauge). Son `robots.txt` interdit les robots :
//! l'utilisateur a l'autorisation du Crous ; on reste léger (requête seulement fenêtre Resto ouverte,
//! au plus une par minute et par resto).

use chrono::NaiveDate;

pub struct Resto {
    pub name: &'static str,
    pub page: &'static str,
    /// Page de la jauge « repas servis » (affichage officiel).
    pub gauge: &'static str,
    /// Code du lieu pour l'API de jauge (`/api/ecran/{code}`).
    pub gauge_code: &'static str,
}

pub const RESTOS: [Resto; 2] = [
    Resto {
        name: "LA VAGUE",
        page: "https://www.crous-bordeaux.fr/restaurant/resto-u-la-vague/",
        gauge: "https://restos.lescrous.fr/fiche/5-21-16-1",
        gauge_code: "5-21-16-1",
    },
    Resto {
        name: "CAP SUD",
        page: "https://www.crous-bordeaux.fr/restaurant/resto-u-cap-sud/",
        gauge: "https://restos.lescrous.fr/fiche/5-20-21-1",
        gauge_code: "5-20-21-1",
    },
];

#[derive(Clone, Debug)]
pub struct Meal {
    pub name: String,
    /// (catégorie, plats) : « Plat Chaud » → [« Blé », …]
    pub sections: Vec<(String, Vec<String>)>,
}

#[derive(Clone, Debug)]
pub struct MenuDay {
    pub date: NaiveDate,
    pub meals: Vec<Meal>,
}

impl Meal {
    /// Plats de la catégorie « Plat du jour » (La Vague l'appelle « Plat Chaud »).
    pub fn plat_du_jour(&self) -> Option<&[String]> {
        self.sections.iter().find(|(c, _)| is_plat_du_jour(c)).map(|(_, v)| v.as_slice())
    }

    /// Les autres chaînes (grillade, entrées / desserts…), dans l'ordre du site.
    pub fn autres(&self) -> impl Iterator<Item = &(String, Vec<String>)> {
        self.sections.iter().filter(|(c, _)| !is_plat_du_jour(c))
    }
}

pub fn is_plat_du_jour(cat: &str) -> bool {
    let c = cat.to_lowercase();
    c.contains("plat du jour") || c.contains("plat chaud")
}

#[derive(Clone, Debug, Default)]
pub struct RestoData {
    pub days: Vec<MenuDay>,
}

pub async fn fetch(r: &Resto) -> Result<RestoData, String> {
    let client = crate::net::client_builder()
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) cydera")
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;
    let html = client
        .get(r.page)
        .send()
        .await
        .and_then(|x| x.error_for_status())
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let data = parse(&html);
    if data.days.is_empty() {
        return Err("Page du Crous illisible (structure changée ?)".into());
    }
    Ok(data)
}

// ------------------------------------------------------------------ fréquentation en direct

/// État de la jauge « repas servis » d'un resto.
#[derive(Clone, Debug, PartialEq)]
pub enum Gauge {
    /// Service en cours : repas déjà servis / capacité.
    Open { meals: u32, capacity: u32, color: String },
    /// Bascule midi → soir.
    Transition,
    /// Pas de service en ce moment.
    Closed,
}

#[derive(Clone, Debug)]
pub struct Live {
    pub gauge: Gauge,
    /// Heure de mise à jour annoncée par le Crous (« HH:MM »).
    pub updated: Option<String>,
    /// `false` si le Crous signale des données en retard.
    pub fresh: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ApiEcran {
    maj_le: Option<String>,
    #[serde(default = "yes")]
    donnees_a_jour: bool,
    #[serde(default)]
    lieux: Vec<ApiLieu>,
}

fn yes() -> bool {
    true
}

#[derive(serde::Deserialize)]
struct ApiLieu {
    repas: Option<u32>,
    jauge: Option<u32>,
    couleur: Option<String>,
    #[serde(default)]
    transition: bool,
}

pub fn parse_live(json: &str) -> Result<Live, String> {
    let api: ApiEcran = serde_json::from_str(json).map_err(|e| format!("réponse de jauge illisible : {e}"))?;
    let lieu = api.lieux.first().ok_or("jauge indisponible pour le moment")?;
    let gauge = if lieu.transition {
        Gauge::Transition
    } else {
        match (lieu.jauge, lieu.repas) {
            (Some(cap), Some(m)) if cap > 0 => Gauge::Open { meals: m, capacity: cap, color: lieu.couleur.clone().unwrap_or_default() },
            _ => Gauge::Closed,
        }
    };
    // « 2026-11-27T11:12:25+01:00 » → « 11:12 »
    let updated = api.maj_le.as_deref().and_then(|s| s.split_once('T')).and_then(|(_, t)| t.get(..5)).map(str::to_string);
    Ok(Live { gauge, updated, fresh: api.donnees_a_jour })
}

pub async fn fetch_live(r: &Resto) -> Result<Live, String> {
    let client = crate::net::client_builder()
        .user_agent("cydera (appli etudiante CY Tech)")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let json = client
        .get(format!("https://restos.lescrous.fr/api/ecran/{}", r.gauge_code))
        .header("Accept", "application/json")
        .send()
        .await
        .and_then(|x| x.error_for_status())
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    parse_live(&json)
}

// ------------------------------------------------------------------ parsing HTML minimal

#[derive(Debug)]
enum Tok<'a> {
    Open(&'a str, &'a str), // nom, attributs bruts
    Close(&'a str),
    Text(&'a str),
}

fn tokens(s: &str) -> Vec<Tok<'_>> {
    let mut out = Vec::new();
    let mut i = 0;
    let b = s.as_bytes();
    while i < b.len() {
        if b[i] == b'<' {
            let Some(end) = s[i..].find('>') else { break };
            let inner = &s[i + 1..i + end];
            i += end + 1;
            if inner.starts_with('!') || inner.starts_with('?') {
                continue;
            }
            if let Some(name) = inner.strip_prefix('/') {
                out.push(Tok::Close(name.trim()));
            } else {
                let inner = inner.trim_end_matches('/');
                let (name, attrs) = inner.split_once(char::is_whitespace).unwrap_or((inner, ""));
                out.push(Tok::Open(name, attrs));
            }
        } else {
            let end = s[i..].find('<').map_or(b.len(), |p| i + p);
            out.push(Tok::Text(&s[i..end]));
            i = end;
        }
    }
    out
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        out.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(semi) = rest.find(';').filter(|&x| x <= 8) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..semi];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "rsquo" | "lsquo" => Some('’'),
            "oelig" => Some('œ'),
            "eacute" => Some('é'),
            "egrave" => Some('è'),
            "agrave" => Some('à'),
            _ => ent
                .strip_prefix('#')
                .and_then(|n| if let Some(h) = n.strip_prefix(['x', 'X']) { u32::from_str_radix(h, 16).ok() } else { n.parse().ok() })
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

fn clean(s: &str) -> String {
    decode(s).split_whitespace().collect::<Vec<_>>().join(" ")
}

pub fn parse(html: &str) -> RestoData {
    let toks = tokens(html);
    RestoData { days: parse_menus(&toks) }
}

fn parse_menus(toks: &[Tok]) -> Vec<MenuDay> {
    let mut days: Vec<MenuDay> = Vec::new();
    let mut in_menus = false;
    let mut meal: Option<Meal> = None;
    let mut in_title = false;
    let mut foodies_depth = 0usize; // profondeur de <ul> dans meal_foodies (0 = hors liste)
    let mut cat: Option<String> = None;
    let mut li_text = String::new();
    let mut li_depth = 0usize;

    let flush_meal = |days: &mut Vec<MenuDay>, meal: &mut Option<Meal>| {
        if let (Some(m), Some(d)) = (meal.take(), days.last_mut()) {
            d.meals.push(m);
        }
    };

    for t in toks {
        match t {
            Tok::Open("section", a) if a.contains("menus_section") => in_menus = true,
            Tok::Close("section") if in_menus && foodies_depth == 0 => {
                flush_meal(&mut days, &mut meal);
                in_menus = false;
            }
            _ if !in_menus => {}
            Tok::Open("time", a) => {
                if let Some(d) =
                    a.split("datetime=\"").nth(1).and_then(|x| x.get(..10)).and_then(|x| NaiveDate::parse_from_str(x, "%Y-%m-%d").ok())
                {
                    flush_meal(&mut days, &mut meal);
                    days.push(MenuDay { date: d, meals: Vec::new() });
                }
            }
            Tok::Open("div", a) if a.contains("class=\"meal\"") => {
                flush_meal(&mut days, &mut meal);
                meal = Some(Meal { name: String::new(), sections: Vec::new() });
            }
            Tok::Open("h4", _) => in_title = true,
            Tok::Close("h4") => in_title = false,
            Tok::Text(x) if in_title => {
                if let Some(m) = meal.as_mut() {
                    m.name = clean(x);
                }
            }
            Tok::Open("ul", a) if a.contains("meal_foodies") => foodies_depth = 1,
            Tok::Open("ul", _) if foodies_depth > 0 => foodies_depth += 1,
            Tok::Close("ul") if foodies_depth > 0 => {
                foodies_depth -= 1;
                if foodies_depth == 0 {
                    cat = None;
                }
            }
            Tok::Open("li", _) if foodies_depth > 0 => {
                li_depth += 1;
                li_text.clear();
            }
            Tok::Text(x) if foodies_depth > 0 && li_depth > 0 => li_text.push_str(x),
            Tok::Close("li") if foodies_depth > 0 => {
                li_depth = li_depth.saturating_sub(1);
                let text = clean(&li_text);
                li_text.clear();
                if text.is_empty() {
                    continue;
                }
                if let Some(m) = meal.as_mut() {
                    if foodies_depth == 1 {
                        // <li> de premier niveau sans sous-liste : plat isolé
                        m.sections.push((String::new(), vec![text]));
                    } else if let Some(c) = &cat {
                        match m.sections.iter_mut().find(|(n, _)| n == c) {
                            Some((_, v)) => v.push(text),
                            None => m.sections.push((c.clone(), vec![text])),
                        }
                    }
                }
            }
            _ => {}
        }
        // Un <li> de premier niveau qui contient un <ul> : son texte est le titre de catégorie.
        if let Tok::Open("ul", _) = t
            && foodies_depth == 2
        {
            let c = clean(&li_text);
            if !c.is_empty() {
                cat = Some(c);
            }
            li_text.clear();
        }
    }
    flush_meal(&mut days, &mut meal);
    days
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_sample() {
        let l = parse_live(r#"{"majLe":"2026-11-27T11:12:25+01:00","donneesAJour":true,"lieux":[{"libelle":"Capsud Self UD","repas":20,"jauge":1500,"restant":1480,"pourcentageRestant":99,"couleur":"vert","depassement":false}]}"#).unwrap();
        assert_eq!(l.gauge, Gauge::Open { meals: 20, capacity: 1500, color: "vert".into() });
        assert_eq!(l.updated.as_deref(), Some("11:12"));
        let l = parse_live(r#"{"majLe":"2026-11-27T20:00:00+01:00","donneesAJour":true,"lieux":[{"libelle":"X","jauge":null}]}"#).unwrap();
        assert_eq!(l.gauge, Gauge::Closed);
        let l = parse_live(r#"{"lieux":[{"libelle":"X","transition":true}]}"#).unwrap();
        assert_eq!(l.gauge, Gauge::Transition);
        assert!(parse_live(r#"{"lieux":[]}"#).is_err());
    }

    #[test]
    fn menu_sample() {
        let html = r#"<section class="menus_section"><div class="menus"><div class="menu"><div class="menu_date"><h3><time datetime="2026-11-26">Menu</time></h3></div>
        <div class="meal"><h4 class="meal_title">Déjeuner</h4><ul class="meal_foodies"><li>Plat Chaud<ul><li>Emincé de dinde</li><li>Blé &amp; riz</li></ul></li><li>Grillade<ul><li>Frites</li></ul></li></ul></div></div></div></section>"#;
        let d = parse_menus(&tokens(html));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].meals[0].name, "Déjeuner");
        assert_eq!(d[0].meals[0].sections[0], ("Plat Chaud".to_string(), vec!["Emincé de dinde".to_string(), "Blé & riz".to_string()]));
        assert_eq!(d[0].meals[0].sections[1].0, "Grillade");
        assert_eq!(d[0].meals[0].plat_du_jour().map(|v| v.len()), Some(2));
        assert_eq!(d[0].meals[0].autres().map(|(c, _)| c.as_str()).collect::<Vec<_>>(), ["Grillade"]);
    }
}
