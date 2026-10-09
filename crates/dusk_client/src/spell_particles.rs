//! Particle systems of spell visual kits (`particles=` in `data/spell_visuals.txt`).
//!
//! The emitter sits at the kit's anchor + (`particles_x`, `particles_y`) (`height` = unit height).
//! - casting kit: on the caster; emitted particles stay where they are (the caster stands
//!   still); stopped when the cast ends and removed once empty.
//! - traveling kit: on the projectile; particles stay where they were emitted, so the
//!   projectile leaves a trail.
//! - impact/go kits: a burst at the target (after the projectile's flight) / the caster that
//!   follows the unit; it emits for [`BURST_SECS`] (or the system's own `lifetime`).
//! - aura kits are shown while the aura is up, following the unit.
//!
//! Projectiles fly at feet level; the kit's `particles_y` (typically -20) lifts the emitter.

use crate::{
    data::GameData,
    iso,
    net::{Net, SpellNet},
    particles::{self, ParticleEmitter},
    unit::Unit,
};
use bevy::prelude::*;
use dusk_formats::spell::VisualKit;
use dusk_protocol::ServerMsg;

pub struct SpellParticlesPlugin;

impl Plugin for SpellParticlesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (spawn_kit_particles.run_if(crate::state::in_game), delayed_bursts, follow_units, fly).chain(),
        )
        .add_systems(
            Update,
            fx_test.run_if(|| std::env::var_os("DUSK_FX_TEST").is_some()).run_if(crate::state::in_game),
        );
    }
}

/// How long an impact/go burst emits.
pub const BURST_SECS: f32 = 0.25;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KitKind {
    Casting,
    Aura,
    Burst,
}

/// Emitter following a unit (casting/aura kits, bursts).
#[derive(Component)]
struct OnUnit {
    unit: Entity,
    spell: u32,
    kind: KitKind,
    x: String,
    y: String,
}

/// Stops a burst emitter at `.0` (elapsed seconds).
#[derive(Component)]
struct StopAt(f32);

/// An impact burst waiting for the projectile to arrive.
#[derive(Component)]
struct DelayedBurst {
    at: f32,
    target: Entity,
    spell: u32,
    kit: VisualKit,
}

/// Emitter riding a projectile from `from` to `to` (feet positions, cells).
#[derive(Component)]
struct OnProjectile {
    from: Vec2,
    to: Vec2,
    offset: Vec2,
    elapsed: f32,
    duration: f32,
}

/// The kit, if it has a particle system.
pub fn kit_psi(kit: &Option<VisualKit>) -> Option<&VisualKit> {
    kit.as_ref().filter(|k| !k.particles.is_empty())
}

/// Spawns a burst of `kit`'s particles following unit `e`.
fn spawn_burst(commands: &mut Commands, now: f32, kit: &VisualKit, spell: u32, e: Entity, u: &Unit) {
    let pos = particles::kit_pos(u.pos, &kit.particles_x, &kit.particles_y, unit_height(u));
    commands.spawn((
        OnUnit { unit: e, spell, kind: KitKind::Burst, x: kit.particles_x.clone(), y: kit.particles_y.clone() },
        StopAt(now + BURST_SECS),
        particles::emitter(kit.particles.clone(), pos, iso::depth(u.pos) + 0.3, true),
    ));
}

fn delayed_bursts(
    mut commands: Commands,
    time: Res<Time>,
    units: Query<&Unit>,
    pending: Query<(Entity, &DelayedBurst)>,
    mut bursts: Query<(Entity, &StopAt, &mut ParticleEmitter)>,
) {
    let now = time.elapsed_secs();
    for (e, d) in &pending {
        if now < d.at {
            continue;
        }
        commands.entity(e).despawn();
        if let Ok(u) = units.get(d.target) {
            spawn_burst(&mut commands, now, &d.kit, d.spell, d.target, u);
        }
    }
    for (e, stop, mut em) in &mut bursts {
        if now >= stop.0 {
            em.stopped = true;
            commands.entity(e).remove::<StopAt>();
        }
    }
}

