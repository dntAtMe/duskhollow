//! Melee combat, death, experience, corpses/respawn and regeneration.

use crate::ai::Rng;
use crate::gaze::{Bound, GazeMods};
use crate::items::{GearStats, Kills};
use crate::spells::{Auras, Casting, control_of};
use crate::stats::{Stats, player_stats, resolve_melee};
use crate::world::{
    Dead, GameWorld, Hidden, Home, Motion, NetId, Npc, OnMap, Outbox, Player, Scope, entity_info, to_pos,
};
use bevy::prelude::*;
use dusk_protocol::{HitResult, ServerMsg};

/// Max centre-to-centre distance (cells) for a melee swing.
pub const MELEE_RANGE: f32 = 1.6;
/// NPC corpses stay this long before disappearing.
const CORPSE_SECS: f32 = 8.0;
/// Players revive at the start point after this long. DESIGN (no graveyards in the db).
const PLAYER_REVIVE_SECS: f32 = 5.0;
/// "Out of combat" after this long without swinging or being hit.
const OUT_OF_COMBAT_SECS: f32 = 5.0;
const REGEN_TICK_SECS: f32 = 2.0;

#[derive(Component)]
pub struct Attacking {
    pub target: Entity,
    /// Seconds until the next swing may happen.
    pub cooldown: f32,
}

impl Attacking {
    pub fn new(target: Entity) -> Self {
        // Short wind-up so engaging is not an instant free hit.
        Self { target, cooldown: 0.4 }
    }
}

/// Walking home after leashing; immune to damage meanwhile.
#[derive(Component)]
pub struct Evading;

/// Last unit that damaged this one (experience goes to it).
#[derive(Component)]
pub struct LastAttacker(pub Entity);

/// Seconds since this unit last swung or was hit.
#[derive(Component, Default)]
pub struct CombatClock(pub f32);

pub fn melee(
    mut commands: Commands,
    time: Res<Time>,
    mut rng: Local<Rng>,
    mut outbox: ResMut<Outbox>,
    mut attackers: Query<
        (Entity, &NetId, &OnMap, &Motion, &mut Attacking, Has<Player>, Has<Casting>, Option<&GazeMods>),
        (Without<Dead>, Without<Hidden>),
    >,
    mut units: Query<(&NetId, &OnMap, &Motion, &mut Stats, Has<Npc>, Has<Evading>, Has<Dead>, Has<Hidden>)>,
    auras: Query<&Auras>,
) {
    let dt = time.delta_secs();
    let mut swings = Vec::new();
    for (e, id, map, m, mut atk, is_player, casting, gaze) in &mut attackers {
        atk.cooldown -= dt;
        let target = units.get(atk.target).ok().filter(|t| t.1 == map && !t.6 && !t.7 && t.3.hp > 0);
        let Some((_, _, tm, ts, _, evading, ..)) = target else {
            commands.entity(e).remove::<Attacking>();
            if is_player {
                outbox.push(Scope::To(e), ServerMsg::TargetLost);
            } else {
                // Target died or left: NPCs reset by walking home.
                commands.entity(e).insert(Evading);
            }
            continue;
        };
        let stunned = control_of(auras.get(e).ok()).stunned;
        if atk.cooldown > 0.0 || casting || stunned || tm.pos.distance(m.pos) > MELEE_RANGE {
            continue;
        }
        let Ok((.., att_stats, _, _, _, _)) = units.get(e) else { continue };
        atk.cooldown = att_stats.melee_speed_ms as f32 / 1000.0;
        let (result, amount) = if evading { (HitResult::Evade, 0) } else { resolve_melee(att_stats, ts, &mut *rng) };
        let amount = crate::gaze::scale_damage(amount, gaze);
        swings.push((e, id.0, atk.target, result, amount, map.0, m.pos));
    }

    for (att, att_id, tgt, result, amount, map, pos) in swings {
        // Wards (Ash Ward, Set Your Feet) soften blows too.
        let pct = auras.get(tgt).map_or(0, |a| a.damage_taken_pct());
        let amount = (amount as f32 * (1.0 + pct as f32 / 100.0)).round().max(0.0) as i32;
        let Ok((tid, _, _, mut ts, is_npc, evading, ..)) = units.get_mut(tgt) else { continue };
        ts.hp = (ts.hp - amount).max(0);
        outbox.push(Scope::Near(map, pos), ServerMsg::Swing { attacker: att_id, target: tid.0, result, amount });
        outbox.push(Scope::Near(map, pos), ServerMsg::Health { id: tid.0, hp: ts.hp, max_hp: ts.max_hp });
        commands.entity(tgt).insert((LastAttacker(att), CombatClock(0.0)));
        commands.entity(att).insert(CombatClock(0.0));
        // Anything attacked fights back (neutral NPCs only ever fight this way).
        if is_npc && !evading && ts.hp > 0 && attackers.get(tgt).is_err() {
            commands.entity(tgt).insert(Attacking::new(att));
        }
    }
}

