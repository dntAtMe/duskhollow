//! Typed access to `game.db` (SQLite). Columns are dynamically typed in the
//! original data (`''` appears in int columns), so readers here are lenient.

use rusqlite::{Connection, OpenFlags, Row, types::ValueRef};
use std::collections::HashMap;
use std::path::Path;

pub struct GameDb {
    conn: Connection,
}

pub(crate) fn int(row: &Row, col: &str) -> i64 {
    match row.get_ref(col) {
        Ok(ValueRef::Integer(i)) => i,
        Ok(ValueRef::Real(f)) => f as i64,
        Ok(ValueRef::Text(t)) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        _ => 0,
    }
}

pub(crate) fn real(row: &Row, col: &str) -> f32 {
    match row.get_ref(col) {
        Ok(ValueRef::Integer(i)) => i as f32,
        Ok(ValueRef::Real(f)) => f as f32,
        Ok(ValueRef::Text(t)) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0.0),
        _ => 0.0,
    }
}

pub(crate) fn text(row: &Row, col: &str) -> String {
    match row.get_ref(col) {
        Ok(ValueRef::Text(t)) => String::from_utf8_lossy(t).into_owned(),
        Ok(ValueRef::Integer(i)) => i.to_string(),
        _ => String::new(),
    }
}

#[derive(Debug, Clone)]
pub struct MapInfo {
    pub id: i64,
    pub name: String,
    /// Comma-separated list of tracks in the original.
    pub music: Vec<String>,
    pub ambience: String,
    pub start: (f32, f32),
}

#[derive(Debug, Clone)]
pub struct NpcModel {
    pub id: i64,
    /// Matches `scripts/npc/<name>.txt`.
    pub name: String,
    pub height: i64,
}

#[derive(Debug, Clone)]
pub struct NpcTemplate {
    pub entry: i64,
    pub name: String,
    pub subname: String,
    pub model_id: i64,
    pub min_level: i64,
    pub max_level: i64,
    pub faction: i64,
    /// Percent; 0 in data means default (100).
    pub model_scale: i64,
    /// -1 = derive from level.
    pub health: i64,
    pub mana: i64,
    /// -1 = derive from level.
    pub weapon_value: i64,
    pub armor: i64,
    pub melee_speed_ms: i64,
    /// 0 = use the server default.
    pub leash_range: i64,
    /// 0 = melee, 1 = caster, 2 = archer.
    pub ai_type: i64,
    pub npc_flags: i64,
    pub strength: i64,
    pub agility: i64,
    pub intellect: i64,
    pub willpower: i64,
    pub courage: i64,
    /// Resistance ratings: frost, fire, shadow, holy.
    pub resist: [i64; 4],
    pub spells: Vec<NpcSpell>,
    pub elite: bool,
    pub boss: bool,
    /// `npc_template.portrait`: suffix of `portrait_<name>.png` (empty = none).
    pub portrait: String,
}

/// One of `npc_template.spell_N_*` (N = 1..=4).
#[derive(Debug, Clone, Copy)]
pub struct NpcSpell {
    pub spell: i64,
    /// Percent chance to cast when the interval elapses.
    pub chance: i64,
    pub interval_ms: i64,
    pub cooldown_ms: i64,
    pub target_type: i64,
}

/// `npc_template.faction`, names from the client's DB editor enum table.
pub mod faction {
    pub const PLAYER_DEFAULT: i64 = 0;
    pub const FRIENDLY: i64 = 1;
    pub const NEUTRAL: i64 = 2;
    pub const HOSTILE: i64 = 3;
}

#[derive(Debug, Clone, Copy)]
pub struct ClassStats {
    pub class: i64,
    pub level: i64,
    pub hp: i64,
    pub mana: i64,
    pub strength: i64,
    pub agility: i64,
    pub willpower: i64,
    pub intelligence: i64,
    pub courage: i64,
}

#[derive(Debug, Clone)]
pub struct ExpLevel {
    pub level: i64,
    /// Experience needed to advance from this level.
    pub exp: i64,
    /// Base experience for killing an enemy of this level.
    pub kill_exp: i64,
    /// Rank title shown for the level.
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct NpcSpawn {
    pub guid: i64,
    pub entry: i64,
    pub map: i64,
    /// Cell coordinates (fractional).
    pub x: f32,
    pub y: f32,
    /// Radians.
    pub orientation: f32,
    pub respawn_time: i64,
    pub movement_type: i64,
    pub wander_distance: i64,
}

#[derive(Debug, Clone)]
pub struct Teleport {
    pub name: String,
    pub map: i64,
    pub x: f32,
    pub y: f32,
}

impl GameDb {
    pub fn open(path: impl AsRef<Path>) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        Ok(Self { conn })
    }

    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    pub fn maps(&self) -> rusqlite::Result<Vec<MapInfo>> {
        self.query("SELECT * FROM map", |r| MapInfo {
            id: int(r, "id"),
            name: text(r, "name"),
            music: text(r, "music").split(',').filter(|s| !s.is_empty()).map(str::to_string).collect(),
            ambience: text(r, "ambience"),
            start: (real(r, "start_x"), real(r, "start_y")),
        })
    }

