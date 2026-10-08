//! Game feel: hit-stop, camera shake, hit flash, death dust and corpse fade, rising undead,
//! footstep dust, `hit_heavy` on crits, and the follow camera (smoothed, leading the movement).
//! Every effect has a switch below; floating combat text lives in `combat_ui`.

use crate::{
    audio::PlaySfx,
    dialogue::Modal,
    env_light::FeelColor,
    iso,
    map_render::MapLoaded,
    minimap::overlay_layer,
    net::{CombatNet, Net, SpellNet},
    player::{MainCamera, Player},
    unit::{Dead, Health, Npc, Unit},
};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use dusk_formats::db::faction;
use dusk_protocol::{EntityId, HitResult, ServerMsg};

pub const HITSTOP: bool = true;
pub const SHAKE: bool = true;
pub const FLASH: bool = true;
pub const DEATH_FX: bool = true;
pub const FOOTSTEP_DUST: bool = true;
pub const CAMERA_LEAD: bool = true;

/// Animation freeze on crits / heavy hits.
const HITSTOP_SECS: f32 = 0.06;
/// A hit is "heavy" from this fraction of the target's max health.
const HEAVY_FRACTION: f32 = 0.15;
/// Max camera offset (px) at full trauma; trauma decays per second.
const SHAKE_PX: f32 = 7.0;
const TRAUMA_DECAY: f32 = 1.8;
const FLASH_SECS: f32 = 0.13;
/// Warm red-bone tint (linear, >1 brightens): reads as "struck", not as a white flash.
const FLASH_TINT: LinearRgba = LinearRgba::rgb(1.9, 1.05, 0.8);
const CORPSE_FADE_SECS: f32 = 0.7;
const RISE_SECS: f32 = 1.1;
/// Units of these entries fade in with dust when they appear mid-game (the risen at the gate).
const RISEN: [i64; 2] = [50003, 50004];
/// Camera: how fast it catches up (1/s) and how far it leads the movement (px).
const FOLLOW_RATE: f32 = 7.0;
const LEAD_PX: f32 = 36.0;
/// The camera looks this far above the player's feet.
const CAMERA_Y: f32 = 40.0;
const STEP_SECS: f32 = 0.3;

pub struct FeelPlugin;

impl Plugin for FeelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Shake>().init_resource::<MapAge>().add_systems(Startup, make_dust_texture).add_systems(
            Update,
            (
                (on_hits, on_deaths, rise_new_units, footsteps, delayed_dust).chain(),
                (tick_tints, animate_dust).chain(),
                follow_camera.after(crate::player::move_player),
            ),
        );
    }
}

/// Seconds since the current map loaded (units already there when it loads don't "rise").
#[derive(Resource, Default)]
struct MapAge(f32);

/// Camera trauma (0..1) and the smoothed follow state.
#[derive(Resource, Default)]
pub struct Shake {
    pub trauma: f32,
    look: Option<Vec2>,
    lead: Vec2,
    last_player: Option<Vec2>,
}

impl Shake {
    pub fn add(&mut self, amount: f32) {
        if SHAKE {
            self.trauma = (self.trauma + amount).min(1.0);
        }
    }
}

#[derive(Resource)]
struct DustTexture(Handle<Image>);

/// Brief warm tint after being struck.
#[derive(Component)]
pub struct Flash(f32);

/// Alpha fade: in (rising) or out (corpse removal, despawns at the end).
#[derive(Component)]
pub struct Fade {
    t: f32,
    secs: f32,
    out: bool,
}

#[derive(Component)]
struct Dust {
    age: f32,
    life: f32,
    vel: Vec2,
    alpha: f32,
}

/// Dust to spawn after a delay (the body hitting the ground).
#[derive(Component)]
struct DustLater {
    at: f32,
    pos: Vec2,
    count: usize,
}

/// Footstep timer per running unit.
#[derive(Component, Default)]
struct Steps(f32);

/// Removes a unit the server despawned: corpses fade out first, everything else goes now.
pub fn despawn_unit(commands: &mut Commands, e: Entity, dead: bool) {
    if DEATH_FX && dead {
        commands.entity(e).insert(Fade { t: 0.0, secs: CORPSE_FADE_SECS, out: true });
    } else {
        commands.entity(e).despawn();
    }
}

