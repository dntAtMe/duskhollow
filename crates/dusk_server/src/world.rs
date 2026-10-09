//! World state: maps (collision), entity components, NPC spawning.

use crate::ai::Rng;
use crate::stats::{Stats, npc_stats};
use bevy::prelude::*;
use dusk_formats::{
    db::{ClassStats, ExpLevel, NpcTemplate},
    map::{MapFile, WalkGrid},
    spell::SpellTemplate,
};
use dusk_protocol::{EntityId, EntityInfo, EntityKind, Pos, ServerMsg};
use std::collections::HashMap;
use std::path::Path;

pub struct ServerMap {
    pub name: String,
    pub grid: WalkGrid,
}

#[derive(Resource)]
pub struct GameWorld {
    pub maps: HashMap<i64, ServerMap>,
    pub npc_templates: HashMap<i64, NpcTemplate>,
    /// (class, level) -> stats
    pub class_stats: HashMap<(i64, i64), ClassStats>,
    pub exp_levels: Vec<ExpLevel>,
    pub spells: HashMap<i64, SpellTemplate>,
    /// NPC spawns by map id (`dusk_formats::content::maps::spawns`).
    pub spawns: HashMap<i64, Vec<dusk_formats::db::NpcSpawn>>,
    /// class -> starting spells (`data/class_spells.txt`)
    pub class_spells: HashMap<i64, Vec<i64>>,
    /// New characters (and the dead) appear here.
    pub start: (i64, Vec2),
    next_id: EntityId,
}

impl GameWorld {
    /// `root`: our content root ([`dusk_formats::content_root`]). `start_map`: override the
    /// spawn map (offline play); default is the default map's start point.
    pub fn load(root: &Path, start_map: Option<&str>) -> anyhow::Result<Self> {
        use dusk_formats::content;
        let infos = content::maps::load(root)?;
        let mut maps = HashMap::new();
        let mut spawns = HashMap::new();
        for info in &infos {
            let Some(path) = content::maps::map_file(root, &info.name, "map") else {
                warn!("map {} unavailable: no {}.map", info.name, info.name);
                continue;
            };
            match MapFile::load(&path) {
                Ok(m) => {
                    maps.insert(info.id, ServerMap { name: info.name.clone(), grid: m.walk_grid() });
                    spawns.insert(info.id, content::maps::spawns(root, info));
                }
                Err(e) => warn!("map {} unavailable: {e}", info.name),
            }
        }
        // New characters (and the dead) appear at the start map's `arrival` marker (its
        // `MapInfo::start`), else in the middle of it; `start_map` overrides the default map.
        let info = match start_map {
            Some(name) => infos.iter().find(|i| i.name == name).ok_or_else(|| anyhow::anyhow!("unknown map {name}"))?,
            None => content::maps::default_map(&infos).ok_or_else(|| anyhow::anyhow!("no default map"))?,
        };
        let grid = &maps.get(&info.id).ok_or_else(|| anyhow::anyhow!("map {} failed to load", info.name))?.grid;
        let middle = (grid.size as f32 / 2.0, grid.size as f32 / 2.0);
        let want = if info.start != (0.0, 0.0) { info.start } else { middle };
        let (x, y) = grid.nearest_floor(want).unwrap_or(want);
        let start = (info.id, Vec2::new(x, y));
        let rules = content::rules::load(root)?;
        Ok(Self {
            npc_templates: content::npcs::load(root)?.templates,
            class_stats: rules.class_stats,
            exp_levels: rules.exp_levels,
            spells: content::spells::load(root)?,
            spawns,
            class_spells: rules.class_spells,
            maps,
            start,
            next_id: 1,
        })
    }

    pub fn alloc_id(&mut self) -> EntityId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn is_walkable(&self, map: i64, p: Vec2) -> bool {
        self.maps.get(&map).is_some_and(|m| m.grid.is_walkable(p.x, p.y))
    }

    pub fn class_stats(&self, class: i64, level: u32) -> Option<&ClassStats> {
        self.class_stats.get(&(class, level as i64))
    }

    pub fn max_level(&self) -> u32 {
        self.exp_levels.last().map(|l| l.level as u32).unwrap_or(1)
    }

    /// Experience required to advance from `level`.
    pub fn xp_to_next(&self, level: u32) -> u32 {
        self.exp_levels.iter().find(|l| l.level == level as i64).map(|l| l.exp.max(1) as u32).unwrap_or(u32::MAX)
    }

    pub fn kill_xp(&self, level: u32) -> u32 {
        self.exp_levels.iter().find(|l| l.level == level as i64).map(|l| l.kill_exp.max(0) as u32).unwrap_or(0)
    }
}

/// Server entity id -> ECS entity.
#[derive(Resource, Default)]
pub struct NetIndex(pub HashMap<EntityId, Entity>);

#[derive(Component, Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct NetId(pub EntityId);

#[derive(Component, Clone, Copy, PartialEq, Eq, Debug)]
pub struct OnMap(pub i64);

/// Replicated movement state. `dirty` = needs broadcasting this tick.
#[derive(Component, Debug)]
pub struct Motion {
    pub pos: Vec2,
    pub orientation: f32,
    pub moving: bool,
    pub dirty: bool,
}

