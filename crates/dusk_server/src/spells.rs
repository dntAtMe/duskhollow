//! Spells: cast validation, cast bars, projectiles, effects and auras, NPC casting.
//!
//! Field semantics recovered from the data (see docs/combat.md):
//! - SchoolDamage / Heal: amount = `effectN_scale_formula` (with `value` = data2)
//! - WeaponDamage: weapon percent = formula (with `value` = data2)
//! - ApplyAura: data1 = aura type, data2 = misc (mechanic / school mask / stat), data3 = value

use crate::ai::Rng;
use crate::combat::{Attacking, CombatClock, Evading, LastAttacker, player_stats_msg};
use crate::stats::{Roll, Stats, resolve_melee};
use crate::world::{Dead, Faction, GameWorld, Hidden, Motion, NetId, Npc, OnMap, Outbox, Player, Scope};
use bevy::prelude::*;
use dusk_formats::db::faction;
use dusk_formats::spell::{FormulaVars, SpellEffect, SpellTemplate, aura, effect, eval_formula, mechanic, target};
use dusk_protocol::{EntityId, HitResult, ServerMsg, SpellId};
use std::collections::HashMap;

/// Global cooldown after any cast. DESIGN.
pub const GCD_SECS: f32 = 1.0;
/// Extra range tolerance (cells) for latency.
const RANGE_SLACK: f32 = 0.6;
/// Projectile speed scale: `spell.speed` 16 ~ 12 cells/s. DESIGN.
const SPEED_TO_CELLS: f32 = 0.75;
/// Spell level until talents / spell ranks exist.
const SPELL_LEVEL: f64 = 1.0;

#[derive(Component, Default)]
pub struct Spellbook {
    pub known: Vec<SpellId>,
    cooldowns: HashMap<SpellId, f32>,
    gcd: f32,
}

impl Spellbook {
    pub fn new(known: Vec<SpellId>) -> Self {
        Self { known, ..default() }
    }

    /// Off cooldown and off the global cooldown.
    pub fn ready(&self, spell: SpellId) -> bool {
        self.gcd <= 0.0 && self.cooldowns.get(&spell).is_none_or(|c| *c <= 0.0)
    }
}

#[derive(Component)]
pub struct Casting {
    pub spell: SpellId,
    target: Option<Entity>,
    remaining: f32,
    start_pos: Vec2,
}

#[derive(Clone, Debug)]
pub struct Aura {
    pub spell: SpellId,
    pub caster: Entity,
    pub caster_id: EntityId,
    pub kind: i64,
    pub misc: i64,
    pub value: i32,
    pub remaining: f32,
    interval: f32,
    tick_timer: f32,
    pub positive: bool,
}

#[derive(Component, Default)]
pub struct Auras(pub Vec<Aura>);

/// Movement/action restrictions derived from auras.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Control {
    pub speed_mult: f32,
    pub rooted: bool,
    /// Can't act at all (stun, sleep, incapacitate, fear).
    pub stunned: bool,
}

impl Auras {
    pub fn control(&self) -> Control {
        let mut c = Control { speed_mult: 1.0, rooted: false, stunned: false };
        for a in &self.0 {
            match (a.kind, a.misc) {
                (aura::INFLICT_MECHANIC, mechanic::SNARE) | (aura::MODIFY_MOVE_SPEED_PCT, _) => {
                    c.speed_mult *= (1.0 + a.value as f32 / 100.0).max(0.1)
                }
                (aura::INFLICT_MECHANIC, mechanic::ROOT) => c.rooted = true,
                (
                    aura::INFLICT_MECHANIC,
                    mechanic::STUN | mechanic::SLEEP | mechanic::INCAPACITATED | mechanic::FEAR,
                ) => c.stunned = true,
                _ => {}
            }
        }
        c
    }

    /// Percent modifier to damage taken (ModifyDmgReceivedPct).
    fn damage_taken_pct(&self) -> i32 {
        self.0.iter().filter(|a| a.kind == aura::MODIFY_DMG_RECEIVED_PCT).map(|a| a.value).sum()
    }
}

pub fn control_of(auras: Option<&Auras>) -> Control {
    auras.map(|a| a.control()).unwrap_or(Control { speed_mult: 1.0, rooted: false, stunned: false })
}

/// A cast request from a player message or NPC AI.
pub struct CastRequest {
    pub caster: Entity,
    pub spell: SpellId,
    pub target: Option<Entity>,
    /// Cast by using an item (potion): the spell need not be in the spellbook.
    pub from_item: bool,
}

