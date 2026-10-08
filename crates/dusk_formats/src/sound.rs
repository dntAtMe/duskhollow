//! Sound-related `game.db` tables (music/ambience playlists, NPC voice sets, looping
//! sounds next to map sprites) plus the sound files the original client names in code.
//!
//! See `docs/audio.md`.

use crate::{FileIndex, db::GameDb, map::MapFile, map::TERRAIN_CHUNK};
use rusqlite::{Row, types::ValueRef};
use std::collections::HashMap;

/// `zone_template` / `area_template` music + ambience (playlists, comma separated in the db).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RegionSound {
    pub id: i64,
    pub name: String,
    pub music: Vec<String>,
    pub ambience: Vec<String>,
}

/// `sprite_proximity_sound`: a looping sound audible within `radius` cells of a map sprite.
#[derive(Debug, Clone, PartialEq)]
pub struct ProximitySound {
    pub sound: String,
    pub radius: f32,
}

/// All sound tables of `game.db`.
#[derive(Debug, Clone, Default)]
pub struct SoundTables {
    pub zones: HashMap<i64, RegionSound>,
    pub areas: HashMap<i64, RegionSound>,
    /// `(npc_models.name lowercased, event)` -> candidate sounds (one is picked at random).
    /// Events in the data: `aggro`, `attack`, `damage`, `die`.
    pub npc: HashMap<(String, String), Vec<String>>,
    /// Lowercased sprite filename -> looping sound.
    pub proximity: HashMap<String, ProximitySound>,
}

impl SoundTables {
    /// Sounds for an NPC model event (empty if none).
    pub fn npc_sounds(&self, model: &str, event: &str) -> &[String] {
        self.npc.get(&(model.to_lowercase(), event.to_string())).map(Vec::as_slice).unwrap_or(&[])
    }
}

/// Hard-coded sound files referenced by the legacy client (strings + xrefs in Ghidra).
pub mod builtin {
    /// Melee hit by an NPC (or an unarmed player): one at random (`FUN_0054ce40`).
    pub const HIT_UNARMED: [&str; 5] = [
        "attack_hit_var01.wav",
        "attack_hit_var02.wav",
        "attack_hit_var03.wav",
        "attack_hit_var04.wav",
        "attack_hit_var05.wav",
    ];
    /// Melee hit by a player with a bladed weapon (`FUN_0054fe50`).
    pub const HIT_BLADE: [&str; 4] = [
        "attack_sword_normal_h.ogg",
        "attack_sword_normal_m.ogg",
        "attack_sword_normal_m2.ogg",
        "attack_sword_normal_s.ogg",
    ];
    /// Melee hit by a player with weapon_type 1/4/6 (blunt) (`FUN_0054fe50`).
    pub const HIT_BLUNT: [&str; 5] = [
        "attack_metal_hit_var01.wav",
        "attack_metal_hit_var02.wav",
        "attack_metal_hit_var03.wav",
        "attack_metal_hit_var04.wav",
        "attack_metal_hit_var05.wav",
    ];
    /// Hit result sounds (`FUN_00554030`): Miss/Evade, Dodge, Parry; Block picks one of `BLOCK`.
    pub const MISS: &str = "dodge_default.wav";
    pub const DODGE: &str = "swishverb4.ogg";
    pub const PARRY: &str = "attack_metal_case.ogg";
    pub const BLOCK: [&str; 3] = ["e3_attack_hardhit01.ogg", "e3_attack_hardhit02.ogg", "e3_attack_hardhit03.ogg"];
    /// The local player got hurt (`FUN_0054f9e0`): male / female voice.
    pub const PLAYER_HURT_MALE: &str = "vdamage3_mlb_4.ogg";
    pub const PLAYER_HURT_FEMALE: &str = "vdamage3_flc_1.ogg";
    pub const LEVEL_UP: &str = "alert_levelup_a.ogg";
    pub const BUTTON_CLICK: &str = "button_click_a.ogg";
    pub const TARGET_OPEN: &str = "window_target_open_a.ogg";
    pub const WINDOW_OPEN: &str = "window_open_a.ogg";
    pub const WINDOW_CLOSE: &str = "window_close_a.ogg";
    pub const LOGIN_MUSIC: &str = "login_creation.ogg";