/// A 12x8 dithered dust blob (oldschool: hard alpha steps, no smooth gradient).
fn make_dust_texture(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    let (w, h) = (12u32, 8u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let d = Vec2::new(
                (x as f32 + 0.5 - w as f32 / 2.0) / (w as f32 / 2.0),
                (y as f32 + 0.5 - h as f32 / 2.0) / (h as f32 / 2.0),
            )
            .length();
            // Two alpha levels, checkerboard-dithered at the rim.
            let a = if d < 0.55 {
                255
            } else if d < 1.0 && (x + y) % 2 == 0 {
                150
            } else {
                0
            };
            data.extend([255, 255, 255, a]);
        }
    }
    let img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    commands.insert_resource(DustTexture(images.add(img)));
}

/// Tiny deterministic noise for dust spread (no rand dependency).
fn hash(n: u32) -> f32 {
    let mut x = n.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
    x ^= x >> 15;
    x = x.wrapping_mul(0x2C1B_3C6D);
    x ^= x >> 12;
    (x & 0xFFFF) as f32 / 65535.0
}

fn spawn_dust(commands: &mut Commands, tex: &DustTexture, cell: Vec2, count: usize, strength: f32, seed: u32) {
    let feet = iso::to_screen(cell);
    for i in 0..count {
        let r = |k: u32| hash(seed.wrapping_mul(31).wrapping_add(i as u32 * 7 + k));
        let dir = Vec2::from_angle(r(1) * std::f32::consts::TAU);
        let pos = feet + dir * Vec2::new(10.0, 5.0) * r(2) * strength;
        let shade = 0.42 + 0.12 * r(3);
        commands.spawn((
            Dust {
                age: 0.0,
                life: 0.5 + 0.5 * r(4) * strength.min(1.5),
                vel: Vec2::new(dir.x * 16.0, 10.0 + 12.0 * r(5)) * strength,
                alpha: 0.32 + 0.18 * strength.min(1.0),
            },
            Sprite { image: tex.0.clone(), color: Color::srgba(shade + 0.08, shade, shade - 0.06, 0.0), ..default() },
            Transform::from_xyz(pos.x, pos.y, iso::depth(cell) + 0.0005)
                .with_scale(Vec3::splat(0.8 + 0.8 * r(6) * strength.min(1.5))),
            overlay_layer(),
        ));
    }
}

fn animate_dust(
    mut commands: Commands,
    time: Res<Time>,
    mut dust: Query<(Entity, &mut Dust, &mut Transform, &mut Sprite)>,
) {
    let dt = time.delta_secs();
    for (e, mut d, mut t, mut s) in &mut dust {
        d.age += dt;
        if d.age >= d.life {
            commands.entity(e).despawn();
            continue;
        }
        let k = d.age / d.life;
        t.translation += (d.vel * dt).extend(0.0);
        d.vel *= 1.0 - 3.0 * dt;
        t.scale += Vec3::splat(0.9 * dt);
        // Quick in, slow out; quantised to keep the oldschool step.
        let a = d.alpha * (k / 0.15).min(1.0) * (1.0 - k);
        s.color.set_alpha((a * 8.0).round() / 8.0);
    }
}

fn delayed_dust(
    mut commands: Commands,
    time: Res<Time>,
    tex: Option<Res<DustTexture>>,
    mut pending: Query<(Entity, &mut DustLater)>,
) {
    let Some(tex) = tex else { return };
    for (e, mut d) in &mut pending {
        d.at -= time.delta_secs();
        if d.at <= 0.0 {
            spawn_dust(&mut commands, &tex, d.pos, d.count, 1.3, e.index_u32());
            commands.entity(e).despawn();
        }
    }
}

