//! The map list (ids, names, music, start points) and NPC spawns per map.

use crate::custom::{CUSTOM_MAP_PREFIX, parse_spawns};
use crate::db::{MapInfo, NpcSpawn};
use std::path::{Path, PathBuf};

/// Id of the first of our maps (`maps/custom_*.map`, sorted by name).
pub const CUSTOM_MAP_FIRST: i64 = 10_000;

/// Legacy maps, then ours (ids from [`CUSTOM_MAP_FIRST`]). The default map (where new
/// characters start without `--start-map`) has `default` set and its `start` at that spot.
pub fn load(root: &Path) -> anyhow::Result<Vec<MapInfo>> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "map, teleport_names");
    let mut maps = db.maps()?;
    if let Some(t) = db.teleports()?.into_iter().find(|t| t.name == "start")
        && let Some(m) = maps.iter_mut().find(|m| m.id == t.map)
    {
        m.default = true;
        m.start = (t.x + 0.5, t.y + 0.5);
    }
    let mut own: Vec<String> = std::fs::read_dir(root.join("maps"))
        .map(|d| {
            d.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(|n| n.strip_suffix(".map")).map(String::from))
                .filter(|n| n.starts_with(CUSTOM_MAP_PREFIX))
                .collect()
        })
        .unwrap_or_default();
    own.sort();
    for (i, name) in own.into_iter().enumerate() {
        maps.push(MapInfo {
            id: CUSTOM_MAP_FIRST + i as i64,
            name,
            music: vec![],
            ambience: String::new(),
            start: (0.0, 0.0),
            default: false,
        });
    }
    Ok(maps)
}

/// NPC spawns of a map: ours from `maps/<name>.spawns`, legacy ones from the legacy data.
pub fn spawns(root: &Path, map: &MapInfo) -> Vec<NpcSpawn> {
    if map.name.starts_with(CUSTOM_MAP_PREFIX) {
        return std::fs::read_to_string(root.join("maps").join(format!("{}.spawns", map.name)))
            .map(|t| parse_spawns(&t, map.id, 1_000_000 + map.id * 1000))
            .unwrap_or_default();
    }
    crate::legacy_note("game.db", "npc");
    super::legacy_db().and_then(|db| Ok(db.npc_spawns(map.id)?)).unwrap_or_default()
}

pub fn default_map(maps: &[MapInfo]) -> Option<&MapInfo> {
    maps.iter().find(|m| m.default)
}

/// `maps/<name>.<ext>` (`map`, `cover`, `markers`, ...): ours first, else legacy.
pub fn map_file(root: &Path, name: &str, ext: &str) -> Option<PathBuf> {
    crate::find_file(root, &format!("maps/{name}.{ext}"))
}