pub fn npc_deaths(
    mut commands: Commands,
    world: Res<GameWorld>,
    mut outbox: ResMut<Outbox>,
    mut kills: ResMut<Kills>,
    dying: Query<(Entity, &NetId, &OnMap, &Stats, Option<&LastAttacker>), (With<Npc>, Without<Dead>, Without<Hidden>)>,
    mut players: Query<(&NetId, &OnMap, &Motion, &mut Player, &mut Stats, Option<&GearStats>), Without<Npc>>,
) {
    for (e, id, map, s, last) in &dying {
        if s.hp > 0 {
            continue;
        }
        commands.entity(e).remove::<(Attacking, Evading, LastAttacker, Casting)>().insert(Dead { timer: CORPSE_SECS });
        outbox.push(Scope::Map(map.0), ServerMsg::Died { id: id.0 });

        let Some((pid, pmap, pm, mut p, mut ps, gear)) = last.and_then(|l| players.get_mut(l.0).ok()) else { continue };
        kills.0.push((e, last.unwrap().0));
        // DESIGN: kill_exp of the victim's level, +/-10% per level of difference, nothing 5+ levels below.
        let diff = s.level as f32 - ps.level as f32;
        let factor = if diff <= -5.0 { 0.0 } else { (1.0 + 0.1 * diff).clamp(0.1, 2.0) };
        let amount = (world.kill_xp(s.level) as f32 * factor).round() as u32;
        grant_xp(&world, &mut outbox, last.unwrap().0, (pid, pmap, pm), &mut p, &mut ps, gear, amount);
    }
}

/// Adds experience (levelling up as far as it goes) and tells the player.
pub fn grant_xp(
    world: &GameWorld,
    outbox: &mut Outbox,
    player: Entity,
    (pid, pmap, pm): (&NetId, &OnMap, &Motion),
    p: &mut Player,
    ps: &mut Stats,
    gear: Option<&GearStats>,
    amount: u32,
) {
    p.xp += amount;
    let mut leveled = false;
    while ps.level < world.max_level() && p.xp >= world.xp_to_next(ps.level) {
        p.xp -= world.xp_to_next(ps.level);
        if let Some(c) = world.class_stats(p.class, ps.level + 1) {
            *ps = player_stats(c, &gear.map(|g| g.0.clone()).unwrap_or_default());
            leveled = true;
        } else {
            break;
        }
    }
    if leveled {
        info!("{} reached level {}", p.name, ps.level);
        outbox.push(Scope::Near(pmap.0, pm.pos), ServerMsg::Health { id: pid.0, hp: ps.hp, max_hp: ps.max_hp });
    }
    outbox.push(Scope::To(player), player_stats_msg(world, p, ps));
}

pub fn player_stats_msg(world: &GameWorld, p: &Player, s: &Stats) -> ServerMsg {
    ServerMsg::PlayerStats {
        level: s.level,
        xp: p.xp,
        xp_next: world.xp_to_next(s.level),
        mana: s.mana,
        max_mana: s.max_mana,
        attributes: s.attrs,
    }
}

pub fn player_deaths(
    mut commands: Commands,
    mut outbox: ResMut<Outbox>,
    dying: Query<(Entity, &NetId, &OnMap, &Stats), (With<Player>, Without<Dead>)>,
) {
    for (e, id, map, s) in &dying {
        if s.hp <= 0 {
            commands.entity(e).remove::<(Attacking, Casting)>().insert(Dead { timer: PLAYER_REVIVE_SECS });
            outbox.push(Scope::Map(map.0), ServerMsg::Died { id: id.0 });
            outbox.push(Scope::To(e), ServerMsg::TargetLost);
        }
    }
}

