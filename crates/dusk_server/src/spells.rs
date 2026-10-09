//! Spells: cast validation, cast bars, projectiles, effects and auras, NPC casting.
//!
//! Effect fields (see docs/combat.md):
//! - SchoolDamage / Heal: amount = `effectN_scale_formula` (with `value` = data2)
//! - WeaponDamage: weapon percent = formula (with `value` = data2)
//! - ApplyAura: data1 = aura type, data2 = misc (mechanic / school mask / stat), data3 = value

use crate::ai::Rng;
use crate::combat::{Attacking, CombatClock, Evading, LastAttacker, player_stats_msg};
use crate::stats::{Roll, Stats, resolve_melee};
use crate::world::{Dead, Faction, GameWorld, Hidden, Motion, NetId, Npc, OnMap, Outbox, Player, Scope};
use bevy::prelude::*;
use dusk_formats::content::types::faction;
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

    /// Percent modifier to damage taken (ModifyDmgReceivedPct), spells and melee alike.
    pub fn damage_taken_pct(&self) -> i32 {
        self.0.iter().filter(|a| a.kind == aura::MODIFY_DMG_RECEIVED_PCT).map(|a| a.value).sum()
    }

    /// Multiplier on gaze strain gains (Draw the Veil), never below 0.
    pub fn strain_gain_mult(&self) -> f32 {
        let pct: i32 = self.0.iter().filter(|a| a.kind == aura::MODIFY_STRAIN_GAIN_PCT).map(|a| a.value).sum();
        (1.0 + pct as f32 / 100.0).max(0.0)
    }
}

/// A charge stops this far (cells) short of its target: inside melee range.
const CHARGE_STOP: f32 = 1.1;