    /// Every name above (for tests).
    pub fn all() -> Vec<&'static str> {
        let mut v: Vec<&str> = Vec::new();
        v.extend(HIT_UNARMED);
        v.extend(HIT_BLADE);
        v.extend(HIT_BLUNT);
        v.extend(BLOCK);
        v.extend([
            MISS,
            DODGE,
            PARRY,
            PLAYER_HURT_MALE,
            PLAYER_HURT_FEMALE,
            LEVEL_UP,
            BUTTON_CLICK,
            TARGET_OPEN,
            WINDOW_OPEN,
            WINDOW_CLOSE,
            LOGIN_MUSIC,
        ]);
        v
    }
}

/// Splits a comma-separated playlist, dropping blanks and non-filenames
/// (`zone_template` 66 has `' '`, some `spell_visual_kit.sound` are `0`).
pub fn split_playlist(s: &str) -> Vec<String> {
    s.split(',').map(str::trim).filter(|s| s.contains('.')).map(str::to_string).collect()
}

/// Resolves a sound name to its path under the assets root. The db names a few
/// `.mp3` tracks that the install only ships as `.ogg` (`zorkfouralchs.mp3` →
/// `zorkfouralchs_01.ogg`); those fall back to `<stem>.ogg`, then `<stem>_01.ogg`.
pub fn resolve_sound<'a>(index: &'a FileIndex, name: &str) -> Option<&'a str> {
    let name = name.trim();
    if !name.contains('.') {
        return None;
    }
    index.resolve(name).or_else(|| {
        let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
        index.resolve(&format!("{stem}.ogg")).or_else(|| index.resolve(&format!("{stem}_01.ogg")))
    })
}

/// Zone and area lookup for a map: both are keyed by 13x13-cell terrain chunk.
#[derive(Debug, Clone, Default)]
pub struct RegionGrid {
    /// Terrain chunks per side.
    pub width: u32,
    /// Chunk id -> `zone_template.id`.
    pub zones: HashMap<u32, u32>,
    /// Chunk id -> `area_template.id`.
    pub areas: HashMap<u32, u32>,
}

impl RegionGrid {
    pub fn new(map: &MapFile) -> Self {
        Self {
            width: map.terrain_width(),
            zones: map.zones.iter().copied().collect(),
            areas: map.areas.iter().map(|&(area, chunk)| (chunk, area)).collect(),
        }
    }

    fn chunk(&self, x: f32, y: f32) -> Option<u32> {
        if x < 0.0 || y < 0.0 || self.width == 0 {
            return None;
        }
        let (col, row) = (x as u32 / TERRAIN_CHUNK, y as u32 / TERRAIN_CHUNK);
        (col < self.width && row < self.width).then_some(row * self.width + col)
    }

    /// `(zone id, area id)` under a cell position; 0 = none.
    pub fn at(&self, x: f32, y: f32) -> (u32, u32) {
        let Some(c) = self.chunk(x, y) else { return (0, 0) };
        (self.zones.get(&c).copied().unwrap_or(0), self.areas.get(&c).copied().unwrap_or(0))
    }
}

fn int(row: &Row, col: &str) -> i64 {
    match row.get_ref(col) {
        Ok(ValueRef::Integer(i)) => i,
        Ok(ValueRef::Real(f)) => f as i64,
        Ok(ValueRef::Text(t)) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        _ => 0,
    }
}

fn real(row: &Row, col: &str) -> f32 {
    match row.get_ref(col) {
        Ok(ValueRef::Integer(i)) => i as f32,
        Ok(ValueRef::Real(f)) => f as f32,
        Ok(ValueRef::Text(t)) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0),
        _ => 0.0,
    }
}

