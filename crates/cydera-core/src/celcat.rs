//! Client Celcat : port Rust du scraping du script Obsidian (example.md).

use crate::config::Config;
use crate::model::{Cours, Kind, short_room};
use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("réseau : {0}")]
    Network(#[from] reqwest::Error),
    #[error("jeton de sécurité introuvable sur la page de connexion")]
    NoToken,
    #[error("échec d'identification (code {0}) — vérifie CELCAT_PASSWORD")]
    BadCredentials(u16),
    #[error("réponse Celcat invalide : {0}")]
    Api(String),
}

pub async fn fetch(cfg: &Config, password: &str, from: NaiveDate, to: NaiveDate) -> Result<Vec<Cours>, Error> {
    let client = crate::net::client_builder()
        .cookie_store(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("Mozilla/5.0 (X11; Linux x86_64) cydera")
        .build()?;
    let base = cfg.base_url.trim_end_matches('/');

    // 1. Page de login → jeton anti-CSRF
    let page = client.get(format!("{base}/LdapLogin")).send().await?.text().await?;
    let token = extract_token(&page).ok_or(Error::NoToken)?;

    // 2. Authentification (302 attendu)
    let res = client
        .post(format!("{base}/LdapLogin/Logon"))
        .form(&[("Name", cfg.username.as_str()), ("Password", password), ("__RequestVerificationToken", token.as_str())])
        .send()
        .await?;
    if res.status().as_u16() != 302 {
        return Err(Error::BadCredentials(res.status().as_u16()));
    }

    // 3. Données du calendrier
    let start = from.format("%Y-%m-%d").to_string();
    let end = to.format("%Y-%m-%d").to_string();
    let text = client
        .post(format!("{base}/Home/GetCalendarData"))
        .header("X-Requested-With", "XMLHttpRequest")
        .form(&[
            ("start", start.as_str()),
            ("end", end.as_str()),
            ("resType", "104"),
            ("calView", "month"),
            ("federationIds[]", cfg.federation_id.as_str()),
            ("colourScheme", "3"),
        ])
        .send()
        .await?
        .text()
        .await?;
    if text.trim().is_empty() {
        return Err(Error::Api("requête rejetée (réponse vide)".into()));
    }
    let raw: Value = serde_json::from_str(&text).map_err(|e| Error::Api(e.to_string()))?;
    let events = raw.as_array().ok_or_else(|| Error::Api("tableau attendu".into()))?;
    let mut cours: Vec<Cours> = events.iter().filter_map(parse_event).collect();
    cours.sort_by_key(|c| c.start);
    Ok(cours)
}

fn extract_token(html: &str) -> Option<String> {
    let idx = html.find("__RequestVerificationToken")?;
    let tag_start = html[..idx].rfind('<')?;
    let tag_end = idx + html[idx..].find('>')?;
    let tag = &html[tag_start..tag_end];
    let v = tag.find("value=\"")? + 7;
    let len = tag[v..].find('"')?;
    Some(tag[v..v + len].to_string())
}

fn parse_dt(s: &str) -> Option<NaiveDateTime> {
    let s = s.trim_end_matches('Z');
    NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").or_else(|_| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M")).ok()
}

pub(crate) fn parse_event(evt: &Value) -> Option<Cours> {
    if evt.get("allDay").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let start = parse_dt(evt.get("start")?.as_str()?)?;
    let end = parse_dt(evt.get("end")?.as_str()?)?;
    let category = evt.get("eventCategory").and_then(Value::as_str).unwrap_or("CM").trim().to_string();
    let desc = evt.get("description").and_then(Value::as_str).unwrap_or("");
    let parts = split_description(desc);
    let get = |i: usize| parts.get(i).cloned().filter(|s| !s.is_empty());

    let kind = Kind::from_category(&category);
    let (matiere, salle, prof) = if kind == Kind::Indispo {
        ("Indisponibilité / Événement".to_string(), get(2).unwrap_or_else(|| "Non spécifiée".into()), get(3).unwrap_or_default())
    } else {
        (
            get(2).unwrap_or_else(|| "Cours".into()),
            get(3).unwrap_or_else(|| "Non spécifiée".into()),
            get(4).unwrap_or_else(|| "Non spécifié".into()),
        )
    };
    let salle = strip_capacity(&salle);
    Some(Cours { start, end, salle_courte: short_room(&salle), salle, matiere, category, kind, prof })
}