    pub fn npc_models(&self) -> rusqlite::Result<HashMap<i64, NpcModel>> {
        let v = self.query("SELECT * FROM npc_models", |r| NpcModel {
            id: int(r, "id"),
            name: text(r, "name"),
            height: int(r, "height"),
        })?;
        Ok(v.into_iter().map(|m| (m.id, m)).collect())
    }

    pub fn npc_templates(&self) -> rusqlite::Result<HashMap<i64, NpcTemplate>> {
        let v = self.query("SELECT * FROM npc_template", |r| NpcTemplate {
            entry: int(r, "entry"),
            name: text(r, "name"),
            subname: text(r, "subname"),
            model_id: int(r, "model_id"),
            min_level: int(r, "min_level"),
            max_level: int(r, "max_level"),
            faction: int(r, "faction"),
            model_scale: int(r, "model_scale"),
            health: int(r, "health"),
            mana: int(r, "mana"),
            weapon_value: int(r, "weapon_value"),
            armor: int(r, "armor"),
            melee_speed_ms: int(r, "melee_speed"),
            leash_range: int(r, "leash_range"),
            ai_type: int(r, "ai_type"),
            npc_flags: int(r, "npc_flags"),
            strength: int(r, "strength"),
            agility: int(r, "agility"),
            intellect: int(r, "intellect"),
            willpower: int(r, "willpower"),
            courage: int(r, "courage"),
            resist: ["frost", "fire", "shadow", "holy"].map(|s| int(r, &format!("resistance_{s}"))),
            spells: (1..=4)
                .map(|i| NpcSpell {
                    spell: int(r, &format!("spell_{i}_id")),
                    chance: int(r, &format!("spell_{i}_chance")),
                    interval_ms: int(r, &format!("spell_{i}_interval")),
                    cooldown_ms: int(r, &format!("spell_{i}_cooldown")),
                    target_type: int(r, &format!("spell_{i}_targetType")),
                })
                .filter(|s| s.spell > 0)
                .collect(),
            elite: int(r, "bool_elite") != 0,
            boss: int(r, "bool_boss") != 0,
            portrait: text(r, "portrait"),
        })?;
        Ok(v.into_iter().map(|t| (t.entry, t)).collect())
    }

    pub fn npc_spawns(&self, map: i64) -> rusqlite::Result<Vec<NpcSpawn>> {
        let mut stmt = self.conn.prepare("SELECT * FROM npc WHERE map = ?1")?;
        let rows = stmt.query_map([map], |r| Ok(spawn(r)))?;
        rows.collect()
    }

    pub fn class_stats(&self) -> rusqlite::Result<Vec<ClassStats>> {
        self.query("SELECT * FROM player_class_stats", |r| ClassStats {
            class: int(r, "Class"),
            level: int(r, "Level"),
            hp: int(r, "HP"),
            mana: int(r, "Mana"),
            strength: int(r, "Strength"),
            agility: int(r, "Agility"),
            willpower: int(r, "Willpower"),
            intelligence: int(r, "Intelligence"),
            courage: int(r, "Courage"),
        })
    }

    pub fn exp_levels(&self) -> rusqlite::Result<Vec<ExpLevel>> {
        self.query("SELECT * FROM player_exp_levels ORDER BY level", |r| ExpLevel {
            level: int(r, "level"),
            exp: int(r, "exp"),
            kill_exp: int(r, "kill_exp"),
            name: text(r, "name"),
        })
    }

    /// `teleport_names`: named GM teleport targets (`start` is the new-character spot).
    pub fn teleports(&self) -> rusqlite::Result<Vec<Teleport>> {
        self.query("SELECT * FROM teleport_names", |r| Teleport {
            name: text(r, "name"),
            map: int(r, "target_mapId"),
            x: real(r, "target_x"),
            y: real(r, "target_y"),
        })
    }

    /// `sprite_hotspot`: filename -> pivot offset in pixels.
    pub fn sprite_hotspots(&self) -> rusqlite::Result<HashMap<String, (i32, i32)>> {
        let v = self.query("SELECT * FROM sprite_hotspot", |r| {
            (text(r, "filename").to_lowercase(), (int(r, "x_offset") as i32, int(r, "y_offset") as i32))
        })?;
        Ok(v.into_iter().collect())
    }

    fn query<T>(&self, sql: &str, f: impl Fn(&Row) -> T) -> rusqlite::Result<Vec<T>> {
        let mut stmt = self.conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| Ok(f(r)))?;
        rows.collect()
    }
}

fn spawn(r: &Row) -> NpcSpawn {
    NpcSpawn {
        guid: int(r, "guid"),
        entry: int(r, "entry"),
        map: int(r, "map"),
        x: real(r, "position_x"),
        y: real(r, "position_y"),
        orientation: real(r, "orientation"),
        respawn_time: int(r, "respawn_time"),
        movement_type: int(r, "movement_type"),
        wander_distance: int(r, "wander_distance"),
    }
}