/// Hit-stop, flash, shake and the heavy-hit sound from melee swings and spell hits.
#[allow(clippy::too_many_arguments)]
fn on_hits(
    mut commands: Commands,
    mut combat: MessageReader<CombatNet>,
    mut spells: MessageReader<SpellNet>,
    net: Res<Net>,
    data: Res<crate::data::GameData>,
    mut shake: ResMut<Shake>,
    mut units: Query<(&mut Unit, Option<&Health>, Option<&Npc>)>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let hits = combat
        .read()
        .filter_map(|CombatNet(m)| match m {
            ServerMsg::Swing { attacker, target, result, amount } => Some((*attacker, *target, *result, *amount)),
            _ => None,
        })
        .chain(spells.read().filter_map(|SpellNet(m)| match m {
            ServerMsg::SpellHit { caster, target, result, amount, heal: false, .. } => {
                Some((*caster, *target, *result, *amount))
            }
            _ => None,
        }))
        .collect::<Vec<(EntityId, EntityId, HitResult, i32)>>();
    for (attacker, target, result, amount) in hits {
        if amount <= 0 || !matches!(result, HitResult::Hit | HitResult::Crit | HitResult::Block) {
            continue;
        }
        let (Some(&te), ae) = (net.entities.get(&target), net.entities.get(&attacker).copied()) else { continue };
        let Ok((mut tu, health, _)) = units.get_mut(te) else { continue };
        let max = health.map_or(1, |h| h.max.max(1)) as f32;
        let crit = result == HitResult::Crit;
        let heavy = crit || amount as f32 >= max * HEAVY_FRACTION;
        if HITSTOP && heavy {
            tu.hitstop = HITSTOP_SECS;
        }
        if FLASH {
            commands.entity(te).try_insert(Flash(FLASH_SECS));
        }
        if crit {
            sfx.write(PlaySfx::at_unit("hit_heavy.wav", te));
        }
        let boss_attacker = ae
            .and_then(|a| units.get(a).ok())
            .and_then(|(_, _, n)| n)
            .and_then(|n| data.npc_templates.get(&n.entry))
            .is_some_and(|t| t.boss);
        if HITSTOP && heavy {
            if let Some(Ok((mut au, ..))) = ae.map(|a| units.get_mut(a)) {
                au.hitstop = HITSTOP_SECS;
            }
        }
        if Some(target) == net.my_id {
            let frac = amount as f32 / max;
            if frac >= 0.08 || boss_attacker {
                shake.add((frac * 2.5).clamp(0.2, 0.6) + if boss_attacker { 0.25 } else { 0.0 });
            }
        } else if Some(attacker) == net.my_id && crit {
            shake.add(0.12);
        }
    }
}

/// A hostile NPC died: dust where the body lands.
fn on_deaths(
    mut commands: Commands,
    mut combat: MessageReader<CombatNet>,
    net: Res<Net>,
    data: Res<crate::data::GameData>,
    units: Query<(&Unit, Option<&Npc>)>,
) {
    for CombatNet(m) in combat.read() {
        let ServerMsg::Died { id } = m else { continue };
        let Some((u, npc)) = net.entities.get(id).and_then(|e| units.get(*e).ok()) else { continue };
        let hostile =
            npc.and_then(|n| data.npc_templates.get(&n.entry)).is_some_and(|t| t.faction != faction::FRIENDLY);
        if DEATH_FX && hostile {
            commands.spawn(DustLater { at: 0.45, pos: u.pos, count: (6.0 * u.scale.max(0.8)) as usize });
        }
    }
}

/// Undead appearing mid-game rise: fade in from nothing with a burst of dust.
fn rise_new_units(
    mut commands: Commands,
    time: Res<Time>,
    mut age: ResMut<MapAge>,
    mut loaded: MessageReader<MapLoaded>,
    tex: Option<Res<DustTexture>>,
    new: Query<(Entity, &Unit, &Npc), Added<Npc>>,
) {
    if loaded.read().count() > 0 {
        age.0 = 0.0;
    }
    age.0 += time.delta_secs();
    let Some(tex) = tex else { return };
    if !DEATH_FX || age.0 < 3.0 {
        return;
    }
    for (e, u, npc) in &new {
        if RISEN.contains(&npc.entry) {
            commands.entity(e).insert(Fade { t: 0.0, secs: RISE_SECS, out: false });
            spawn_dust(&mut commands, &tex, u.pos, 9, 1.5, e.index_u32());
        }
    }
}