/// Découpe la description sur `\n\n<br />\n\n` et décode le HTML de chaque partie.
fn split_description(desc: &str) -> Vec<String> {
    let norm = desc.replace("\r\n", "\n");
    norm.split("\n\n<br />\n\n")
        .map(|p| {
            // Plusieurs enseignants / salles sont séparés par des retours à la ligne.
            decode_html(p.trim()).lines().map(str::trim).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(", ")
        })
        .collect()
}

/// Retire le suffixe de capacité « 30p ».
fn strip_capacity(s: &str) -> String {
    let s = s.trim();
    if let Some((head, tail)) = s.rsplit_once(char::is_whitespace)
        && tail.len() > 1
        && tail.ends_with('p')
        && tail[..tail.len() - 1].chars().all(|c| c.is_ascii_digit())
    {
        return head.trim_end().to_string();
    }
    s.to_string()
}

fn decode_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find(['&', '<']) {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        if rest.starts_with('<') {
            // Balise : <br> → saut de ligne, le reste est ignoré.
            match rest.find('>') {
                Some(j) => {
                    if rest[..j].to_ascii_lowercase().starts_with("<br") {
                        out.push('\n');
                    }
                    rest = &rest[j + 1..];
                }
                None => {
                    out.push_str(rest);
                    rest = "";
                }
            }
            continue;
        }
        let Some(j) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..j];
        let decoded = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[j + 1..];
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token() {
        let html = r#"<form><input name="__RequestVerificationToken" type="hidden" value="abc-123" /></form>"#;
        assert_eq!(extract_token(html).as_deref(), Some("abc-123"));
    }

    #[test]
    fn html() {
        assert_eq!(decode_html("R&#233;seaux avanc&eacute;s"), "Réseaux avanc&eacute;s");
        assert_eq!(decode_html("A &amp; B&#x27;s"), "A & B's");
    }

    #[test]
    fn capacity() {
        assert_eq!(strip_capacity("PAU E211 SALLE 30p"), "PAU E211 SALLE");
        assert_eq!(strip_capacity("E211"), "E211");
    }

    #[test]
    fn event_cm() {
        let e = json!({
            "start": "2026-11-17T08:00:00",
            "end": "2026-11-17T09:30:00",
            "allDay": false,
            "eventCategory": "CM",
            "description": "CM\r\n\r\n<br />\r\n\r\nING2\r\n\r\n<br />\r\n\r\nProgrammation\r\n\r\n<br />\r\n\r\nPAU E211 SALLE POLYVALENTE 40p\r\n\r\n<br />\r\n\r\nGARCIA THIERRY\r\n"
        });
        let c = parse_event(&e).unwrap();
        assert_eq!(c.matiere, "Programmation");
        assert_eq!(c.salle, "PAU E211 SALLE POLYVALENTE");
        assert_eq!(c.salle_courte, "E211");
        assert_eq!(c.prof, "GARCIA THIERRY");
        assert_eq!(c.kind, Kind::Cm);
        assert_eq!(c.label(), "CM - Programmation");
        assert_eq!(c.minutes(), 90);
    }

    #[test]
    fn event_indispo() {
        let e = json!({
            "start": "2026-11-18T12:00:00",
            "end": "2026-11-18T13:00:00",
            "eventCategory": "Indisponibilité",
            "description": "Indisponibilité\r\n\r\n<br />\r\n\r\nX\r\n\r\n<br />\r\n\r\nAmphi\r\n\r\n<br />\r\n\r\nRéunion"
        });
        let c = parse_event(&e).unwrap();
        assert_eq!(c.kind, Kind::Indispo);
        assert_eq!(c.salle, "Amphi");
        assert_eq!(c.prof, "Réunion");
    }
}