/// Corpse timers, player revival and NPC respawns.
pub fn corpses(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut outbox: ResMut<Outbox>,
    mut dead: Query<(
        Entity,
        &NetId,
        &OnMap,
        &mut Dead,
        &mut Motion,
        &mut Stats,
        Has<Npc>,
        Option<&Home>,
        Option<&Bound>,
    )>,
    mut hidden: Query<(Entity, &NetId, &OnMap, &mut Hidden, &Home, &mut Motion, &mut Stats, &Npc), Without<Dead>>,
) {
    let dt = time.delta_secs();
    for (e, id, map, mut d, mut m, mut s, is_npc, home, bound) in &mut dead {
        d.timer -= dt;
        if d.timer > 0.0 {
            continue;
        }
        commands.entity(e).remove::<Dead>();
        if is_npc {
            outbox.push(Scope::Map(map.0), ServerMsg::Despawn { id: id.0 });
            let secs = home.map(|h| h.respawn_secs).unwrap_or(60.0);
            commands.entity(e).insert(Hidden { timer: secs });
        } else {
            if let Some(b) = bound.filter(|b| b.map == map.0) {
                m.pos = b.pos; // the cairn the player last rested at
            } else if map.0 == world.start.0 {
                m.pos = world.start.1;
            }
            m.moving = false;
            m.dirty = true;
            s.hp = s.max_hp;
            s.mana = s.max_mana;
            outbox.push(Scope::Map(map.0), ServerMsg::Revive { id: id.0, pos: to_pos(m.pos), hp: s.hp });
        }
    }
    for (e, id, map, mut h, home, mut m, mut s, npc) in &mut hidden {
        h.timer -= dt;
        if h.timer > 0.0 {
            continue;
        }
        m.pos = home.pos;
        m.orientation = home.orientation;
        m.moving = false;
        s.hp = s.max_hp;
        commands.entity(e).remove::<(Hidden, Auras)>();
        outbox.push(Scope::Map(map.0), ServerMsg::Spawn(entity_info(id, &m, &s, Some(npc), None, false)));
    }
}

pub fn tick_combat_clocks(time: Res<Time>, mut clocks: Query<&mut CombatClock>) {
    for mut c in &mut clocks {
        c.0 += time.delta_secs();
    }
}

/// Out-of-combat health/mana regeneration. DESIGN: players 5%, NPCs 10% per tick.
pub fn regen(
    time: Res<Time>,
    world: Res<GameWorld>,
    mut acc: Local<f32>,
    mut outbox: ResMut<Outbox>,
    mut units: Query<
        (Entity, &NetId, &OnMap, &Motion, &mut Stats, Option<&CombatClock>, Option<&Player>, Option<&GazeMods>),
        (Without<Dead>, Without<Hidden>, Without<Attacking>, Without<Evading>, Without<crate::ai::KeepsWounds>),
    >,
) {
    *acc += time.delta_secs();
    if *acc < REGEN_TICK_SECS {
        return;
    }
    *acc = 0.0;
    for (e, id, map, m, mut s, clock, player, gaze) in &mut units {
        if clock.is_some_and(|c| c.0 < OUT_OF_COMBAT_SECS) || (s.hp >= s.max_hp && s.mana >= s.max_mana) {
            continue;
        }
        let pct = if player.is_some() { 0.05 } else { 0.10 };
        let g = gaze.copied().unwrap_or_default();
        if s.hp < s.max_hp && g.hp_regen > 0.0 {
            s.hp = (s.hp + ((s.max_hp as f32 * pct * g.hp_regen).ceil() as i32).max(1)).min(s.max_hp);
            outbox.push(Scope::Near(map.0, m.pos), ServerMsg::Health { id: id.0, hp: s.hp, max_hp: s.max_hp });
        }
        if s.mana < s.max_mana {
            s.mana = (s.mana + ((s.max_mana as f32 * pct * g.mana_regen).ceil() as i32).max(1)).min(s.max_mana);
        }
        if let Some(p) = player {
            outbox.push(Scope::To(e), player_stats_msg(&world, p, &s));
        }
    }
}
