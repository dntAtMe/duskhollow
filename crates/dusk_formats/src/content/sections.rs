//! The generic `[id]` / `[kind id]` + `key=value` text format of our data files.
//!
//! ```text
//! # comment
//! [50001]
//! name=Glarewolf
//! spell=12          # repeated keys are kept in order (see `Section::all`)
//! [spell 50002]
//! impact=bleed_hit
//! ```
//! Blank lines and lines starting with `#` are ignored. A key outside a section and a repeated
//! `(kind, id)` are errors.

use anyhow::{Context, bail};
use std::collections::HashSet;
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Section {
    /// `spell` in `[spell 50002]`; `None` for a plain `[50001]`.
    pub kind: Option<String>,
    pub id: String,
    /// 1-based line of the header.
    pub line: usize,
    /// `key=value` pairs in file order (keys and values trimmed).
    pub entries: Vec<(String, String)>,
}

impl Section {
    /// Last value of `k`.
    pub fn get(&self, k: &str) -> Option<&str> {
        self.entries.iter().rev().find(|(key, _)| key == k).map(|(_, v)| v.as_str())
    }

    /// Every value of a repeated key, in order.
    pub fn all<'a>(&'a self, k: &'a str) -> impl Iterator<Item = &'a str> {
        self.entries.iter().filter(move |(key, _)| key == k).map(|(_, v)| v.as_str())
    }

    pub fn int(&self, k: &str, default: i64) -> i64 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    pub fn num(&self, k: &str, default: f32) -> f32 {
        self.get(k).and_then(|v| v.parse().ok()).unwrap_or(default)
    }

    /// Comma-separated value of `k`, items trimmed, empty items dropped.
    pub fn list(&self, k: &str) -> Vec<&str> {
        self.get(k).map(|v| v.split(',').map(str::trim).filter(|s| !s.is_empty()).collect()).unwrap_or_default()
    }

    /// `a-b`, `a,b` or a single `a` (= `a-a`).
    pub fn range(&self, k: &str) -> Option<(f32, f32)> {
        let v = self.get(k)?;
        // A leading '-' is a sign, not the separator.
        let sep = v.char_indices().skip(1).find(|&(_, c)| c == '-' || c == ',').map(|(i, _)| i);
        match sep {
            Some(i) => Some((v[..i].trim().parse().ok()?, v[i + 1..].trim().parse().ok()?)),
            None => {
                let x = v.trim().parse().ok()?;
                Some((x, x))
            }
        }
    }

    /// The id as a number (`[50001]`).
    pub fn id_int(&self) -> Option<i64> {
        self.id.parse().ok()
    }
}

pub fn parse(text: &str) -> anyhow::Result<Vec<Section>> {
    let mut out: Vec<Section> = Vec::new();
    let mut seen = HashSet::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        let n = i + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(head) = line.strip_prefix('[') {
            let Some(head) = head.strip_suffix(']') else { bail!("line {n}: unterminated section header {line:?}") };
            let words: Vec<&str> = head.split_whitespace().collect();
            let (kind, id) = match words[..] {
                [id] => (None, id.to_string()),
                [kind, id] => (Some(kind.to_string()), id.to_string()),
                _ => bail!("line {n}: bad section header {line:?}"),
            };
            if !seen.insert((kind.clone(), id.clone())) {
                bail!("line {n}: duplicate section {line}");
            }
            out.push(Section { kind, id, line: n, entries: Vec::new() });
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { bail!("line {n}: expected key=value, got {line:?}") };
        let Some(section) = out.last_mut() else { bail!("line {n}: key {:?} outside a section", k.trim()) };
        section.entries.push((k.trim().to_string(), v.trim().to_string()));
    }
    Ok(out)
}

pub fn load(path: &Path) -> anyhow::Result<Vec<Section>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    parse(&text).with_context(|| path.display().to_string())
}

/// `"<section>: <key>"` for every key not in `known` (tests assert shipped files have none).
pub fn unknown_keys(sections: &[Section], known: &[&str]) -> Vec<String> {
    sections
        .iter()
        .flat_map(|s| {
            s.entries.iter().filter(|(k, _)| !known.contains(&k.as_str())).map(move |(k, _)| {
                let head = s.kind.as_ref().map_or(s.id.clone(), |kind| format!("{kind} {}", s.id));
                format!("[{head}]: {k}")
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections() {
        let s =
            parse("# c\n[50001]\nname = Wolf \nspell=1\nspell=2\nlife=0.5,1.5\n\n[spell 7]\nx=-3\nr=-2-4\n").unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].kind.as_deref(), s[0].id.as_str(), s[0].line), (None, "50001", 2));
        assert_eq!(s[0].get("name"), Some("Wolf"));
        assert_eq!(s[0].all("spell").collect::<Vec<_>>(), ["1", "2"]);
        assert_eq!(s[0].range("life"), Some((0.5, 1.5)));
        assert_eq!(s[0].list("life"), ["0.5", "1.5"]);
        assert_eq!((s[0].int("missing", 4), s[0].id_int()), (4, Some(50001)));
        assert_eq!((s[1].kind.as_deref(), s[1].int("x", 0), s[1].num("x", 0.0)), (Some("spell"), -3, -3.0));
        assert_eq!(s[1].range("r"), Some((-2.0, 4.0)));
        assert_eq!(unknown_keys(&s, &["name", "spell", "life", "x"]), ["[spell 7]: r"]);
    }

    #[test]
    fn rejects_bad_files() {
        assert!(parse("name=x\n").is_err());
        assert!(parse("[1]\n[1]\n").is_err());
        assert!(parse("[a b c]\n").is_err());
        assert!(parse("[1]\nno equals\n").is_err());
        assert!(parse("[1]\n[kind 1]\n").is_ok());
    }
}
