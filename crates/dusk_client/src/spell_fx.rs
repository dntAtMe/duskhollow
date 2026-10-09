//! Spell visuals (`data/spell_visuals.txt`): unit cast/go animations, `.sa` flipbooks for
//! projectiles and impacts. Kit particle systems live in `spell_particles`, kit sounds in
//! `audio`. Projectiles with neither a flipbook nor particles get a small orb (`unit_glow`).

use crate::{
    data::GameData,
    iso,
    net::{Net, SpellNet},
    unit::Unit,
};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use dusk_formats::spell::{FormulaVars, KitAnim, VisualKit, eval_formula};
use dusk_formats::sprite_anim::SpriteAnim;
use dusk_protocol::ServerMsg;
use std::collections::HashSet;
use std::sync::Arc;

pub struct SpellFxPlugin;

impl Plugin for SpellFxPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<KeyQueue>()
            .add_systems(
                Update,
                (
                    on_spell_events.run_if(crate::state::in_game),
                    fly_projectiles,
                    delayed_impacts,
                    play_flipbooks,
                    luma_key,
                )
                    .chain(),
            )
            .add_systems(OnEnter(crate::state::AppState::Connecting), crate::state::reset::<KeyQueue>)
            .add_systems(
                Update,
                fx_test.run_if(|| std::env::var_os("DUSK_FX_TEST").is_some()).run_if(crate::state::in_game),
            );
    }
}

/// Unit animation ids (`content::visuals::unit_anim_id`): 2 Shoot, 6 Cast, 7 Swing, 8 Hit,
/// 9 Block, 10 CastAlt.
fn unit_anim(id: i64) -> Option<&'static str> {
    match id {
        2 => Some("shoot"),
        6 => Some("cast"),
        7 => Some("swing"),
        8 => Some("hit"),
        9 => Some("block"),
        10 => Some("cast_alt"),
        _ => None,
    }
}

/// Plays a unit action; sheets without a shoot pose (most NPCs) fall back to casting.
fn play_unit_anim(u: &mut Unit, anim: &'static str) {
    u.play_action(if u.has_anim(anim) { anim } else { "cast" });
}