#[derive(Component)]
pub struct Npc {
    pub entry: i64,
}

/// NPC template `faction` (see `dusk_formats::db::faction`); players use PLAYER_DEFAULT.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub struct Faction(pub i64);

/// Where an NPC spawned and returns to.
#[derive(Component)]
pub struct Home {
    pub pos: Vec2,
    pub orientation: f32,
    pub leash: f32,
    pub respawn_secs: f32,
}

/// Random wandering around home (`npc.movement_type = 1`).
#[derive(Component)]
pub struct Wander {
    pub radius: f32,
    pub target: Option<Vec2>,
    pub wait: f32,
}

#[derive(Component)]
pub struct Player {
    pub name: String,
    pub class: i64,
    pub xp: u32,
}

/// Dead; `timer` counts down to corpse removal (NPC) or revival (player).
#[derive(Component)]
pub struct Dead {
    pub timer: f32,
}

/// Corpse removed, waiting to respawn; invisible to clients.
#[derive(Component)]
pub struct Hidden {
    pub timer: f32,
}

pub fn to_pos(v: Vec2) -> Pos {
    Pos { x: v.x, y: v.y }
}

pub fn entity_info(
    id: &NetId,
    m: &Motion,
    s: &Stats,
    npc: Option<&Npc>,
    player: Option<&Player>,
    dead: bool,
) -> EntityInfo {
    let kind = match (npc, player) {
        (Some(n), _) => EntityKind::Npc { entry: n.entry },
        (_, Some(p)) => EntityKind::Player { name: p.name.clone() },
        _ => EntityKind::Player { name: String::new() },
    };
    EntityInfo {
        id: id.0,
        kind,
        pos: to_pos(m.pos),
        orientation: m.orientation,
        moving: m.moving,
        level: s.level,
        hp: s.hp,
        max_hp: s.max_hp,
        dead,
    }
}

/// Messages queued by game systems, delivered by `net::flush_outbox`.
#[derive(Resource, Default)]
pub struct Outbox(pub Vec<(Scope, ServerMsg)>);

#[derive(Clone, Copy)]
pub enum Scope {
    /// Everyone on the map (spawns, deaths: clients track all entities of their map).
    Map(i64),
    /// Players on the map within the interest radius of a point.
    Near(i64, Vec2),
    /// One player.
    To(Entity),
}

impl Outbox {
    pub fn push(&mut self, scope: Scope, msg: ServerMsg) {
        self.0.push((scope, msg));
    }
}

/// Default leash distance (cells) when a template's `leash_range` is unset. DESIGN.
pub const DEFAULT_LEASH: f32 = 20.0;

pub fn spawn_npcs(
    mut commands: Commands,
    mut world: ResMut<GameWorld>,
    mut index: ResMut<NetIndex>,
    mut rng: Local<Rng>,
) {
    let map_ids: Vec<i64> = world.maps.keys().copied().collect();
    let mut total = 0;
    for map in map_ids {
        let spawns = world.spawns.get(&map).cloned().unwrap_or_default();
        for s in spawns {
            let Some(t) = world.npc_templates.get(&s.entry).cloned() else { continue };
            let id = world.alloc_id();
            let lo = t.min_level.max(1) as u32;
            let hi = (t.max_level.max(1) as u32).max(lo);
            let level = (lo + (rng.next_f32() * (hi - lo + 1) as f32) as u32).min(hi);
            let home = Vec2::new(s.x, s.y);
            let leash = if t.leash_range > 0 { t.leash_range as f32 } else { DEFAULT_LEASH };
            let mut e = commands.spawn((
                NetId(id),
                OnMap(map),
                Npc { entry: s.entry },
                Faction(t.faction),
                npc_stats(&t, level),
                Motion { pos: home, orientation: s.orientation, moving: false, dirty: false },
                Home { pos: home, orientation: s.orientation, leash, respawn_secs: s.respawn_time.max(10) as f32 },
            ));
            if !t.spells.is_empty() {
                e.insert(crate::spells::NpcSpells(
                    t.spells
                        .iter()
                        .map(|sp| crate::spells::NpcSpellState {
                            spell: sp.spell as u32,
                            chance: sp.chance.max(1) as f32,
                            interval: (sp.interval_ms.max(500) as f32) / 1000.0,
                            cooldown: sp.cooldown_ms.max(0) as f32 / 1000.0,
                            timer: (sp.interval_ms.max(500) as f32) / 1000.0,
                            cd: 0.0,
                            // 1 = caster, 2 = friendly: cast on itself.
                            on_self: matches!(sp.target_type, 1 | 2),
                        })
                        .collect(),
                ));
                e.insert(crate::spells::Spellbook::new(t.spells.iter().map(|sp| sp.spell as u32).collect()));
            }
            if s.movement_type == 1 && s.wander_distance > 0 {
                e.insert(Wander { radius: s.wander_distance as f32, target: None, wait: 0.0 });
            }
            index.0.insert(id, e.id());
            total += 1;
        }
    }
    info!("spawned {total} npcs");
}
