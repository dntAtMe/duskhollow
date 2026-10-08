//! Particle systems of spell visual kits (`spell_visual_kit.psystem`).
//!
//! From the client's `WorldSpellAnimation` (update 0x54a7a0, kit creation 0x54ba20 /
//! 0x54b4b0 / 0x54bdb0, kit object 0x4fdf10): the emitter sits at the animation's
//! position + (`psystem_x`, `psystem_y`) (`height` = unit height).
//! - casting kit: on the caster, `setPosition(false, ..)`: emitted particles stay where
//!   they are; stopped when the cast ends (0x54c750) and removed once empty.
//! - traveling kit: on the projectile, `setPosition(true, ..)`: particles move with it.
//! - impact/go kits: their system is stopped right after creation (0x54bdb0), so it never
//!   emits; no shipped impact kit has one anyway.
//! - aura kits (`aura_kit_ontop`) are shown while the aura is up. Attached like the
//!   traveling kit (guess: the aura code was not traced).
//!
//! Projectiles fly at feet level here (the original's projectile height is unknown);
//! the kit's `psystem_y` (typically -20) lifts the emitter.

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
        app.add_systems(Update, (spawn_kit_particles, follow_units, fly).chain())
            .add_systems(Update, fx_test.run_if(|| std::env::var_os("DUSK_FX_TEST").is_some()));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum KitKind {
    Casting,
    Aura,
}

/// Emitter following a unit (casting/aura kits).
#[derive(Component)]
struct OnUnit {
    unit: Entity,
    spell: u32,
    kind: KitKind,
    x: String,
    y: String,
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

/// Kits whose `psystem` names a particle system (some rows hold `0` or a `.sa`).
pub fn kit_psi(kit: &Option<VisualKit>) -> Option<&VisualKit> {
    kit.as_ref().filter(|k| k.psystem.to_lowercase().ends_with(".psi"))
}

fn unit_height(u: &Unit) -> f32 {
    u.height * u.scale
}

#[allow(clippy::too_many_arguments)]
fn spawn_kit_particles(
    mut commands: Commands,
    mut events: MessageReader<SpellNet>,
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
                let pos = particles::kit_pos(u.pos, &kit.psystem_x, &kit.psystem_y, unit_height(u));
                commands.spawn((
                    OnUnit {
                        unit: e,
                        spell: *spell,
                        kind: KitKind::Casting,
                        x: kit.psystem_x.clone(),
                        y: kit.psystem_y.clone(),
                    },
                    particles::emitter(kit.psystem.clone(), pos, iso::depth(u.pos) + 0.3, false),
                ));
            }
            ServerMsg::CastEnd { caster, spell, .. } => {
                if let Some(&e) = net.entities.get(caster) {
                    stop(&mut on_unit, e, *spell, KitKind::Casting);
                }
            }
            ServerMsg::SpellGo { caster, spell, targets, travel_ms } => {
                let Some(kit) = data.spell_visuals.get(&(*spell as i64)).and_then(|v| kit_psi(&v.traveling)) else {
                    continue;
                };
                if *travel_ms == 0 {
                    continue;
                }
                let Some(Ok(cu)) = net.entities.get(caster).map(|e| units.get(*e)) else { continue };
                for t in targets {
                    let Some(Ok(tu)) = net.entities.get(t).map(|e| units.get(*e)) else { continue };
                    let offset = Vec2::new(
                        particles::kit_offset(&kit.psystem_x, unit_height(cu)),
                        -particles::kit_offset(&kit.psystem_y, unit_height(cu)),
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
                        particles::emitter(kit.psystem.clone(), pos, 900.0, true),
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
                let pos = particles::kit_pos(u.pos, &kit.psystem_x, &kit.psystem_y, unit_height(u));
                commands.spawn((
                    OnUnit {
                        unit: e,
                        spell: *spell,
                        kind: KitKind::Aura,
                        x: kit.psystem_x.clone(),
                        y: kit.psystem_y.clone(),
                    },
                    particles::emitter(kit.psystem.clone(), pos, iso::depth(u.pos) + 0.3, true),
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
/// traveling kit flies from the player 6 cells to the north-east; casting/aura kits stay on.
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
                let pos = particles::kit_pos(u.pos, &k.psystem_x, &k.psystem_y, h);
                commands.spawn((
                    OnUnit { unit: e, spell, kind, x: k.psystem_x.clone(), y: k.psystem_y.clone() },
                    particles::emitter(k.psystem.clone(), pos, iso::depth(u.pos) + 0.3, attached),
                ));
            }
        }
    }
    if let Some(k) = kit_psi(&visual.traveling) {
        let offset = Vec2::new(particles::kit_offset(&k.psystem_x, h), -particles::kit_offset(&k.psystem_y, h));
        let to = u.pos + Vec2::new(6.0, -6.0);
        commands.spawn((
            OnProjectile { from: u.pos, to, offset, elapsed: 0.0, duration: 0.9 },
            particles::emitter(k.psystem.clone(), iso::to_screen(u.pos) + offset, 900.0, true),
        ));
    }
}
