//! World state: maps (collision), entity components, NPC spawning from game.db.

use crate::ai::Rng;
use crate::stats::{Stats, npc_stats};
use bevy::prelude::*;
use dusk_formats::{
    db::{ClassStats, ExpLevel, GameDb, NpcTemplate},
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
    pub db: std::sync::Mutex<GameDb>,
    pub maps: HashMap<i64, ServerMap>,
    pub npc_templates: HashMap<i64, NpcTemplate>,
    /// (class, level) -> stats
    pub class_stats: HashMap<(i64, i64), ClassStats>,
    pub exp_levels: Vec<ExpLevel>,
    pub spells: HashMap<i64, SpellTemplate>,
    /// NPC spawns of custom maps (map id -> spawns), see `dusk_formats::custom::parse_spawns`.
    pub custom_spawns: HashMap<i64, Vec<dusk_formats::db::NpcSpawn>>,
    /// class -> starting spells (`player_create_spell`)
    pub class_spells: HashMap<i64, Vec<i64>>,
    /// New characters (and the dead) appear here.
    pub start: (i64, Vec2),
    next_id: EntityId,
}

impl GameWorld {
    /// `start_map`: override the spawn map (offline play); default is the
    /// original `teleport_names.name = 'start'` spot.
    pub fn load(root: &Path, start_map: Option<&str>) -> anyhow::Result<Self> {
        let db = GameDb::open(root.join("game.db"))?;
        if let Err(e) = dusk_formats::custom::install(&dusk_formats::custom_assets_root(), root) {
            warn!("custom assets not installed: {e}");
        }
        let mut infos = db.maps()?;
        // Our own maps (`maps/custom_*.map` from tools/artgen) get ids from 10000 and NPCs
        // from a `.spawns` sidecar instead of the `npc` table.
        let mut custom_spawns = HashMap::new();
        let mut custom: Vec<_> =
            std::fs::read_dir(root.join("maps")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        custom.retain(|p: &std::path::PathBuf| {
            p.extension().is_some_and(|e| e == "map")
                && p.file_stem()
                    .is_some_and(|s| s.to_string_lossy().starts_with(dusk_formats::custom::CUSTOM_MAP_PREFIX))
        });
        custom.sort();
        for (i, path) in custom.iter().enumerate() {
            let id = 10_000 + i as i64;
            let name = path.file_stem().unwrap().to_string_lossy().into_owned();
            let spawns = std::fs::read_to_string(path.with_extension("spawns"))
                .map(|t| dusk_formats::custom::parse_spawns(&t, id, 1_000_000 + id * 1000))
                .unwrap_or_default();
            custom_spawns.insert(id, spawns);
            infos.push(dusk_formats::db::MapInfo {
                id,
                name,
                music: vec![],
                ambience: String::new(),
                start: (0.0, 0.0),
            });
        }
        let mut maps = HashMap::new();
        for info in &infos {
            let path = root.join("maps").join(format!("{}.map", info.name));
            match MapFile::load(&path) {
                Ok(m) => {
                    maps.insert(info.id, ServerMap { name: info.name.clone(), grid: m.walk_grid() });
                }
                Err(e) => warn!("map {} unavailable: {e}", info.name),
            }
        }
        let start = match start_map {
            Some(name) => {
                let info =
                    infos.iter().find(|i| i.name == name).ok_or_else(|| anyhow::anyhow!("unknown map {name}"))?;
                let grid = &maps.get(&info.id).ok_or_else(|| anyhow::anyhow!("map {name} failed to load"))?.grid;
                // Custom maps: the `arrival` marker of `maps/<name>.markers` (`name x y [radius]`).
                let arrival =
                    std::fs::read_to_string(root.join("maps").join(format!("{name}.markers"))).ok().and_then(|t| {
                        t.lines().find_map(|l| {
                            let v: Vec<&str> = l.split_whitespace().collect();
                            match v[..] {
                                ["arrival", x, y, ..] => Some((x.parse().ok()?, y.parse().ok()?)),
                                _ => None,
                            }
                        })
                    });
                let want = if let Some(a) = arrival {
                    a
                } else if info.start != (0.0, 0.0) {
                    info.start
                } else {
                    (grid.size as f32 / 2.0, grid.size as f32 / 2.0)
                };
                let (x, y) = grid.nearest_floor(want).unwrap_or(want);
                (info.id, Vec2::new(x, y))
            }
            None => db
                .teleports()?
                .into_iter()
                .find(|t| t.name == "start")
                .map(|t| (t.map, Vec2::new(t.x + 0.5, t.y + 0.5)))
                .unwrap_or((1, Vec2::new(17.5, 106.5))),
        };
        let custom_npcs = dusk_formats::custom::load_npc_templates(&dusk_formats::custom_assets_root());
        let class_stats = db.class_stats()?.into_iter().map(|c| ((c.class, c.level), c)).collect();
        // Our own skills (`custom_assets/data/spells.txt`, `class_spells.txt`).
        let (mut spells, mut class_spells) = (db.spells()?, db.class_spells()?);
        dusk_formats::custom::merge_spells(&dusk_formats::custom_assets_root(), &mut spells, &mut class_spells);
        Ok(Self {
            npc_templates: db
                .npc_templates()?
                .into_iter()
                .chain(custom_npcs.into_iter().map(|(t, _)| (t.entry, t)))
                .collect(),
            class_stats,
            exp_levels: db.exp_levels()?,
            spells,
            custom_spawns,
            class_spells,
            db: std::sync::Mutex::new(db),
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

/// `npc_template.faction` (see `dusk_formats::db::faction`); players use PLAYER_DEFAULT.
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

/// Default leash distance (cells) when `npc_template.leash_range` is unset. DESIGN.
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
        let spawns = match world.custom_spawns.get(&map) {
            Some(s) => s.clone(),
            None => world.db.lock().unwrap().npc_spawns(map).unwrap_or_default(),
        };
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