fn footsteps(
    mut commands: Commands,
    time: Res<Time>,
    tex: Option<Res<DustTexture>>,
    mut units: Query<(Entity, &Unit, Option<&mut Steps>), Without<Dead>>,
) {
    let Some(tex) = tex else { return };
    if !FOOTSTEP_DUST {
        return;
    }
    let dt = time.delta_secs();
    for (e, u, steps) in &mut units {
        if u.anim != "run" {
            continue;
        }
        let Some(mut steps) = steps else {
            commands.entity(e).insert(Steps::default());
            continue;
        };
        steps.0 -= dt;
        if steps.0 <= 0.0 {
            steps.0 = STEP_SECS;
            let seed = (time.elapsed_secs() * 1000.0) as u32 ^ e.index_u32();
            spawn_dust(&mut commands, &tex, u.pos, 1, 0.45, seed);
        }
    }
}

/// Flash tint and fades of units (as [`FeelColor`]).
pub fn tick_tints(
    mut commands: Commands,
    time: Res<Time>,
    mut units: Query<(Entity, Option<&mut Flash>, Option<&mut Fade>, Has<FeelColor>)>,
) {
    let dt = time.delta_secs();
    for (e, flash, fade, has_color) in &mut units {
        if flash.is_none() && fade.is_none() {
            if has_color {
                commands.entity(e).remove::<FeelColor>();
            }
            continue;
        }
        let mut color = LinearRgba::WHITE;
        if let Some(mut f) = flash {
            f.0 -= dt;
            if f.0 > 0.0 {
                // Snap on, ease off.
                let k = (f.0 / FLASH_SECS).clamp(0.0, 1.0);
                color = LinearRgba::WHITE.mix(&FLASH_TINT, k);
            } else {
                commands.entity(e).remove::<Flash>();
            }
        }
        if let Some(mut f) = fade {
            f.t += dt;
            let k = (f.t / f.secs).clamp(0.0, 1.0);
            color.alpha = if f.out { 1.0 - k } else { k };
            if k >= 1.0 {
                if f.out {
                    commands.entity(e).despawn();
                    continue;
                }
                commands.entity(e).remove::<Fade>();
            }
        }
        // Applied to the sprite layers together with the light by `env_light`.
        commands.entity(e).insert(FeelColor(color));
    }
}

/// Follows the player smoothly, leading a little into the movement, plus shake.
fn follow_camera(
    time: Res<Time>,
    modal: Res<Modal>,
    mut shake: ResMut<Shake>,
    player: Query<&Unit, With<Player>>,
    mut camera: Query<&mut Transform, With<MainCamera>>,
) {
    let (Ok(p), Ok(mut c)) = (player.single(), camera.single_mut()) else { return };
    let dt = time.delta_secs().min(0.1);
    let feet = iso::to_screen(p.pos) + Vec2::new(0.0, CAMERA_Y);
    let vel = shake.last_player.map_or(Vec2::ZERO, |l| (feet - l) / dt.max(1e-4));
    shake.last_player = Some(feet);
    // Lead toward where the player is going; snaps (teleports, map changes) reset everything.
    // Full lead at running speed (4 cells/s is roughly 140 px/s on screen).
    let want_lead = if CAMERA_LEAD && vel.length() < 2000.0 {
        (vel / 140.0 * LEAD_PX).clamp_length_max(LEAD_PX)
    } else {
        Vec2::ZERO
    };
    shake.lead = shake.lead.lerp(want_lead, 1.0 - (-dt * 2.5).exp());
    let target = feet + shake.lead;
    let look = match shake.look {
        Some(l) if l.distance(target) < 600.0 => l.lerp(target, 1.0 - (-dt * FOLLOW_RATE).exp()),
        _ => {
            shake.lead = Vec2::ZERO;
            target
        }
    };
    shake.look = Some(look);
    shake.trauma = (shake.trauma - TRAUMA_DECAY * dt).max(0.0);
    let mut offset = Vec2::ZERO;
    if shake.trauma > 0.0 && !modal.dialogue && !modal.card {
        let t = time.elapsed_secs() * 40.0;
        let n = Vec2::new((t * 1.3).sin() + (t * 2.9).sin() * 0.5, (t * 1.7).cos() + (t * 3.3).sin() * 0.5) / 1.5;
        offset = n * SHAKE_PX * shake.trauma * shake.trauma;
    }
    c.translation.x = look.x + offset.x;
    c.translation.y = look.y + offset.y;
}