/// Packed RGBA (`0xRRGGBBAA`); -1/0 = untinted.
fn rgba(c: i64) -> Option<Color> {
    (c > 0).then(|| {
        let c = c as u32;
        Color::srgba_u8((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8)
    })
}

/// Flipbook frames waiting to be checked for a black background.
#[derive(Resource, Default)]
struct KeyQueue {
    pending: Vec<Handle<Image>>,
    done: HashSet<AssetId<Image>>,
}

/// Effect flipbooks are drawn on opaque black (an additive look). Sprites here alpha-blend, so
/// fully opaque frames get alpha = brightness, which looks the same over the dark-ish world.
/// Frames that already carry alpha are left alone.
fn luma_key(mut queue: ResMut<KeyQueue>, mut images: ResMut<Assets<Image>>) {
    let pending = std::mem::take(&mut queue.pending);
    for h in pending {
        if queue.done.contains(&h.id()) {
            continue;
        }
        let Some(img) = images.get(&h) else {
            queue.pending.push(h); // not loaded yet
            continue;
        };
        let Some(data) = img.data.as_ref() else { continue };
        let opaque = data.chunks_exact(4).filter(|p| p[3] == 255).count();
        let keyed = opaque * 100 >= data.len() / 4 * 98;
        queue.done.insert(h.id());
        if !keyed {
            continue;
        }
        if let Some(mut img) = images.get_mut(&h) {
            if let Some(data) = img.data.as_mut() {
                for p in data.chunks_exact_mut(4) {
                    p[3] = p[0].max(p[1]).max(p[2]);
                }
            }
        }
    }
}

/// Draw scale of a `.sa` flipbook: `1 / sqrt(ratio)` (ratio is an area ratio). A kit's `anim_x`
/// is the frames' horizontal centre times this.
fn sa_scale(anim: &SpriteAnim) -> f32 {
    1.0 / (anim.ratio.max(1) as f32).sqrt()
}

#[derive(Component)]
struct Flipbook {
    anim: Arc<SpriteAnim>,
    frames: Vec<Handle<Image>>,
    elapsed: f32,
    looping: bool,
    /// Canvas top-left in world space at scale [`sa_scale`].
    origin: Vec2,
    z: f32,
}

#[derive(Component)]
struct Projectile {
    from: Vec3,
    to: Vec3,
    elapsed: f32,
    duration: f32,
}

#[derive(Component)]
struct DelayedImpact {
    at: f32,
    target: Entity,
    kit: VisualKit,
}

#[allow(clippy::too_many_arguments)]
fn spawn_flipbook(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    queue: &mut KeyQueue,
    kit_anim: &KitAnim,
    feet: Vec2,
    unit_height: f32,
    depth: f32,
    looping: bool,
) -> Option<Entity> {
    let anim = data.flipbook(&kit_anim.sa)?;
    let frames: Vec<Handle<Image>> = anim
        .frames
        .iter()
        .map(|(n, _, _)| data.asset_path(&anim.frame_file(*n)).map(|p| assets.load(p)).unwrap_or_default())
        .collect();
    if frames.is_empty() {
        return None;
    }
    queue.pending.extend(frames.iter().filter(|f| !queue.done.contains(&f.id())).cloned());
    let scale = sa_scale(&anim);
    let canvas = anim.size as f32 * scale;
    let expr = kit_anim.y.replace("height", &format!("({unit_height})"));
    let y_off = eval_formula(&expr, &FormulaVars::default()).unwrap_or(0.0) as f32;
    // Original (y-down): canvas bottom = feet + y_off, canvas left = feet - x.
    let origin = Vec2::new(feet.x - kit_anim.x as f32, feet.y + canvas - y_off);
    let z = if kit_anim.blend == 0 && depth < 0.0 { 950.0 } else { depth.max(0.0) + 0.2 };
    let mut sprite = Sprite { image: frames[0].clone(), ..default() };
    if let Some(c) = rgba(kit_anim.color) {
        sprite.color = c;
    }
    Some(
        commands
            .spawn((
                Flipbook { anim, frames, elapsed: 0.0, looping, origin, z },
                sprite,
                Anchor::TOP_LEFT,
                Transform::from_xyz(origin.x, origin.y, z).with_scale(Vec3::splat(scale)),
            ))
            .id(),
    )
}

fn spawn_kit(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    queue: &mut KeyQueue,
    kit: &VisualKit,
    unit: &Unit,
) {
    let feet = iso::to_screen(unit.pos);
    let depth = iso::depth(unit.pos);
    for a in &kit.anims {
        spawn_flipbook(commands, data, assets, queue, a, feet, unit.height * unit.scale, depth, false);
    }
}

fn on_spell_events(
    mut commands: Commands,
    mut events: MessageReader<SpellNet>,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    net: Res<Net>,
    mut queue: ResMut<KeyQueue>,
    mut units: Query<&mut Unit>,
) {
    let now = time.elapsed_secs();
    for SpellNet(msg) in events.read() {
        match msg {
            ServerMsg::CastStart { caster, spell, .. } => {
                let visual = data.spell_visuals.get(&(*spell as i64));
                let anim = visual.and_then(|v| unit_anim(v.unit_cast_animation)).unwrap_or("cast");
                if let Some(Ok(mut u)) = net.entities.get(caster).map(|e| units.get_mut(*e)) {
                    play_unit_anim(&mut u, anim);
                }
            }
            ServerMsg::SpellGo { caster, spell, targets, travel_ms } => {
                let Some(visual) = data.spell_visuals.get(&(*spell as i64)) else { continue };
                let caster_e = net.entities.get(caster).copied();
                let target_es: Vec<Entity> = targets.iter().filter_map(|t| net.entities.get(t).copied()).collect();
                let target_pos = target_es.first().and_then(|t| units.get(*t).ok()).map(|u| u.pos);
                if let Some(Ok(mut u)) = caster_e.map(|e| units.get_mut(e)) {
                    if let Some(tp) = target_pos.filter(|tp| *tp != u.pos) {
                        u.dir = iso::direction_from_orientation(iso::orientation_of(tp - u.pos));
                    }
                    if let Some(anim) = unit_anim(visual.unit_go_animation) {
                        play_unit_anim(&mut u, anim);
                    }
                }
                // Go kit: plays on the caster as the spell is released (Gate Slam's ground ring).
                if let (Some(kit), Some(Ok(u))) = (&visual.go, caster_e.map(|e| units.get(e))) {
                    spawn_kit(&mut commands, &data, &assets, &mut queue, kit, u);
                }
                let travel = *travel_ms as f32 / 1000.0;
                for t in target_es {
                    if travel > 0.0 {
                        if let (Some(Ok(cu)), Ok(tu)) = (caster_e.map(|e| units.get(e)), units.get(t)) {
                            let chest = |u: &Unit| iso::to_screen(u.pos) + Vec2::Y * u.height * u.scale * 0.5;
                            let (from, to) = (chest(cu), chest(tu));
                            let z = 900.0;
                            let mut spawned = None;
                            if let Some(a) = visual.traveling.as_ref().and_then(|k| k.anims.first()) {
                                spawned = spawn_flipbook(
                                    &mut commands,
                                    &data,
                                    &assets,
                                    &mut queue,
                                    a,
                                    Vec2::ZERO,
                                    0.0,
                                    z,
                                    true,
                                );
                            }
                            // Particle-only kits (Ember Bolt...) are drawn by `spell_particles`: no orb.
                            let has_psi = crate::spell_particles::kit_psi(&visual.traveling).is_some();
                            if spawned.is_none() && !has_psi {
                                let glow = visual
                                    .traveling
                                    .as_ref()
                                    .and_then(|k| rgba(k.unit_glow))
                                    .unwrap_or(Color::srgb(1.0, 0.6, 0.2));
                                spawned = Some(
                                    commands
                                        .spawn((
                                            Sprite { color: glow, custom_size: Some(Vec2::splat(10.0)), ..default() },
                                            Transform::from_xyz(from.x, from.y, z),
                                        ))
                                        .id(),
                                );
                            }
                            if let Some(e) = spawned {
                                commands.entity(e).insert(Projectile {
                                    from: from.extend(z),
                                    to: to.extend(z),
                                    elapsed: 0.0,
                                    duration: travel,
                                });
                            }
                        }
                    }
                    if let Some(kit) = &visual.impact {
                        commands.spawn(DelayedImpact { at: now + travel, target: t, kit: kit.clone() });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Debug aid: `DUSK_FX_TEST=<spell id>` replays that spell's go, impact and aura kits on the player
/// every second.
fn fx_test(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut queue: ResMut<KeyQueue>,
    mut next: Local<f32>,
    player: Query<&Unit, With<crate::player::Player>>,
) {
    let (Ok(u), Some(spell)) =
        (player.single(), std::env::var("DUSK_FX_TEST").ok().and_then(|s| s.parse::<i64>().ok()))
    else {
        return;
    };
    if time.elapsed_secs() < *next {
        return;
    }
    *next = time.elapsed_secs() + 1.0;
    let Some(visual) = data.spell_visuals.get(&spell) else { return };
    for kit in [&visual.go, &visual.impact, &visual.aura_ontop].into_iter().flatten() {
        spawn_kit(&mut commands, &data, &assets, &mut queue, kit, u);
    }
}

fn fly_projectiles(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Projectile, &mut Transform, Option<&mut Flipbook>)>,
) {
    for (e, mut p, mut t, flip) in &mut q {
        p.elapsed += time.delta_secs();
        let k = (p.elapsed / p.duration).min(1.0);
        let pos = p.from.lerp(p.to, k);
        match flip {
            // Flipbooks position their canvas; keep the canvas centred on the flight path.
            Some(mut f) => {
                let half = f.anim.size as f32 * sa_scale(&f.anim) / 2.0;
                f.origin = Vec2::new(pos.x - half, pos.y + half);
            }
            None => t.translation = pos,
        }
        if k >= 1.0 {
            commands.entity(e).despawn();
        }
    }
}

fn delayed_impacts(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut queue: ResMut<KeyQueue>,
    impacts: Query<(Entity, &DelayedImpact)>,
    units: Query<&Unit>,
) {
    for (e, d) in &impacts {
        if time.elapsed_secs() < d.at {
            continue;
        }
        commands.entity(e).despawn();
        if let Ok(u) = units.get(d.target) {
            spawn_kit(&mut commands, &data, &assets, &mut queue, &d.kit, u);
        }
    }
}

fn play_flipbooks(
    mut commands: Commands,
    time: Res<Time>,
    mut q: Query<(Entity, &mut Flipbook, &mut Sprite, &mut Transform)>,
) {
    for (e, mut f, mut sprite, mut t) in &mut q {
        f.elapsed += time.delta_secs();
        let delay = f.anim.delay_ms.max(16) as f32 / 1000.0;
        let mut i = (f.elapsed / delay) as usize;
        if i >= f.frames.len() {
            if !f.looping {
                commands.entity(e).despawn();
                continue;
            }
            i %= f.frames.len();
        }
        let scale = sa_scale(&f.anim);
        let (_, ox, oy) = f.anim.frames[i];
        sprite.image = f.frames[i].clone();
        t.translation = Vec3::new(f.origin.x + ox as f32 * scale, f.origin.y - oy as f32 * scale, f.z);
    }
}