#[derive(Resource, Default)]
pub struct CastRequests(pub Vec<CastRequest>);

/// Effects waiting for a projectile to arrive.
#[derive(Resource, Default)]
pub struct PendingEffects(Vec<Pending>);

struct Pending {
    due: f32,
    caster: Entity,
    spell: SpellId,
    targets: Vec<Entity>,
}

/// Per-NPC spell timers from `npc_template.spell_N_*`.
#[derive(Component)]
pub struct NpcSpells(pub Vec<NpcSpellState>);

pub struct NpcSpellState {
    pub spell: SpellId,
    pub chance: f32,
    pub interval: f32,
    pub cooldown: f32,
    pub timer: f32,
    pub cd: f32,
    pub on_self: bool,
}

fn vars(s: &Stats, value: f64) -> FormulaVars {
    FormulaVars {
        clvl: s.level as f64,
        splvl: SPELL_LEVEL,
        value,
        str: s.attrs.strength as f64,
        agi: s.attrs.agility as f64,
        wil: s.attrs.willpower as f64,
        int: s.attrs.intelligence as f64,
        cur: s.attrs.courage as f64,
    }
}

fn formula(expr: &str, s: &Stats, value: i64) -> f64 {
    eval_formula(expr, &vars(s, value as f64)).unwrap_or(value as f64)
}

pub fn mana_cost(spell: &SpellTemplate, s: &Stats) -> i32 {
    let flat = if spell.mana_formula.is_empty() { 0.0 } else { formula(&spell.mana_formula, s, 0) };
    (flat as i32).max(0) + s.max_mana * spell.mana_pct.max(0) as i32 / 100
}

fn duration_secs(spell: &SpellTemplate, s: &Stats) -> f32 {
    let ms = if spell.duration_formula.is_empty() {
        spell.duration_ms as f64
    } else {
        formula(&spell.duration_formula, s, spell.duration_ms)
    };
    (ms / 1000.0).max(0.0) as f32
}

