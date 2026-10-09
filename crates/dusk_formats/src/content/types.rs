//! Plain data types of maps, NPCs and class progression, shared by the loaders of
//! [`crate::content`], the server and the client.

#[derive(Debug, Clone)]
pub struct MapInfo {
    pub id: i64,
    /// File name of `maps/<name>.map`.
    pub name: String,
    /// Display name.
    pub title: String,
    /// Tracks (empty = the whole soundtrack).
    pub music: Vec<String>,
    pub ambience: String,
    /// Cells; the `arrival` marker of our maps ((0, 0) = none).
    pub start: (f32, f32),
    /// New characters start on this map (at `start`) unless told otherwise.
    pub default: bool,
    /// 0 = full light .. 1 = black.
    pub darkness: f32,
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
    /// Suffix of `portrait_<name>.png` (empty = none).
    pub portrait: String,
}

/// One of an NPC template's `spellN=` entries (N = 1..=4).
#[derive(Debug, Clone, Copy)]
pub struct NpcSpell {
    pub spell: i64,
    /// Percent chance to cast when the interval elapses.
    pub chance: i64,
    pub interval_ms: i64,
    pub cooldown_ms: i64,
    pub target_type: i64,
}

/// NPC template `faction` (players are [`faction::PLAYER_DEFAULT`]).
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
