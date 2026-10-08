//! Our own art and maps from `tools/artgen` (`custom_assets/`), installed next to the
//! extracted original assets so the engine can load both from one root.

use crate::db::{NpcModel, NpcSpawn, NpcSpell, NpcTemplate};
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

/// First `npc_template.entry` (and model id) used by `custom_assets/data/npc_templates.txt`.
pub const CUSTOM_NPC_FIRST: i64 = 50000;

/// Parses `custom_assets/data/npc_templates.txt`: `[entry]` sections of `key=value` lines, `#`
/// comments. Keys are [`NpcTemplate`] field names plus `model` (sprite script name under
/// `scripts/npc/custom/`), `height` (pixels, nameplate offset), `level=MIN[-MAX]`,
/// `resist=frost,fire,shadow,holy` and `spellN=spell,chance,interval_ms,cooldown_ms,target_type`
/// (N = 1..=4). Each template gets its own model with `id = entry`. Unknown keys are ignored.
pub fn parse_npc_templates(text: &str) -> Vec<(NpcTemplate, NpcModel)> {
    let mut out = Vec::new();
    let mut cur: Option<(NpcTemplate, NpcModel)> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if let Some(entry) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.extend(cur.take());
            let Ok(entry) = entry.trim().parse::<i64>() else { continue };
            cur = Some((default_template(entry), NpcModel { id: entry, name: String::new(), height: 0 }));
            continue;
        }
        let (Some((t, m)), Some((k, v))) = (cur.as_mut(), line.split_once('=')) else { continue };
        let (k, v) = (k.trim(), v.trim());
        let n = || v.parse::<i64>().unwrap_or(0);
        match k {
            "name" => t.name = v.into(),
            "subname" => t.subname = v.into(),
            "model" => m.name = v.into(),
            "height" => m.height = n(),
            "level" => {
                let (lo, hi) = v.split_once('-').unwrap_or((v, v));
                t.min_level = lo.trim().parse().unwrap_or(1);
                t.max_level = hi.trim().parse().unwrap_or(t.min_level);
            }
            "faction" => t.faction = n(),
            "model_scale" => t.model_scale = n(),
            "health" => t.health = n(),
            "mana" => t.mana = n(),
            "weapon_value" => t.weapon_value = n(),
            "armor" => t.armor = n(),
            "melee_speed_ms" => t.melee_speed_ms = n(),
            "leash_range" => t.leash_range = n(),
            "ai_type" => t.ai_type = n(),
            "npc_flags" => t.npc_flags = n(),
            "strength" => t.strength = n(),
            "agility" => t.agility = n(),
            "intellect" => t.intellect = n(),
            "willpower" => t.willpower = n(),
            "courage" => t.courage = n(),
            "resist" => {
                for (r, x) in t.resist.iter_mut().zip(v.split(',')) {
                    *r = x.trim().parse().unwrap_or(0);
                }
            }
            "elite" => t.elite = n() != 0,
            "boss" => t.boss = n() != 0,
            "portrait" => t.portrait = v.into(),
            _ => {
                let Some(i) = k.strip_prefix("spell").and_then(|i| i.parse::<usize>().ok()) else { continue };
                let f: Vec<i64> = v.split(',').map(|x| x.trim().parse().unwrap_or(0)).collect();
                if (1..=4).contains(&i) && f.len() == 5 {
                    t.spells[i - 1] =
                        NpcSpell { spell: f[0], chance: f[1], interval_ms: f[2], cooldown_ms: f[3], target_type: f[4] };
                }
            }
        }
    }
    out.extend(cur);
    out
}

/// Loads `<custom_root>/data/npc_templates.txt` (empty if missing).
pub fn load_npc_templates(custom_root: &Path) -> Vec<(NpcTemplate, NpcModel)> {
    std::fs::read_to_string(custom_root.join("data/npc_templates.txt"))
        .map(|t| parse_npc_templates(&t))
        .unwrap_or_default()
}