/// `CastSpell` -> validated start (cast bar) or immediate release.
#[allow(clippy::too_many_arguments)]
pub fn start_casts(
    mut commands: Commands,
    world: Res<GameWorld>,
    mut requests: ResMut<CastRequests>,
    mut outbox: ResMut<Outbox>,
    mut pending: ResMut<PendingEffects>,
    time: Res<Time>,
    mut casters: Query<(
        &NetId,
        &OnMap,
        &Motion,
        &mut Stats,
        &mut Spellbook,
        Option<&Auras>,
        Option<&CombatClock>,
        Has<Player>,
        Has<Casting>,
        Has<Dead>,
    )>,
    units: Query<(&NetId, &OnMap, &Motion, Option<&Faction>, Has<Player>), (Without<Dead>, Without<Hidden>)>,
) {
    for req in requests.0.drain(..) {
        let Ok((id, map, m, mut stats, mut book, auras, clock, is_player, casting, dead)) = casters.get_mut(req.caster)
        else {
            continue;
        };
        let mut fail = |reason: &str| {
            if is_player {
                outbox.push(Scope::To(req.caster), ServerMsg::CastError { reason: reason.into() });
            }
        };
        let Some(spell) = world.spells.get(&(req.spell as i64)) else { continue };
        if dead {
            continue;
        }
        if !req.from_item && !book.known.contains(&req.spell) {
            fail("You don't know that spell.");
            continue;
        }
        if casting {
            fail("Already casting.");
            continue;
        }
        if control_of(auras).stunned {
            fail("Can't do that now.");
            continue;
        }
        if book.gcd > 0.0 || book.cooldowns.get(&req.spell).is_some_and(|c| *c > 0.0) {
            fail("Not ready yet.");
            continue;
        }
        if spell.is_passive_or_auto() {
            continue; // auto attacks are driven by `Attack`, not casts
        }
        let cost = mana_cost(spell, &stats);
        if stats.mana < cost {
            fail("Not enough mana.");
            continue;
        }
        // NotInCombat attribute (e.g. Sleep): bit numbering unknown, use the description.
        if spell.description.contains("Cannot be used while in combat") && clock.is_some_and(|c| c.0 < 5.0) {
            fail("Can't do that while in combat.");
            continue;
        }

        // Resolve the main target from the first targeted effect.
        let main = spell.effects.iter().map(|e| e.target).find(|t| *t != 0).unwrap_or(target::CASTER);
        let hostile_ok = |e: Entity| {
            units.get(e).is_ok_and(|(_, tm, _, f, p)| {
                tm == map && !p && f.is_none_or(|f| f.0 != faction::FRIENDLY) && e != req.caster
            }) || (!is_player && units.get(e).is_ok_and(|(_, tm, _, _, p)| tm == map && p))
        };
        let tgt = match main {
            target::HOSTILE => match req.target.filter(|t| hostile_ok(*t)) {
                Some(t) => Some(t),
                None => {
                    fail("Invalid target.");
                    continue;
                }
            },
            target::FRIENDLY | target::ANY => Some(
                req.target
                    .filter(|t| {
                        units.get(*t).is_ok_and(|(_, tm, _, f, p)| {
                            tm == map && (p || f.is_some_and(|f| f.0 == faction::FRIENDLY))
                        })
                    })
                    .unwrap_or(req.caster),
            ),
            // Gameobjects / items (lockpicking, enchanting...) are not implemented yet.
            13 | 20 => {
                fail("That can't be used yet.");
                continue;
            }
            _ => Some(req.caster),
        };
        if let Some(t) = tgt.filter(|t| *t != req.caster) {
            let Ok((_, _, tm, ..)) = units.get(t) else { continue };
            if tm.pos.distance(m.pos) > spell.range_cells().max(1.6) + RANGE_SLACK {
                fail("Out of range.");
                continue;
            }
        }

        let tgt_id = tgt.and_then(|t| units.get(t).ok()).map(|u| u.0.0);
        if spell.cast_time_ms > 0 {
            let secs = spell.cast_time_ms as f32 / 1000.0;
            commands.entity(req.caster).insert(Casting {
                spell: req.spell,
                target: tgt,
                remaining: secs,
                start_pos: m.pos,
            });
            outbox.push(
                Scope::Near(map.0, m.pos),
                ServerMsg::CastStart {
                    caster: id.0,
                    spell: req.spell,
                    target: tgt_id,
                    cast_ms: spell.cast_time_ms as u32,
                },
            );
            // GCD starts with the cast so the bar can't be spammed.
            book.gcd = GCD_SECS;
        } else {
            release(
                req.caster,
                *id,
                map.0,
                m.pos,
                spell,
                tgt,
                &mut stats,
                &mut book,
                is_player,
                &units,
                &mut outbox,
                &mut pending,
                time.elapsed_secs(),
            );
        }
    }
}

/// Spends resources, starts cooldowns, picks targets and schedules effects.
#[allow(clippy::too_many_arguments)]
fn release(
    caster: Entity,
    id: NetId,
    map: i64,
    pos: Vec2,
    spell: &SpellTemplate,
    main_target: Option<Entity>,
    stats: &mut Stats,
    book: &mut Spellbook,
    is_player: bool,
    units: &Query<(&NetId, &OnMap, &Motion, Option<&Faction>, Has<Player>), (Without<Dead>, Without<Hidden>)>,
    outbox: &mut Outbox,
    pending: &mut PendingEffects,
    now: f32,
) {
    let spell_id = spell.entry as SpellId;
    stats.mana = (stats.mana - mana_cost(spell, stats)).max(0);
    let cd = spell.cooldown_ms.max(0) as f32 / 1000.0;
    book.cooldowns.insert(spell_id, cd);
    book.gcd = GCD_SECS;
    if is_player {
        outbox.push(
            Scope::To(caster),
            ServerMsg::Cooldown {
                spell: spell_id,
                ms: spell.cooldown_ms.max(0) as u32,
                gcd_ms: (GCD_SECS * 1000.0) as u32,
            },
        );
    }

    // Area effects collect their targets on impact (see `apply_effects`).
    let primary: Vec<Entity> = main_target.into_iter().collect();
    let target_ids: Vec<EntityId> = primary.iter().filter_map(|t| units.get(*t).ok()).map(|u| u.0.0).collect();
    let travel = match (spell.speed, primary.first().and_then(|t| units.get(*t).ok())) {
        (s, Some(t)) if s > 0 => t.2.pos.distance(pos) / (s as f32 * SPEED_TO_CELLS),
        _ => 0.0,
    };
    outbox.push(
        Scope::Near(map, pos),
        ServerMsg::SpellGo { caster: id.0, spell: spell_id, targets: target_ids, travel_ms: (travel * 1000.0) as u32 },
    );
    pending.0.push(Pending { due: now + travel, caster, spell: spell_id, targets: primary });
}

