//! NPC behaviour: idle wandering, aggro, chasing, leashing back home.

use crate::combat::{Attacking, Evading, MELEE_RANGE};
use crate::spells::{Auras, Casting, control_of};
use crate::stats::{Roll, Stats};
use crate::world::{Dead, Faction, GameWorld, Hidden, Home, Motion, NetId, Npc, OnMap, Outbox, Player, Scope, Wander};
use bevy::prelude::*;
use dusk_formats::db::faction;
use dusk_protocol::ServerMsg;

/// Cells per second while wandering.
const WANDER_SPEED: f32 = 1.5;
/// Cells per second while chasing / returning home. DESIGN (player runs at 4).
const CHASE_SPEED: f32 = 3.5;
/// Hostile NPCs notice players within this many cells. DESIGN.
pub const AGGRO_RADIUS: f32 = 5.0;

/// Tiny deterministic PRNG (xorshift) so the server has no extra dependency.
#[derive(Default)]
pub struct Rng(u64);

impl Rng {
    pub fn next_f32(&mut self) -> f32 {
        if self.0 == 0 {
            self.0 = 0x9E37_79B9_7F4A_7C15;
        }
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

impl Roll for Rng {
    fn roll(&mut self) -> f32 {
        self.next_f32()
    }
}

pub fn orientation(v: Vec2) -> f32 {
    v.y.atan2(v.x).rem_euclid(std::f32::consts::TAU)
}

/// Moves up to `dist` toward `target`, sliding along walls. Returns the new position.
pub fn step_toward(world: &GameWorld, map: i64, from: Vec2, target: Vec2, dist: f32) -> Vec2 {
    let to = target - from;
    if to.length() <= dist {
        return if world.is_walkable(map, target) { target } else { from };
    }
    let d = to.normalize() * dist;
    let mut p = from;
    if world.is_walkable(map, p + Vec2::new(d.x, 0.0)) {
        p.x += d.x;
    }
    if world.is_walkable(map, p + Vec2::new(0.0, d.y)) {
        p.y += d.y;
    }
    p
}

pub fn wander(
    time: Res<Time>,
    world: Res<GameWorld>,
    mut rng: Local<Rng>,
    mut npcs: Query<
        (&OnMap, &Home, &mut Wander, &mut Motion, Option<&Auras>),
        (Without<Attacking>, Without<Evading>, Without<Dead>, Without<Hidden>),
    >,
) {
    let dt = time.delta_secs();
    for (map, home, mut w, mut m, auras) in &mut npcs {
        let c = control_of(auras);
        if c.stunned || c.rooted {
            continue;
        }
        let Some(target) = w.target else {
            w.wait -= dt;
            if w.wait <= 0.0 {
                let t = home.pos + Vec2::from_angle(rng.range(0.0, std::f32::consts::TAU)) * rng.range(0.5, w.radius);
                if world.is_walkable(map.0, t) {
                    w.target = Some(t);
                } else {
                    w.wait = 0.5;
                }
            }
            continue;
        };
        let next = step_toward(&world, map.0, m.pos, target, WANDER_SPEED * dt);
        if next == m.pos || next == target {
            m.pos = next;
            w.target = None;
            w.wait = rng.range(3.0, 8.0);
            m.moving = false;
        } else {
            m.orientation = orientation(target - m.pos);
            m.pos = next;
            m.moving = true;
        }
        m.dirty = true;
    }
}

/// Hostile NPCs pick the nearest living player in range.
pub fn aggro(
    mut commands: Commands,
    npcs: Query<
        (Entity, &OnMap, &Motion, &Faction),
        (With<Npc>, Without<Attacking>, Without<Evading>, Without<Dead>, Without<Hidden>),
    >,
    players: Query<(Entity, &OnMap, &Motion), (With<Player>, Without<Dead>)>,
) {
    for (npc, map, m, f) in &npcs {
        if f.0 != faction::HOSTILE {
            continue;
        }
        let nearest = players
            .iter()
            .filter(|(_, pm, _)| pm.0 == map.0)
            .map(|(e, _, pmo)| (e, pmo.pos.distance(m.pos)))
            .filter(|(_, d)| *d <= AGGRO_RADIUS)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((player, _)) = nearest {
            commands.entity(npc).insert(Attacking::new(player));
        }
    }
}

/// Cached A* route for chasing / returning home.
#[derive(Component, Default)]
pub struct Nav {
    points: Vec<Vec2>,
    goal: Vec2,
    age: f32,
}

/// Seconds before a route is recomputed even if the goal did not move.
const REPATH_SECS: f32 = 1.0;
const PATH_MAX_NODES: usize = 4000;

/// Advances up to `dist` along a cached path to `goal`, recomputing it when stale.
fn navigate(world: &GameWorld, map: i64, nav: &mut Nav, pos: Vec2, goal: Vec2, dist: f32, dt: f32) -> Vec2 {
    nav.age += dt;
    if nav.points.is_empty() || nav.goal.distance(goal) > 1.0 || nav.age > REPATH_SECS {
        nav.goal = goal;
        nav.age = 0.0;
        nav.points = world
            .maps
            .get(&map)
            .and_then(|m| m.grid.find_path((pos.x, pos.y), (goal.x, goal.y), PATH_MAX_NODES))
            .map(|v| v.into_iter().map(|(x, y)| Vec2::new(x, y)).collect())
            .unwrap_or_default();
    }
    let (mut p, mut remaining) = (pos, dist);
    while remaining > 0.0 && !nav.points.is_empty() {
        let w = nav.points[0];
        let d = p.distance(w);
        if d <= remaining {
            p = w;
            remaining -= d;
            nav.points.remove(0);
        } else {
            p += (w - p) / d * remaining;
            remaining = 0.0;
        }
    }
    if world.is_walkable(map, p) { p } else { step_toward(world, map, pos, goal, dist) }
}

/// NPCs with a target run to melee range; too far from home -> evade.
pub fn chase(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut npcs: Query<
        (Entity, &OnMap, &Home, &Attacking, &mut Motion, Option<&mut Nav>, Option<&Auras>, Has<Casting>),
        (With<Npc>, Without<Dead>),
    >,
    targets: Query<&Motion, Without<Npc>>,
) {
    let dt = time.delta_secs();
    for (npc, map, home, attacking, mut m, nav, auras, casting) in &mut npcs {
        let control = control_of(auras);
        let Ok(t) = targets.get(attacking.target) else { continue };
        let Some(mut nav) = nav else {
            commands.entity(npc).insert(Nav::default());
            continue;
        };
        if m.pos.distance(home.pos) > home.leash {
            commands.entity(npc).remove::<Attacking>().insert(Evading);
            nav.points.clear();
            continue;
        }
        let d = t.pos.distance(m.pos);
        let mut face = orientation(t.pos - m.pos);
        if d > MELEE_RANGE * 0.9 && !casting && !control.stunned && !control.rooted {
            let next = navigate(&world, map.0, &mut nav, m.pos, t.pos, CHASE_SPEED * control.speed_mult * dt, dt);
            m.moving = next != m.pos;
            if m.moving {
                face = orientation(next - m.pos);
            }
            m.pos = next;
        } else {
            m.moving = false;
        }
        if m.orientation != face || m.moving {
            m.orientation = face;
            m.dirty = true;
        }
    }
}

/// NPCs that keep their wounds when they lose their target (scripted bosses: each attempt
/// chips at the same health bar).
#[derive(Component)]
pub struct KeepsWounds;

/// Evading NPCs walk home, then heal to full (unless [`KeepsWounds`]).
pub fn evade(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut outbox: ResMut<Outbox>,
    mut npcs: Query<
        (Entity, &NetId, &OnMap, &Home, &mut Motion, &mut Stats, Option<&mut Nav>, Has<KeepsWounds>),
        With<Evading>,
    >,
) {
    let dt = time.delta_secs();
    for (npc, id, map, home, mut m, mut s, nav, keeps_wounds) in &mut npcs {
        let mut fallback = Nav::default();
        let nav = match nav {
            Some(n) => n.into_inner(),
            None => &mut fallback,
        };
        let next = navigate(&world, map.0, nav, m.pos, home.pos, CHASE_SPEED * 1.3 * dt, dt);
        if next == m.pos || next.distance(home.pos) < 0.05 {
            // Arrived (or stuck): snap home and reset.
            m.pos = home.pos;
            m.orientation = home.orientation;
            m.moving = false;
            if !keeps_wounds {
                s.hp = s.max_hp;
                outbox.push(Scope::Map(map.0), ServerMsg::Health { id: id.0, hp: s.hp, max_hp: s.max_hp });
            }
            commands.entity(npc).remove::<(Evading, Nav)>();
        } else {
            m.orientation = orientation(next - m.pos);
            m.pos = next;
            m.moving = true;
        }
        m.dirty = true;
    }
}