/// Charge: dash in a straight line towards the target, as far as the floor allows, stopping at
/// melee range. The caster's client is snapped there (`Correct`); everyone else sees a `Moved`.
fn charge(
    world: &GameWorld,
    units: &mut Query<Victim, Without<Hidden>>,
    outbox: &mut Outbox,
    caster: Entity,
    target: Entity,
) {
    let Ok(to) = units.get(target).map(|u| u.3.pos) else { return };
    let Ok((.., map, mut m, _, _, _, _, _, _, _)) = units.get_mut(caster) else { return };
    let from = m.pos;
    let reach = from.distance(to) - CHARGE_STOP;
    if reach <= 0.1 {
        return;
    }
    let dir = (to - from).normalize();
    let mut dest = from;
    let steps = (reach / 0.25).ceil() as i32;
    for i in 1..=steps {
        let p = from + dir * (i as f32 * 0.25).min(reach);
        if !world.is_walkable(map.0, p) {
            break;
        }
        dest = p;
    }
    if dest == from {
        return;
    }
    m.pos = dest;
    m.orientation = crate::ai::orientation(dir);
    m.moving = false;
    m.dirty = true;
    outbox.push(Scope::To(caster), ServerMsg::Correct { pos: crate::world::to_pos(dest) });
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

/// Per-NPC spell timers from the template's `spellN=` slots.
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
    &'a mut Motion,
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
    // Auras for units that had none yet, inserted after all effects ran: several auras of one
    // spell (Ignite, Draw the Veil) would otherwise overwrite each other.
    let mut fresh: HashMap<Entity, Vec<Aura>> = HashMap::new();
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
                if eff.kind == effect::CHARGE {
                    if t != p.caster {
                        charge(&world, &mut units, &mut outbox, p.caster, t);
                    }
                    continue;
                }
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
                    &mut fresh,
                );
            }
        }
    }
    for (e, list) in fresh {
        commands.entity(e).insert(Auras(list));
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
    fresh: &mut HashMap<Entity, Vec<Aura>>,
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
                aura::PERIODIC_DAMAGE | aura::PERIODIC_HEAL | aura::PERIODIC_MANA
                    if !spell.description.contains("every") =>
                {
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
                    let list = fresh.entry(t).or_default();
                    list.retain(|a| {
                        !(a.spell == spell_id && a.caster == caster && a.kind == kind && a.misc == new.misc)
                    });
                    list.push(new);
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
#[allow(clippy::type_complexity)]
pub fn tick_auras(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut outbox: ResMut<Outbox>,
    mut units: Query<(
        Entity,
        &NetId,
        &OnMap,
        &Motion,
        &mut Stats,
        &mut Auras,
        Has<Dead>,
        Has<Npc>,
        Has<Evading>,
        Option<&Player>,
    )>,
    attacking: Query<(), With<Attacking>>,
) {
    let dt = time.delta_secs();
    for (e, id, map, m, mut s, mut auras, dead, is_npc, evading, player) in &mut units {
        if dead {
            for a in auras.0.drain(..) {
                outbox.push(Scope::Near(map.0, m.pos), ServerMsg::AuraRemove { target: id.0, spell: a.spell });
            }
            continue;
        }
        let mut health_changed = false;
        let mut mana_changed = false;
        for a in auras.0.iter_mut() {
            a.remaining -= dt;
            if !matches!(a.kind, aura::PERIODIC_DAMAGE | aura::PERIODIC_HEAL | aura::PERIODIC_MANA) {
                continue;
            }
            a.tick_timer -= dt;
            // Small epsilon: the last tick lands on the same frame the aura expires.
            while a.tick_timer <= 1e-3 && a.remaining > -0.05 {
                a.tick_timer += a.interval;
                let heal = a.kind == aura::PERIODIC_HEAL;
                let amount = a.value.max(1);
                if a.kind == aura::PERIODIC_MANA {
                    s.mana = (s.mana + amount).min(s.max_mana);
                    mana_changed = true;
                    continue;
                }
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
        if let Some(p) = player.filter(|_| mana_changed) {
            outbox.push(Scope::To(e), player_stats_msg(&world, p, &s));
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

/// The Duskhollow skills (`assets/data/spells.txt`) end to end through the cast pipeline,
/// on the real data (skipped without extracted assets).
#[cfg(test)]
mod skill_tests {
    use super::*;
    use crate::stats::{npc_stats, player_stats};
    use bevy::time::TimeUpdateStrategy;
    use std::time::Duration;

    const TICK: f32 = 0.05;

    struct Sim {
        app: App,
        player: Entity,
        start: Vec2,
        /// A direction with at least 7 walkable cells in a straight line from `start`.
        dir: Vec2,
    }

    impl Sim {
        fn new() -> Self {
            let root = dusk_formats::assets_root();
            let world = GameWorld::load(&root, Some("duskhollow")).unwrap();
            let (map, start) = world.start;
            let dir = (0..16)
                .map(|i| Vec2::from_angle(i as f32 * std::f32::consts::TAU / 16.0))
                .find(|d| (0..=28).all(|k| world.is_walkable(map, start + *d * (k as f32 * 0.25))))
                .expect("open ground around the arrival point");
            let known: Vec<SpellId> = world.spells.keys().filter(|e| **e >= 50000).map(|e| *e as SpellId).collect();
            assert!(known.len() >= 10, "skills loaded: {known:?}");
            let cs = *world.class_stats(1, 1).unwrap();
            let gaze = crate::gaze::Gaze::load(&root, &world);
            let mut app = App::new();
            app.add_plugins(MinimalPlugins)
                .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(TICK)))
                .insert_resource(world)
                .insert_resource(gaze)
                .init_resource::<Outbox>()
                .init_resource::<CastRequests>()
                .init_resource::<PendingEffects>()
                .add_systems(
                    Update,
                    (start_casts, update_casts, apply_effects, tick_auras, tick_cooldowns, crate::gaze::update_strain)
                        .chain(),
                );
            let player = app
                .world_mut()
                .spawn((
                    NetId(1),
                    OnMap(map),
                    Motion { pos: start, orientation: 0.0, moving: false, dirty: false },
                    player_stats(&cs, &Default::default()),
                    Spellbook::new(known),
                    Player { name: "Test".into(), class: 1, xp: 0 },
                    Faction(faction::PLAYER_DEFAULT),
                ))
                .id();
            app.update();
            Self { app, player, start, dir }
        }

        /// A level 1 glarewolf `dist` cells from the start, unable to dodge, parry or block.
        fn wolf(&mut self, id: EntityId, dist: f32) -> Entity {
            let w = self.app.world_mut();
            let t = w.resource::<GameWorld>().npc_templates[&50001].clone();
            let mut s = npc_stats(&t, 1);
            s.max_hp = 500;
            s.hp = 500;
            (s.dodge, s.parry, s.block) = (0, 0, 0);
            let pos = self.start + self.dir * dist;
            let map = w.resource::<GameWorld>().start.0;
            w.spawn((
                NetId(id),
                OnMap(map),
                Npc { entry: 50001 },
                Faction(faction::HOSTILE),
                s,
                Motion { pos, orientation: 0.0, moving: false, dirty: false },
            ))
            .id()
        }

        fn run(&mut self, secs: f32) {
            for _ in 0..(secs / TICK).ceil() as i32 {
                self.app.update();
            }
        }

        /// Casts with a fresh spellbook and full mana, then runs `secs`; returns what was sent.
        fn cast(&mut self, spell: SpellId, target: Option<Entity>, secs: f32) -> Vec<ServerMsg> {
            self.request(spell, target, secs, false)
        }

        /// Like [`Sim::cast`], as if from using an item (potions).
        fn use_item(&mut self, spell: SpellId, secs: f32) -> Vec<ServerMsg> {
            self.request(spell, Some(self.player), secs, true)
        }

        fn request(&mut self, spell: SpellId, target: Option<Entity>, secs: f32, from_item: bool) -> Vec<ServerMsg> {
            let w = self.app.world_mut();
            let known = w.get::<Spellbook>(self.player).unwrap().known.clone();
            w.entity_mut(self.player).insert(Spellbook::new(known));
            let mut s = w.get_mut::<Stats>(self.player).unwrap();
            s.mana = s.max_mana;
            w.resource_mut::<Outbox>().0.clear();
            w.resource_mut::<CastRequests>().0.push(CastRequest { caster: self.player, spell, target, from_item });
            self.run(secs);
            self.app.world_mut().resource_mut::<Outbox>().0.drain(..).map(|(_, m)| m).collect()
        }

        fn hp(&self, e: Entity) -> i32 {
            self.app.world().get::<Stats>(e).unwrap().hp
        }

        fn control(&self, e: Entity) -> Control {
            control_of(self.app.world().get::<Auras>(e))
        }

        fn despawn(&mut self, e: Entity) {
            self.app.world_mut().despawn(e);
        }
    }

    fn hits(msgs: &[ServerMsg], spell: SpellId, target: EntityId) -> Vec<(i32, bool)> {
        msgs.iter()
            .filter_map(|m| match m {
                ServerMsg::SpellHit { spell: s, target: t, amount, heal, .. } if *s == spell && *t == target => {
                    Some((*amount, *heal))
                }
                _ => None,
            })
            .collect()
    }

    fn errors(msgs: &[ServerMsg]) -> Vec<String> {
        msgs.iter()
            .filter_map(|m| match m {
                ServerMsg::CastError { reason } => Some(reason.clone()),
                _ => None,
            })
            .collect()
    }

    fn aura_applied(msgs: &[ServerMsg], spell: SpellId, target: EntityId) -> bool {
        msgs.iter()
            .any(|m| matches!(m, ServerMsg::AuraApply { spell: s, target: t, .. } if *s == spell && *t == target))
    }

    #[test]
    fn every_skill_casts_and_lands() {
        let mut sim = Sim::new();

        // Cairnbreaker: 0.7 s wind-up, then a heavy weapon hit.
        let wolf = sim.wolf(10, 1.0);
        let m = sim.cast(50001, Some(wolf), 1.0);
        assert!(errors(&m).is_empty(), "{:?}", errors(&m));
        assert!(m.iter().any(|m| matches!(m, ServerMsg::CastStart { spell: 50001, cast_ms: 700, .. })));
        let h = hits(&m, 50001, 10);
        println!("Cairnbreaker: {h:?}");
        assert!(h.len() == 1 && h[0].0 > 0);

        // Open Vein: a cut, then three bleed ticks over 9 s.
        let m = sim.cast(50002, Some(wolf), 9.5);
        let h = hits(&m, 50002, 10);
        println!("Open Vein: {h:?}");
        assert!(aura_applied(&m, 50002, 10));
        assert_eq!(h.len(), 4, "hit + 3 ticks");
        assert!(h[1..].iter().all(|(a, heal)| *a > 0 && !heal));

        // Skullcrack: stunned for 2 s, then free again.
        let m = sim.cast(50003, Some(wolf), 0.2);
        println!("Skullcrack: {:?}", hits(&m, 50003, 10));
        assert!(aura_applied(&m, 50003, 10) && sim.control(wolf).stunned);
        sim.run(2.0);
        assert!(!sim.control(wolf).stunned);
        sim.despawn(wolf);

        // Run Down: from 5 cells away, ends in melee range with the target slowed.
        let far = sim.wolf(11, 5.0);
        let before = sim.app.world().get::<Motion>(sim.player).unwrap().pos;
        let m = sim.cast(50004, Some(far), 0.2);
        assert!(errors(&m).is_empty(), "{:?}", errors(&m));
        let after = sim.app.world().get::<Motion>(sim.player).unwrap().pos;
        let wolf_pos = sim.app.world().get::<Motion>(far).unwrap().pos;
        println!("Run Down: {:.2} -> {:.2} cells from the target", before.distance(wolf_pos), after.distance(wolf_pos));
        assert!(after.distance(wolf_pos) < 1.6, "ends in melee range");
        assert!(m.iter().any(|m| matches!(m, ServerMsg::Correct { .. })));
        assert_eq!(hits(&m, 50004, 11).len(), 1);
        assert!((sim.control(far).speed_mult - 0.5).abs() < 1e-3);
        sim.despawn(far);
        sim.app.world_mut().get_mut::<Motion>(sim.player).unwrap().pos = sim.start;

        // Clear the Row: everything within 2 cells, nothing beyond.
        let (a, b, c) = (sim.wolf(12, 1.0), sim.wolf(13, -1.5), sim.wolf(14, 4.0));
        let m = sim.cast(50005, None, 0.2);
        println!("Clear the Row: {:?} {:?}", hits(&m, 50005, 12), hits(&m, 50005, 13));
        assert_eq!((hits(&m, 50005, 12).len(), hits(&m, 50005, 13).len(), hits(&m, 50005, 14).len()), (1, 1, 0));
        for e in [a, b] {
            sim.despawn(e);
        }

        // Flung Blade: a projectile across 4 cells.
        let m = sim.cast(50006, Some(c), 1.0);
        let travel = m.iter().find_map(|m| match m {
            ServerMsg::SpellGo { spell: 50006, travel_ms, .. } => Some(*travel_ms),
            _ => None,
        });
        let h = hits(&m, 50006, 14);
        println!("Flung Blade: {h:?}, travel {travel:?} ms");
        assert!(travel.is_some_and(|t| t > 0) && h.len() == 1 && h[0].0 > 0);

        // Ember Bolt: 1.2 s cast, fire hit, then the coal smoulders for 3 ticks.
        let m = sim.cast(50007, Some(c), 5.0);
        let h = hits(&m, 50007, 14);
        println!("Ember Bolt: {h:?}");
        assert!(aura_applied(&m, 50007, 14) && h.len() == 4);

        // Drag-Hook: hit and slowed by half.
        let m = sim.cast(50008, Some(c), 1.0);
        println!("Drag-Hook: {:?}", hits(&m, 50008, 14));
        assert_eq!(hits(&m, 50008, 14).len(), 1);
        assert!((sim.control(c).speed_mult - 0.5).abs() < 1e-3);

        // Kept Ember: four heal ticks on the caster.
        sim.app.world_mut().get_mut::<Stats>(sim.player).unwrap().hp = 10;
        let m = sim.cast(50009, None, 13.5);
        let h = hits(&m, 50009, 1);
        println!("Kept Ember: {h:?}, hp 10 -> {}", sim.hp(sim.player));
        assert!(h.len() == 4 && h.iter().all(|(a, heal)| *heal && *a > 0));
        assert_eq!(sim.hp(sim.player), 10 + h.iter().map(|h| h.0).sum::<i32>());
    }

    #[test]
    fn draw_the_veil_halves_strain_gain() {
        let mut sim = Sim::new();
        // Out of combat, under open sky at the arrival point: compare 6 s of strain gain.
        sim.run(1.0);
        let strain = |sim: &Sim| sim.app.world().get::<crate::gaze::Strain>(sim.player).unwrap().strain;
        let s0 = strain(&sim);
        sim.run(6.0);
        let bare = strain(&sim) - s0;
        let m = sim.cast(50010, None, 0.05);
        assert!(aura_applied(&m, 50010, 1));
        let auras = sim.app.world().get::<Auras>(sim.player).unwrap();
        assert!((auras.strain_gain_mult() - 0.5).abs() < 1e-6);
        assert!((sim.control(sim.player).speed_mult - 0.85).abs() < 1e-3);
        let s1 = strain(&sim);
        sim.run(6.0);
        let veiled = strain(&sim) - s1;
        println!("strain over 6 s: bare {bare:.2}, veiled {veiled:.2}");
        assert!(bare > 1.0, "the arrival point is under open sky");
        assert!((veiled / bare - 0.5).abs() < 0.05, "bare {bare}, veiled {veiled}");
    }

    #[test]
    fn class_kit_skills_land() {
        let mut sim = Sim::new();
        let (a, b, far) = (sim.wolf(20, 3.0), sim.wolf(21, 4.0), sim.wolf(22, 7.5));

        // Hurled Brand: 1.8 s cast, a brand in flight, one fire hit.
        let m = sim.cast(50011, Some(a), 2.6);
        assert!(errors(&m).is_empty(), "{:?}", errors(&m));
        let h = hits(&m, 50011, 20);
        println!("Hurled Brand: {h:?}");
        assert!(h.len() == 1 && h[0].0 > 0);

        // Scatter the Coals: the target takes the throw and the burst, its neighbour (1 cell
        // away) the burst, the far one nothing; both slowed.
        let m = sim.cast(50012, Some(a), 1.0);
        println!("Scatter the Coals: {:?} {:?}", hits(&m, 50012, 20), hits(&m, 50012, 21));
        assert_eq!((hits(&m, 50012, 20).len(), hits(&m, 50012, 21).len(), hits(&m, 50012, 22).len()), (2, 1, 0));
        assert!((sim.control(b).speed_mult - 0.7).abs() < 1e-3);

        // Blinding Flare: out of the fight until hit.
        let m = sim.cast(50013, Some(far), 1.5);
        assert!(aura_applied(&m, 50013, 22) && sim.control(far).stunned);
        sim.cast(50011, Some(far), 3.0);
        assert!(!sim.control(far).stunned, "damage breaks the flare");
        for e in [a, b, far] {
            sim.despawn(e);
        }

        // Between the Ribs: one heavy cut.
        let wolf = sim.wolf(23, 1.0);
        let m = sim.cast(50014, Some(wolf), 0.2);
        println!("Between the Ribs: {:?}", hits(&m, 50014, 23));
        assert_eq!(hits(&m, 50014, 23).len(), 1);

        // Ember Prayer without a friendly target heals the caster.
        sim.app.world_mut().get_mut::<Stats>(sim.player).unwrap().hp = 10;
        let m = sim.cast(50015, Some(wolf), 2.0);
        let h = hits(&m, 50015, 1);
        println!("Ember Prayer: {h:?}");
        assert!(h.len() == 1 && h[0].1 && sim.hp(sim.player) > 10);

        // Ash Ward and Set Your Feet: less damage taken, Set Your Feet also slows.
        let m = sim.cast(50016, None, 0.1);
        assert!(aura_applied(&m, 50016, 1));
        let m = sim.cast(50017, None, 0.1);
        assert!(aura_applied(&m, 50017, 1));
        let auras = sim.app.world().get::<Auras>(sim.player).unwrap();
        assert_eq!(auras.damage_taken_pct(), -55);
        assert!((sim.control(sim.player).speed_mult - 0.7).abs() < 1e-3);
        sim.despawn(wolf);
    }

    #[test]
    fn potions_and_npc_spells() {
        let mut sim = Sim::new();
        // Ember Draught: 9 health every 2 s for 20 s, without knowing the spell.
        sim.app.world_mut().entity_mut(sim.player).insert(Spellbook::new(vec![]));
        sim.app.world_mut().get_mut::<Stats>(sim.player).unwrap().hp = 1;
        let m = sim.use_item(50110, 20.5);
        let h = hits(&m, 50110, 1);
        assert_eq!(h.len(), 10, "{h:?}");
        assert!(h.iter().all(|&(a, heal)| a == 9 && heal));
        let max_hp = sim.app.world().get::<Stats>(sim.player).unwrap().max_hp;
        assert_eq!(sim.hp(sim.player), 91.min(max_hp));
        // Lamp Tonic: 11 mana every 2 s.
        sim.app.world_mut().entity_mut(sim.player).insert(Spellbook::new(vec![]));
        let m = {
            let w = sim.app.world_mut();
            w.resource_mut::<Outbox>().0.clear();
            w.resource_mut::<CastRequests>().0.push(CastRequest {
                caster: sim.player,
                spell: 50111,
                target: Some(sim.player),
                from_item: true,
            });
            let mut s = w.get_mut::<Stats>(sim.player).unwrap();
            (s.mana, s.max_mana) = (0, 500);
            sim.run(6.5);
            sim.app.world_mut().resource_mut::<Outbox>().0.drain(..).map(|(_, m)| m).collect::<Vec<_>>()
        };
        assert!(aura_applied(&m, 50111, 1));
        let mana = sim.app.world().get::<Stats>(sim.player).unwrap().mana;
        assert_eq!(mana, 33, "3 ticks of 11");
        assert!(m.iter().any(|m| matches!(m, ServerMsg::PlayerStats { .. })));

        // The NPC spells, cast by the player for the test.
        let known: Vec<SpellId> = vec![51001, 51002, 51003, 51004];
        sim.app.world_mut().entity_mut(sim.player).insert(Spellbook::new(known));
        let wolf = sim.wolf(30, 1.0);
        let m = sim.cast(51001, Some(wolf), 8.5);
        println!("Rend: {:?}", hits(&m, 51001, 30));
        assert_eq!(hits(&m, 51001, 30).len(), 4, "a bleed of 4 ticks");
        let m = sim.cast(51002, Some(wolf), 0.2);
        assert!(hits(&m, 51002, 30).len() == 1 && aura_applied(&m, 51002, 30));
        let m = sim.cast(51004, Some(wolf), 0.5);
        assert_eq!(hits(&m, 51004, 30).len(), 1);
        let m = sim.cast(51003, None, 0.7);
        assert!(aura_applied(&m, 51003, 30) && sim.control(wolf).stunned);
    }
}