fn text(row: &Row, col: &str) -> String {
    match row.get_ref(col) {
        Ok(ValueRef::Text(t)) => String::from_utf8_lossy(t).trim().to_string(),
        _ => String::new(),
    }
}

impl GameDb {
    fn rows<T>(&self, sql: &str, f: impl Fn(&Row) -> T) -> rusqlite::Result<Vec<T>> {
        let mut stmt = self.conn().prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok(f(r)))?;
        rows.collect()
    }

    pub fn sound_tables(&self) -> rusqlite::Result<SoundTables> {
        let region = |r: &Row| RegionSound {
            id: int(r, "id"),
            name: text(r, "name"),
            music: split_playlist(&text(r, "music")),
            ambience: split_playlist(&text(r, "ambience")),
        };
        let mut npc: HashMap<(String, String), Vec<String>> = HashMap::new();
        for (model, event, sound) in
            self.rows("SELECT * FROM npc_sounds", |r| (text(r, "model"), text(r, "event"), text(r, "sound")))?
        {
            if !sound.is_empty() {
                npc.entry((model.to_lowercase(), event)).or_default().push(sound);
            }
        }
        Ok(SoundTables {
            zones: self.rows("SELECT * FROM zone_template", region)?.into_iter().map(|z| (z.id, z)).collect(),
            areas: self.rows("SELECT * FROM area_template", region)?.into_iter().map(|a| (a.id, a)).collect(),
            npc,
            proximity: self
                .rows("SELECT * FROM sprite_proximity_sound", |r| {
                    (
                        text(r, "filename").to_lowercase(),
                        ProximitySound { sound: text(r, "sound"), radius: real(r, "radius") },
                    )
                })?
                .into_iter()
                .collect(),
        })
    }

    /// Every sound file named anywhere in the db, as `(table.column, file)`, deduplicated.
    pub fn referenced_sounds(&self) -> rusqlite::Result<Vec<(String, String)>> {
        let sources = [
            ("map", "music"),
            ("map", "ambience"),
            ("zone_template", "music"),
            ("zone_template", "ambience"),
            ("area_template", "music"),
            ("area_template", "ambience"),
            ("spell_visual_kit", "sound"),
            ("npc_sounds", "sound"),
            ("sprite_proximity_sound", "sound"),
            ("item_template", "icon_sound"),
            ("item_dictionary", "sound"),
            ("gameobject_models", "sound_use"),
            ("gameobject_models", "sound_unlocked"),
        ];
        let mut out = Vec::new();
        for (table, col) in sources {
            let mut names = self.rows(&format!("SELECT DISTINCT \"{col}\" AS v FROM \"{table}\""), |r| text(r, "v"))?;
            names = names.iter().flat_map(|n| split_playlist(n)).collect();
            names.sort();
            names.dedup();
            out.extend(names.into_iter().map(|n| (format!("{table}.{col}"), n)));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playlists_and_regions() {
        assert_eq!(split_playlist("a.ogg, b.ogg,, "), ["a.ogg", "b.ogg"]);
        assert!(split_playlist(" ").is_empty());
        assert!(split_playlist("0").is_empty());
        let map = MapFile {
            size: 26,
            textures: vec![],
            cells: vec![],
            terrain_textures: vec![],
            terrain: vec![],
            zones: vec![(0, 5), (3, 7)],
            areas: vec![(9, 3)],
        };
        let g = RegionGrid::new(&map);
        assert_eq!(g.at(1.0, 1.0), (5, 0));
        assert_eq!(g.at(14.0, 20.0), (7, 9));
        assert_eq!(g.at(12.9, 14.0), (0, 0));
        assert_eq!(g.at(-1.0, 3.0), (0, 0));
        assert_eq!(g.at(30.0, 3.0), (0, 0));
    }
}