/// Ticks cast bars; moving or losing the target interrupts.
#[allow(clippy::too_many_arguments)]
pub fn update_casts(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut outbox: ResMut<Outbox>,
    mut pending: ResMut<PendingEffects>,
    mut casters: Query<(
        Entity,
        &NetId,
        &OnMap,
        &Motion,
        &mut Casting,
        &mut Stats,
        &mut Spellbook,
        Option<&Auras>,
        Has<Player>,
        Has<Dead>,
    )>,
    units: Query<(&NetId, &OnMap, &Motion, Option<&Faction>, Has<Player>), (Without<Dead>, Without<Hidden>)>,
) {
    let dt = time.delta_secs();
    for (e, id, map, m, mut cast, mut stats, mut book, auras, is_player, dead) in &mut casters {
        let spell_id = cast.spell;
        let interrupted = dead
            || control_of(auras).stunned
            || m.pos.distance(cast.start_pos) > 0.15
            || cast.target.is_some_and(|t| t != e && units.get(t).is_err());
        if interrupted {
            commands.entity(e).remove::<Casting>();
            outbox.push(
                Scope::Near(map.0, m.pos),
                ServerMsg::CastEnd { caster: id.0, spell: spell_id, interrupted: true },
            );
            continue;
        }
        cast.remaining -= dt;
        if cast.remaining > 0.0 {
            continue;
        }
        commands.entity(e).remove::<Casting>();
        outbox
            .push(Scope::Near(map.0, m.pos), ServerMsg::CastEnd { caster: id.0, spell: spell_id, interrupted: false });
        let Some(spell) = world.spells.get(&(spell_id as i64)) else { continue };
        if stats.mana < mana_cost(spell, &stats) {
            continue;
        }
        let target = cast.target;
        release(
            e,
            *id,
            map.0,
            m.pos,
            spell,
            target,
            &mut stats,
            &mut book,
            is_player,
            &units,
            &mut outbox,
            &mut pending,
            time.elapsed_secs(),
        );
    }
}

pub fn tick_cooldowns(time: Res<Time>, mut books: Query<&mut Spellbook>) {
    let dt = time.delta_secs();
    for mut b in &mut books {
        b.gcd = (b.gcd - dt).max(0.0);
        b.cooldowns.retain(|_, c| {
            *c -= dt;
            *c > 0.0
        });
    }
}

type Victim<'a> = (
    Entity,
    &'a NetId,
    &'a OnMap,
    &'a Motion,
    &'a mut Stats,
    Option<&'a mut Auras>,
    Option<&'a Faction>,
    Has<Npc>,
    Has<Player>,
    Has<Evading>,
    Has<Dead>,
);

