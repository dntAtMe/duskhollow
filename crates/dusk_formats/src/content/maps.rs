//! The map list (`data/maps.txt`) and NPC spawns per map (`maps/<name>.spawns`).
//!
//! `[<map name>]` sections (the name of `maps/<name>.map`): `id`, `title`, `default=1` (where new
//! characters start without `--start-map`), `music=` (comma-separated tracks; empty = the whole
//! soundtrack), `ambience=`, `darkness=0..1` (how dark the map is, 0 = full light). A map's
//! start point is its `arrival` marker (`maps/<name>.markers`).

use super::sections::{self, Section};
use crate::custom::{parse_markers, parse_spawns};
use crate::db::{MapInfo, NpcSpawn};
use anyhow::{Context, bail};
use std::path::{Path, PathBuf};

/// Id of the first of our maps.
pub const CUSTOM_MAP_FIRST: i64 = 10_000;

pub const KEYS: &[&str] = &["id", "title", "default", "music", "ambience", "darkness"];

/// The marker new characters (and the dead) appear at.
pub const ARRIVAL: &str = "arrival";

pub fn parse_one(root: &Path, s: &Section) -> anyhow::Result<MapInfo> {
    if s.kind.is_some() {
        bail!("line {}: want [map_name]", s.line);
    }
    let id = s.get("id").with_context(|| format!("[{}] missing id", s.id))?;
    let darkness = s.num("darkness", 0.0);
    if !(0.0..=1.0).contains(&darkness) {
        bail!("[{}] darkness={darkness}: want 0..1", s.id);
    }
    let start = std::fs::read_to_string(root.join("maps").join(format!("{}.markers", s.id)))
        .ok()
        .and_then(|t| parse_markers(&t).into_iter().find(|m| m.name == ARRIVAL))
        .map_or((0.0, 0.0), |m| (m.x, m.y));
    Ok(MapInfo {
        id: id.parse().with_context(|| format!("[{}] id={id:?}", s.id))?,
        name: s.id.clone(),
        title: s.get("title").unwrap_or(&s.id).to_string(),
        music: s.list("music").into_iter().map(String::from).collect(),
        ambience: s.get("ambience").unwrap_or_default().to_string(),
        start,
        default: s.int("default", 0) != 0,
        darkness,
    })
}

/// Our maps, in file order. Exactly one is the default.
pub fn load(root: &Path) -> anyhow::Result<Vec<MapInfo>> {
    let maps = sections::load(&root.join("data/maps.txt"))?
        .iter()
        .map(|s| parse_one(root, s))
        .collect::<anyhow::Result<Vec<_>>>()?;
    if maps.iter().filter(|m| m.default).count() != 1 {
        bail!("data/maps.txt: exactly one map needs default=1");
    }
    Ok(maps)
}

/// NPC spawns of a map (`maps/<name>.spawns`; none if missing).
pub fn spawns(root: &Path, map: &MapInfo) -> Vec<NpcSpawn> {
    std::fs::read_to_string(root.join("maps").join(format!("{}.spawns", map.name)))
        .map(|t| parse_spawns(&t, map.id, 1_000_000 + map.id * 1000))
        .unwrap_or_default()
}

pub fn default_map(maps: &[MapInfo]) -> Option<&MapInfo> {
    maps.iter().find(|m| m.default)
}

/// `maps/<name>.<ext>` (`map`, `cover`, `markers`, `spawns`) of our content, if it exists.
pub fn map_file(root: &Path, name: &str, ext: &str) -> Option<PathBuf> {
    let p = root.join("maps").join(format!("{name}.{ext}"));
    p.exists().then_some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shipped_maps_are_valid() {
        let root = crate::content_root();
        let s = sections::load(&root.join("data/maps.txt")).unwrap();
        assert!(sections::unknown_keys(&s, KEYS).is_empty());
        let maps = load(&root).unwrap();
        let npcs = super::super::npcs::load(&root).unwrap();
        let d = default_map(&maps).unwrap();
        assert_eq!((d.name.as_str(), d.id), ("custom_duskhollow", CUSTOM_MAP_FIRST));
        assert!(d.start != (0.0, 0.0), "the default map needs an arrival marker");
        for m in &maps {
            assert!(map_file(&root, &m.name, "map").is_some(), "{}", m.name);
            for sp in spawns(&root, m) {
                assert!(npcs.templates.contains_key(&sp.entry), "{}: unknown npc {}", m.name, sp.entry);
            }
        }
        let mut ids: Vec<i64> = maps.iter().map(|m| m.id).collect();
        ids.dedup();
        assert_eq!(ids.len(), maps.len());
    }
}