fn default_template(entry: i64) -> NpcTemplate {
    NpcTemplate {
        entry,
        name: String::new(),
        subname: String::new(),
        model_id: entry,
        min_level: 1,
        max_level: 1,
        faction: crate::db::faction::HOSTILE,
        model_scale: 100,
        health: -1,
        mana: 0,
        weapon_value: -1,
        armor: 0,
        melee_speed_ms: 2000,
        leash_range: 0,
        ai_type: 0,
        npc_flags: 0,
        strength: 0,
        agility: 0,
        intellect: 0,
        willpower: 0,
        courage: 0,
        resist: [0; 4],
        spells: vec![NpcSpell { spell: 0, chance: 0, interval_ms: 0, cooldown_ms: 0, target_type: 0 }; 4],
        elite: false,
        boss: false,
        portrait: String::new(),
    }
}

/// A named point of a custom map (`maps/<name>.markers`: `name x y [radius]` per line, cells).
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
}

pub fn parse_markers(text: &str) -> Vec<Marker> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?.to_string();
            let v: Vec<f32> = it.map(|s| s.parse().ok()).collect::<Option<_>>()?;
            (v.len() >= 2).then(|| Marker { name, x: v[0], y: v[1], radius: v.get(2).copied().unwrap_or(3.0) })
        })
        .collect()
}

/// Cover of one cell (`maps/<name>.cover`, docs/demo-plan.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    #[default]
    Open,
    Shade,
    Shelter,
    Cairn,
}

/// Per-cell cover grid: first line `W H`, then H rows of W chars (`.` open, `s` shade,
/// `S` deep shelter, `C` rest cairn). Cells outside the grid are open sky.
#[derive(Debug, Clone, Default)]
pub struct CoverGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Cover>,
}

impl CoverGrid {
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines().map(str::trim_end).filter(|l| !l.starts_with('#'));
        let mut dims = lines.next()?.split_whitespace().map(|s| s.parse::<usize>());
        let (width, height) = (dims.next()?.ok()?, dims.next()?.ok()?);
        let mut cells = vec![Cover::Open; width * height];
        for (y, row) in lines.take(height).enumerate() {
            for (x, c) in row.chars().take(width).enumerate() {
                cells[y * width + x] = match c {
                    's' => Cover::Shade,
                    'S' => Cover::Shelter,
                    'C' => Cover::Cairn,
                    _ => Cover::Open,
                };
            }
        }
        Some(Self { width, height, cells })
    }

    pub fn get(&self, x: i32, y: i32) -> Cover {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return Cover::Open;
        }
        self.cells[y as usize * self.width + x as usize]
    }

    /// Cell centres of every `C`.
    pub fn cairns(&self) -> Vec<(f32, f32)> {
        (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cover::Cairn)
            .map(|i| ((i % self.width) as f32 + 0.5, (i / self.width) as f32 + 0.5))
            .collect()
    }
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

    #[test]
    fn npc_templates_file() {
        let t = parse_npc_templates(
            "# c\n[50001]\nname=Glarewolf\nmodel=glarewolf\nheight=40\nlevel=2-3\nboss=1\nspell2=12,50,4000,8000,14\n\n[50010]\nname=Ysolde\nfaction=1\n",
        );
        assert_eq!(t.len(), 2);
        let (w, m) = &t[0];
        assert_eq!((w.entry, w.model_id, w.min_level, w.max_level, w.boss), (50001, 50001, 2, 3, true));
        assert_eq!((m.name.as_str(), m.height, w.spells[1].spell, w.spells[1].target_type), ("glarewolf", 40, 12, 14));
        assert_eq!(t[1].0.faction, 1);
    }

    #[test]
    fn cover_and_markers() {
        let g = CoverGrid::parse("3 2\n.sS\nC..\n").unwrap();
        assert_eq!(
            (g.get(1, 0), g.get(2, 0), g.get(0, 1), g.get(5, 5)),
            (Cover::Shade, Cover::Shelter, Cover::Cairn, Cover::Open)
        );
        assert_eq!(g.cairns(), vec![(0.5, 1.5)]);
        let m = parse_markers("# x\nglare_gate 30 8 6\nspawn 31.5 60\n");
        assert_eq!(m[0], Marker { name: "glare_gate".into(), x: 30.0, y: 8.0, radius: 6.0 });
        assert_eq!(m[1].radius, 3.0);
    }
}