/// Applies effects whose projectile has arrived.
#[allow(clippy::too_many_arguments)]
pub fn apply_effects(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut rng: Local<Rng>,
    mut outbox: ResMut<Outbox>,
    mut pending: ResMut<PendingEffects>,
    mut units: Query<Victim, Without<Hidden>>,
    attacking: Query<(), With<Attacking>>,
    gaze: Query<&crate::gaze::GazeMods>,
) {
    let now = time.elapsed_secs();
    let due: Vec<Pending> = {
        let (due, rest): (Vec<_>, Vec<_>) = pending.0.drain(..).partition(|p| p.due <= now);
        pending.0 = rest;
        due
    };
    for p in due {
        let Some(spell) = world.spells.get(&(p.spell as i64)) else { continue };
        let Ok((_, caster_id, caster_map, caster_motion, caster_stats, .., caster_is_player, _, _)) =
            units.get(p.caster)
        else {
            continue;
        };
        let (caster_id, map, caster_pos, cs) = (caster_id.0, caster_map.0, caster_motion.pos, caster_stats.clone());

        for eff in &spell.effects {
            let targets: Vec<Entity> = match eff.target {
                target::CASTER => vec![p.caster],
                target::AREA_SRC_HOSTILE | target::AREA_DST_HOSTILE | target::AREA_SRC_FRIENDLY => {
                    let hostile = eff.target != target::AREA_SRC_FRIENDLY;
                    let centre = if eff.target == target::AREA_DST_HOSTILE {
                        p.targets.first().and_then(|t| units.get(*t).ok()).map(|u| u.3.pos).unwrap_or(caster_pos)
                    } else {
                        caster_pos
                    };
                    let radius = eff.radius.max(1) as f32;
                    units
                        .iter()
                        .filter(|(t, _, tm, um, _, _, f, is_npc, is_player, _, dead)| {
                            let is_enemy = if caster_is_player {
                                *is_npc && f.is_none_or(|f| f.0 != faction::FRIENDLY)
                            } else {
                                *is_player
                            };
                            *t != p.caster
                                && !dead
                                && tm.0 == map
                                && um.pos.distance(centre) <= radius
                                && is_enemy == hostile
                        })
                        .map(|u| u.0)
                        .collect()
                }
                _ => p.targets.clone(),
            };
            for t in targets {
                apply_one(
                    &mut commands,
                    &mut *rng,
                    &mut outbox,
                    &mut units,
                    &attacking,
                    p.caster,
                    caster_id,
                    map,
                    &cs,
                    spell,
                    eff,
                    t,
                    gaze.get(p.caster).ok(),
                );
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_one(
    commands: &mut Commands,
    rng: &mut Rng,
    outbox: &mut Outbox,
    units: &mut Query<Victim, Without<Hidden>>,
    attacking: &Query<(), With<Attacking>>,
    caster: Entity,
    caster_id: EntityId,
    map: i64,
    cs: &Stats,
    spell: &SpellTemplate,
    eff: &SpellEffect,
    t: Entity,
    gaze: Option<&crate::gaze::GazeMods>,
) {
    let spell_id = spell.entry as SpellId;
    let Ok((_, tid, _, tm, mut ts, auras, _, is_npc, _, evading, dead)) = units.get_mut(t) else { return };
    if dead {
        return;
    }
    let tid = tid.0;
    let tpos = tm.pos;
    let near = Scope::Near(map, tpos);
    match eff.kind {
        effect::SCHOOL_DAMAGE | effect::WEAPON_DAMAGE => {
            let (result, mut amount) = if evading {
                (HitResult::Evade, 0)
            } else if eff.kind == effect::WEAPON_DAMAGE {
                let pct = formula(&eff.formula, cs, eff.data[1]).max(0.0) as f32;
                let mut weapon = cs.clone();
                weapon.weapon_value = (weapon.weapon_value as f32 * pct / 100.0).round() as i32;
                resolve_melee(&weapon, &ts, rng)
            } else {
                spell_roll(cs, &ts, formula(&eff.formula, cs, eff.data[1]) as f32, spell.school, rng)
            };
            if let Some(a) = &auras {
                amount = (amount as f32 * (1.0 + a.damage_taken_pct() as f32 / 100.0)).round() as i32;
            }
            amount = crate::gaze::scale_damage(amount, gaze);
            ts.hp = (ts.hp - amount).max(0);
            outbox.push(
                near,
                ServerMsg::SpellHit { caster: caster_id, target: tid, spell: spell_id, result, amount, heal: false },
            );
            outbox.push(near, ServerMsg::Health { id: tid, hp: ts.hp, max_hp: ts.max_hp });
            if amount > 0 {
                break_on_damage(auras, outbox, near, tid);
            }
            commands.entity(t).insert((LastAttacker(caster), CombatClock(0.0)));
            commands.entity(caster).insert(CombatClock(0.0));
            if is_npc && !evading && ts.hp > 0 && attacking.get(t).is_err() {
                commands.entity(t).insert(Attacking::new(caster));
            }
        }
        effect::HEAL | effect::HEAL_PCT => {
            let amount = if eff.kind == effect::HEAL_PCT {
                ts.max_hp * eff.data[0].max(0) as i32 / 100
            } else {
                let base = formula(&eff.formula, cs, eff.data[1]).max(0.0) as f32;
                (base * (0.9 + 0.2 * rng.roll())).round() as i32
            };
            ts.hp = (ts.hp + amount).min(ts.max_hp);
            outbox.push(
                near,
                ServerMsg::SpellHit {
                    caster: caster_id,
                    target: tid,
                    spell: spell_id,
                    result: HitResult::Hit,
                    amount,
                    heal: true,
                },
            );
            outbox.push(near, ServerMsg::Health { id: tid, hp: ts.hp, max_hp: ts.max_hp });
        }
        effect::RESTORE_MANA_PCT => {
            ts.mana = (ts.mana + ts.max_mana * eff.data[0].max(0) as i32 / 100).min(ts.max_mana);
        }
        effect::APPLY_AURA => {
            if evading {
                return;
            }
            let duration = duration_secs(spell, cs);
            if duration <= 0.0 {
                return;
            }
            let kind = eff.data[0];
            let value = formula(&eff.formula, cs, eff.data[2]);
            let interval = if spell.interval_ms > 0 { spell.interval_ms as f32 / 1000.0 } else { 1.0 };
            let value = match kind {
                // Periodic totals are spread over the ticks unless the tooltip says "every".
                aura::PERIODIC_DAMAGE | aura::PERIODIC_HEAL if !spell.description.contains("every") => {
                    let ticks = (duration / interval).floor().max(1.0);
                    (value / ticks as f64).ceil()
                }
                _ => value,
            };
            let new = Aura {
                spell: spell_id,
                caster,
                caster_id,
                kind,
                misc: eff.data[1],
                value: value.round() as i32,
                remaining: duration,
                interval,
                tick_timer: interval,
                positive: eff.positive,
            };
            match auras {
                Some(mut list) => {
                    list.0.retain(|a| {
                        !(a.spell == spell_id && a.caster == caster && a.kind == kind && a.misc == new.misc)
                    });
                    list.0.push(new);
                }
                None => {
                    commands.entity(t).insert(Auras(vec![new]));
                }
            }
            outbox.push(
                near,
                ServerMsg::AuraApply {
                    target: tid,
                    spell: spell_id,
                    caster: caster_id,
                    duration_ms: (duration * 1000.0) as u32,
                    positive: eff.positive,
                },
            );
            if !eff.positive {
                commands.entity(t).insert((LastAttacker(caster), CombatClock(0.0)));
                if is_npc && ts.hp > 0 && attacking.get(t).is_err() {
                    commands.entity(t).insert(Attacking::new(caster));
                }
            }
        }
        _ => {} // threat, teleports, items... not implemented yet
    }
}

/// Damage/resist/crit roll for a direct spell.
/// TEXT: resist chance = ResistX / attacker level %, halves damage; crit chance = SpellCritical / target level %.
fn spell_roll(cs: &Stats, ts: &Stats, base: f32, school: i64, rng: &mut Rng) -> (HitResult, i32) {
    let mut dmg = base.max(0.0) * (0.9 + 0.2 * rng.roll());
    // Schools: 2 frost, 3 fire, 4 shadow, 5 holy (cast_school); 1 = physical.
    let resist = match school {
        2..=5 => ts.resist[(school - 2) as usize],
        _ => 0,
    };
    let mut result = HitResult::Hit;
    if resist > 0 && rng.roll() < (resist as f32 / cs.level.max(1) as f32 / 100.0).min(0.75) {
        dmg *= 0.5;
        result = HitResult::Resist;
    } else if rng.roll() < (cs.spell_crit as f32 / ts.level.max(1) as f32 / 100.0).clamp(0.0, 0.5) {
        dmg *= 1.5;
        result = HitResult::Crit;
    }
    (result, dmg.round().max(1.0) as i32)
}

/// Incapacitate/sleep "breaks on damage".
fn break_on_damage(auras: Option<Mut<Auras>>, outbox: &mut Outbox, scope: Scope, tid: EntityId) {
    let Some(mut auras) = auras else { return };
    let (map, pos) = match scope {
        Scope::Near(m, p) => (m, p),
        _ => return,
    };
    auras.0.retain(|a| {
        let breaks = a.kind == aura::INFLICT_MECHANIC && matches!(a.misc, mechanic::INCAPACITATED | mechanic::SLEEP);
        if breaks {
            outbox.push(Scope::Near(map, pos), ServerMsg::AuraRemove { target: tid, spell: a.spell });
        }
        !breaks
    });
}

/// Aura durations and periodic ticks.
pub fn tick_auras(
    mut commands: Commands,
    time: Res<Time>,
    mut outbox: ResMut<Outbox>,
    mut units: Query<(Entity, &NetId, &OnMap, &Motion, &mut Stats, &mut Auras, Has<Dead>, Has<Npc>, Has<Evading>)>,
    attacking: Query<(), With<Attacking>>,
) {
    let dt = time.delta_secs();
    for (e, id, map, m, mut s, mut auras, dead, is_npc, evading) in &mut units {
        if dead {
            for a in auras.0.drain(..) {
                outbox.push(Scope::Near(map.0, m.pos), ServerMsg::AuraRemove { target: id.0, spell: a.spell });
            }
            continue;
        }
        let mut health_changed = false;
        for a in auras.0.iter_mut() {
            a.remaining -= dt;
            if !matches!(a.kind, aura::PERIODIC_DAMAGE | aura::PERIODIC_HEAL) {
                continue;
            }
            a.tick_timer -= dt;
            while a.tick_timer <= 0.0 && a.remaining > -0.05 {
                a.tick_timer += a.interval;
                let heal = a.kind == aura::PERIODIC_HEAL;
                let amount = a.value.max(1);
                if heal {
                    s.hp = (s.hp + amount).min(s.max_hp);
                } else if !evading {
                    s.hp = (s.hp - amount).max(0);
                    commands.entity(e).insert((LastAttacker(a.caster), CombatClock(0.0)));
                    if is_npc && s.hp > 0 && attacking.get(e).is_err() {
                        commands.entity(e).insert(Attacking::new(a.caster));
                    }
                }
                health_changed = true;
                outbox.push(
                    Scope::Near(map.0, m.pos),
                    ServerMsg::SpellHit {
                        caster: a.caster_id,
                        target: id.0,
                        spell: a.spell,
                        result: HitResult::Hit,
                        amount,
                        heal,
                    },
                );
            }
        }
        if health_changed {
            outbox.push(Scope::Near(map.0, m.pos), ServerMsg::Health { id: id.0, hp: s.hp, max_hp: s.max_hp });
        }
        auras.0.retain(|a| {
            if a.remaining <= 0.0 {
                outbox.push(Scope::Near(map.0, m.pos), ServerMsg::AuraRemove { target: id.0, spell: a.spell });
            }
            a.remaining > 0.0
        });
    }
}

/// Tells players about changes to their own movement restrictions.
pub fn sync_control(
    mut outbox: ResMut<Outbox>,
    mut last: Local<HashMap<Entity, Control>>,
    players: Query<(Entity, Option<&Auras>, Option<&crate::gaze::GazeMods>), With<Player>>,
) {
    let mut seen = HashMap::new();
    for (e, auras, gaze) in &players {
        let mut c = control_of(auras);
        c.speed_mult *= gaze.map_or(1.0, |g| g.speed);
        if last.get(&e) != Some(&c) {
            outbox.push(
                Scope::To(e),
                ServerMsg::ControlState {
                    speed_pct: (c.speed_mult * 100.0).round() as i32,
                    rooted: c.rooted,
                    stunned: c.stunned,
                },
            );
        }
        seen.insert(e, c);
    }
    *last = seen;
}

/// NPCs in combat roll their `spell_N` slots.
pub fn npc_cast(
    time: Res<Time>,
    mut rng: Local<Rng>,
    mut requests: ResMut<CastRequests>,
    mut npcs: Query<(Entity, &Attacking, &mut NpcSpells, Option<&Auras>), (Without<Casting>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    for (e, atk, mut spells, auras) in &mut npcs {
        if control_of(auras).stunned {
            continue;
        }
        for s in spells.0.iter_mut() {
            s.cd -= dt;
            s.timer -= dt;
            if s.timer > 0.0 {
                continue;
            }
            s.timer = s.interval;
            if s.cd <= 0.0 && rng.roll() * 100.0 < s.chance {
                s.cd = s.cooldown;
                requests.0.push(CastRequest {
                    caster: e,
                    spell: s.spell,
                    target: Some(if s.on_self { e } else { atk.target }),
                    from_item: false,
                });
                break;
            }
        }
    }
}

/// Mana for casters keeps trickling in during combat (Meditate-like). DESIGN: 2% per 2 s.
pub fn combat_mana(
    time: Res<Time>,
    world: Res<GameWorld>,
    mut acc: Local<f32>,
    mut outbox: ResMut<Outbox>,
    mut players: Query<(Entity, &Player, &mut Stats), Without<Dead>>,
) {
    *acc += time.delta_secs();
    if *acc < 2.0 {
        return;
    }
    *acc = 0.0;
    for (e, p, mut s) in &mut players {
        if s.mana < s.max_mana {
            s.mana = (s.mana + (s.max_mana / 50).max(1)).min(s.max_mana);
            outbox.push(Scope::To(e), player_stats_msg(&world, p, &s));
        }
    }
}