fn unit_height(u: &Unit) -> f32 {
    u.height * u.scale
}

#[allow(clippy::too_many_arguments)]
fn spawn_kit_particles(
    mut commands: Commands,
    mut events: MessageReader<SpellNet>,
    time: Res<Time>,
    data: Res<GameData>,
    net: Res<Net>,
    units: Query<&Unit>,
    mut on_unit: Query<(&OnUnit, &mut ParticleEmitter)>,
) {
    fn stop(q: &mut Query<(&OnUnit, &mut ParticleEmitter)>, unit: Entity, spell: u32, kind: KitKind) {
        for (o, mut em) in q {
            if o.unit == unit && o.spell == spell && o.kind == kind {
                em.stopped = true;
            }
        }
    }
    for SpellNet(msg) in events.read() {
        match msg {
            ServerMsg::CastStart { caster, spell, .. } => {
                let Some(kit) = data.spell_visuals.get(&(*spell as i64)).and_then(|v| kit_psi(&v.casting)) else {
                    continue;
                };
                let Some(&e) = net.entities.get(caster) else { continue };
                let Ok(u) = units.get(e) else { continue };
                let pos = particles::kit_pos(u.pos, &kit.particles_x, &kit.particles_y, unit_height(u));
                commands.spawn((
                    OnUnit {
                        unit: e,
                        spell: *spell,
                        kind: KitKind::Casting,
                        x: kit.particles_x.clone(),
                        y: kit.particles_y.clone(),
                    },
                    particles::emitter(kit.particles.clone(), pos, iso::depth(u.pos) + 0.3, false),
                ));
            }
            ServerMsg::CastEnd { caster, spell, .. } => {
                if let Some(&e) = net.entities.get(caster) {
                    stop(&mut on_unit, e, *spell, KitKind::Casting);
                }
            }
            ServerMsg::SpellGo { caster, spell, targets, travel_ms } => {
                let Some(visual) = data.spell_visuals.get(&(*spell as i64)) else { continue };
                let now = time.elapsed_secs();
                let caster_e = net.entities.get(caster).copied();
                if let (Some(kit), Some(e)) = (kit_psi(&visual.go), caster_e)
                    && let Ok(u) = units.get(e)
                {
                    spawn_burst(&mut commands, now, kit, *spell, e, u);
                }
                if let Some(kit) = kit_psi(&visual.impact) {
                    for t in targets.iter().filter_map(|t| net.entities.get(t)) {
                        let at = now + *travel_ms as f32 / 1000.0;
                        commands.spawn(DelayedBurst { at, target: *t, spell: *spell, kit: kit.clone() });
                    }
                }
                let Some(kit) = kit_psi(&visual.traveling) else { continue };
                if *travel_ms == 0 {
                    continue;
                }
                let Some(Ok(cu)) = caster_e.map(|e| units.get(e)) else { continue };
                for t in targets {
                    let Some(Ok(tu)) = net.entities.get(t).map(|e| units.get(*e)) else { continue };
                    let offset = Vec2::new(
                        particles::kit_offset(&kit.particles_x, unit_height(cu)),
                        -particles::kit_offset(&kit.particles_y, unit_height(cu)),
                    );
                    let pos = iso::to_screen(cu.pos) + offset;
                    commands.spawn((
                        OnProjectile {
                            from: cu.pos,
                            to: tu.pos,
                            offset,
                            elapsed: 0.0,
                            duration: *travel_ms as f32 / 1000.0,
                        },
                        particles::emitter(kit.particles.clone(), pos, 900.0, false),
                    ));
                }
            }
            ServerMsg::AuraApply { target, spell, .. } => {
                let Some(kit) = data.spell_visuals.get(&(*spell as i64)).and_then(|v| kit_psi(&v.aura_ontop)) else {
                    continue;
                };
                let Some(&e) = net.entities.get(target) else { continue };
                let Ok(u) = units.get(e) else { continue };
                // Re-applied auras refresh: keep a single emitter.
                if on_unit
                    .iter()
                    .any(|(o, em)| o.unit == e && o.spell == *spell && o.kind == KitKind::Aura && !em.stopped)
                {
                    continue;
                }
                let pos = particles::kit_pos(u.pos, &kit.particles_x, &kit.particles_y, unit_height(u));
                commands.spawn((
                    OnUnit {
                        unit: e,
                        spell: *spell,
                        kind: KitKind::Aura,
                        x: kit.particles_x.clone(),
                        y: kit.particles_y.clone(),
                    },
                    particles::emitter(kit.particles.clone(), pos, iso::depth(u.pos) + 0.3, true),
                ));
            }
            ServerMsg::AuraRemove { target, spell } => {
                if let Some(&e) = net.entities.get(target) {
                    stop(&mut on_unit, e, *spell, KitKind::Aura);
                }
            }
            _ => {}
        }
    }
}

