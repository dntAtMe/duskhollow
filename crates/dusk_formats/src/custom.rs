//! Our own art and maps from `tools/artgen` (`custom_assets/`), installed next to the
//! extracted original assets so the engine can load both from one root.

use crate::db::NpcSpawn;
use std::path::Path;

/// Prefix of map files that come from `custom_assets/maps` rather than the original game.
pub const CUSTOM_MAP_PREFIX: &str = "custom_";

/// Mirrors `custom_assets/{content,scripts,maps}` into the same folders under `assets_root`
/// (copying only files that are newer) and returns `(basename, relative path)` for every
/// `content/` file so callers can add them to their [`crate::FileIndex`].
/// Only our own names are written (`content/custom/...`, `scripts/player/custom/...`,
/// `maps/custom_*`), never original files.
pub fn install(custom_root: &Path, assets_root: &Path) -> std::io::Result<Vec<(String, String)>> {
    let mut content = Vec::new();
    if !custom_root.exists() {
        return Ok(content);
    }
    let mut stack = vec![custom_root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in std::fs::read_dir(&dir)?.flatten() {
            let path = e.path();
            let Ok(rel) = path.strip_prefix(custom_root) else { continue };
            let top = rel.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned());
            if !matches!(top.as_deref(), Some("content" | "scripts" | "maps")) {
                continue; // previews, manifests...
            }
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let dest = assets_root.join(rel);
            let stale = std::fs::metadata(&dest)
                .and_then(|d| Ok(d.modified()? < std::fs::metadata(&path)?.modified()?))
                .unwrap_or(true);
            if stale {
                std::fs::create_dir_all(dest.parent().unwrap())?;
                std::fs::copy(&path, &dest)?;
            }
            if top.as_deref() == Some("content") {
                content.push((e.file_name().to_string_lossy().into_owned(), rel.to_string_lossy().replace('\\', "/")));
            }
        }
    }
    Ok(content)
}

/// Parses a `maps/<name>.spawns` sidecar: `entry x y orientation wander_distance` per line,
/// `#` comments. Spawns get synthetic guids from `first_guid` and wander if the distance is > 0.
pub fn parse_spawns(text: &str, map: i64, first_guid: i64) -> Vec<NpcSpawn> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .enumerate()
        .filter_map(|(i, l)| {
            let v: Vec<f32> = l.split_whitespace().map(|s| s.parse().ok()).collect::<Option<_>>()?;
            (v.len() >= 3).then(|| NpcSpawn {
                guid: first_guid + i as i64,
                entry: v[0] as i64,
                map,
                x: v[1],
                y: v[2],
                orientation: v.get(3).copied().unwrap_or(0.0),
                respawn_time: 60,
                movement_type: if v.get(4).copied().unwrap_or(0.0) > 0.0 { 1 } else { 0 },
                wander_distance: v.get(4).copied().unwrap_or(0.0) as i64,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_sidecar() {
        let s = parse_spawns("# comment\n2 10.5 12 1.5 4\n\n13 3 4\nbad line\n", 10000, 1);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].entry, s[0].x, s[0].wander_distance, s[0].movement_type), (2, 10.5, 4, 1));
        assert_eq!((s[1].entry, s[1].movement_type, s[1].guid), (13, 0, 2));
    }
}