fn follow_units(units: Query<&Unit>, mut q: Query<(&OnUnit, &mut ParticleEmitter)>) {
    for (o, mut em) in &mut q {
        match units.get(o.unit) {
            Ok(u) => {
                em.pos = particles::kit_pos(u.pos, &o.x, &o.y, unit_height(u));
                em.z = iso::depth(u.pos) + 0.3;
            }
            Err(_) => em.stopped = true,
        }
    }
}

fn fly(time: Res<Time>, mut q: Query<(&mut OnProjectile, &mut ParticleEmitter)>) {
    for (mut p, mut em) in &mut q {
        p.elapsed += time.delta_secs();
        let k = (p.elapsed / p.duration).min(1.0);
        em.pos = iso::to_screen(p.from.lerp(p.to, k)) + p.offset;
        if k >= 1.0 {
            em.stopped = true;
        }
    }
}

/// Debug aid (with `DUSK_FX_TEST=<spell id>`, see `spell_fx`): every second the spell's
/// traveling kit flies from the player 6 cells to the north-east and its go/impact particles
/// burst on the player; casting/aura kits stay on.
fn fx_test(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    mut next: Local<f32>,
    player: Query<(Entity, &Unit), With<crate::player::Player>>,
) {
    let (Ok((e, u)), Some(spell)) =
        (player.single(), std::env::var("DUSK_FX_TEST").ok().and_then(|s| s.parse::<u32>().ok()))
    else {
        return;
    };
    let Some(visual) = data.spell_visuals.get(&(spell as i64)) else { return };
    let first = *next == 0.0;
    if time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 1.0;
    let h = unit_height(u);
    if first {
        for (kit, kind, attached) in
            [(&visual.casting, KitKind::Casting, false), (&visual.aura_ontop, KitKind::Aura, true)]
        {
            if let Some(k) = kit_psi(kit) {
                let pos = particles::kit_pos(u.pos, &k.particles_x, &k.particles_y, h);
                commands.spawn((
                    OnUnit { unit: e, spell, kind, x: k.particles_x.clone(), y: k.particles_y.clone() },
                    particles::emitter(k.particles.clone(), pos, iso::depth(u.pos) + 0.3, attached),
                ));
            }
        }
    }
    for k in [&visual.go, &visual.impact].into_iter().filter_map(kit_psi) {
        spawn_burst(&mut commands, time.elapsed_secs(), k, spell, e, u);
    }
    if let Some(k) = kit_psi(&visual.traveling) {
        let offset = Vec2::new(particles::kit_offset(&k.particles_x, h), -particles::kit_offset(&k.particles_y, h));
        let to = u.pos + Vec2::new(6.0, -6.0);
        commands.spawn((
            OnProjectile { from: u.pos, to, offset, elapsed: 0.0, duration: 0.9 },
            particles::emitter(k.particles.clone(), iso::to_screen(u.pos) + offset, 900.0, false),
        ));
    }
}
