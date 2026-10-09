//! The living backdrop of the main menu: the real `custom_duskhollow` map around Lowshade's
//! cairn under a slow camera drift, the crimson grade and fire glows of the game, the Eye's
//! lid breathing, embers rising, a dark vignette, and the title card.

use super::{KeepOnMenu, MenuModel, Screen, widgets::*};
use crate::{gaze::GazeView, iso, map_render::CurrentMap, player::MainCamera};
use bevy::asset::RenderAssetUsages;
use bevy::color::Mix;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

/// The vista map and the cell the camera drifts around (Lowshade's rest cairn, docs/world.md).
pub const VISTA_MAP: &str = "custom_duskhollow";
const VISTA_CELL: Vec2 = Vec2::new(17.0, 34.0);
/// Drift: cells of sway and its (slow) angular speeds.
const DRIFT: Vec2 = Vec2::new(4.5, 3.0);
const DRIFT_RATE: Vec2 = Vec2::new(0.031, 0.047);
const EMBERS: usize = 72;

#[derive(Component)]
pub struct VistaRoot;
#[derive(Component)]
pub(super) struct Ember {
    x: f32,
    y: f32,
    speed: f32,
    sway: f32,
    phase: f32,
    life: f32,
    age: f32,
    size: f32,
}
#[derive(Component)]
pub struct TitleRoot;
#[derive(Component)]
pub struct TitleText;
#[derive(Component)]
pub struct Tagline;
#[derive(Component)]
pub struct TitleRule;

/// Seconds since the vista appeared (title fade-in).
#[derive(Resource, Default)]
pub struct VistaClock(pub f32);

fn rng(seed: &mut u32) -> f32 {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 17;
    *seed ^= *seed << 5;
    (*seed % 10_000) as f32 / 10_000.0
}

/// Requests the vista map if another one is loaded (e.g. after a session on a legacy map).
pub(super) fn request_map(
    mut map: ResMut<CurrentMap>,
    data: Res<crate::data::GameData>,
    mut brightness: ResMut<crate::lights::Brightness>,
) {
    let exists = data.root.join("maps").join(format!("{VISTA_MAP}.map")).exists();
    if exists && map.name != VISTA_MAP && map.requested.is_none() {
        map.request(VISTA_MAP);
    }
    // Legacy zones may have left the darkness on.
    *brightness = crate::lights::Brightness::default();
}

/// Spawns / despawns the backdrop with the menu.
pub(super) fn manage(
    mut commands: Commands,
    model: Res<MenuModel>,
    skin: Option<Res<Skin>>,
    roots: Query<Entity, With<VistaRoot>>,
    mut images: ResMut<Assets<Image>>,
    mut vignette: Local<Option<Handle<Image>>>,
    mut clock: ResMut<VistaClock>,
) {
    let want = model.vista;
    let have = !roots.is_empty();
    if want == have {
        return;
    }
    if !want {
        for e in &roots {
            commands.entity(e).despawn();
        }
        return;
    }
    let Some(skin) = skin else { return };
    clock.0 = 0.0;
    let vignette = vignette.get_or_insert_with(|| images.add(vignette_image())).clone();
    let mut seed = 0x9e37_79b9u32;
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            GlobalZIndex(100),
            VistaRoot,
            KeepOnMenu,
        ))
        .with_children(|v| {
            v.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    width: Val::Percent(100.0),
                    height: Val::Percent(100.0),
                    ..default()
                },
                ImageNode::new(vignette).with_mode(NodeImageMode::Stretch),
            ));
            for _ in 0..EMBERS {
                let life = 6.0 + rng(&mut seed) * 9.0;
                v.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Px(2.0),
                        height: Val::Px(2.0),
                        ..default()
                    },
                    BackgroundColor(EMBER.with_alpha(0.0)),
                    Ember {
                        x: rng(&mut seed),
                        y: 0.35 + rng(&mut seed) * 0.75,
                        speed: 0.012 + rng(&mut seed) * 0.03,
                        sway: 4.0 + rng(&mut seed) * 14.0,
                        phase: rng(&mut seed) * std::f32::consts::TAU,
                        life,
                        age: rng(&mut seed) * life,
                        size: [2.0, 2.0, 3.0, 4.0][(rng(&mut seed) * 3.99) as usize],
                    },
                ));
            }
            // Title card (main screen only; shown by `animate`).
            v.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Percent(13.0),
                    width: Val::Percent(100.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: Val::Px(6.0),
                    ..default()
                },
                Visibility::Hidden,
                TitleRoot,
            ))
            .with_children(|t| {
                t.spawn((
                    Text::new("D U S K H O L L O W"),
                    skin.title(60.0),
                    TextColor(BONE.with_alpha(0.0)),
                    TextShadow { offset: Vec2::new(0.0, 3.0), color: Color::srgba(0.35, 0.03, 0.02, 0.0) },
                    TitleText,
                ));
                t.spawn((
                    Node { width: Val::Px(360.0), height: Val::Px(14.0), ..default() },
                    ImageNode::new(skin.rule.clone()),
                    TitleRule,
                ));
                t.spawn((
                    Text::new("Another sword under the Eye."),
                    skin.text(21.0),
                    TextColor(DIM.with_alpha(0.0)),
                    TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.0) },
                    Tagline,
                ));
            });
            v.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(12.0), bottom: Val::Px(8.0), ..default() },
                Text::new(super::version_string()),
                skin.text(12.0),
                TextColor(FAINT),
                shadow(),
            ));
        });
}

/// Dark vignette: corners near-black, a heavier band at the bottom (under the buttons),
/// dithered alpha steps instead of a smooth ramp.
fn vignette_image() -> Image {
    let (w, h) = (320u32, 180u32);
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    const BAYER: [[f32; 4]; 4] =
        [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
            let r = (u * u * 0.8 + v * v * 1.1).sqrt();
            // A general dimming, near-black corners, a heavier floor and soft scrims behind
            // the title and the buttons so the lettering reads over the busy village.
            let soft = |cx: f32, cy: f32, rx: f32, ry: f32| {
                let d = (((u - cx) / rx).powi(2) + ((v - cy) / ry).powi(2)).sqrt();
                (1.0 - d).clamp(0.0, 1.0).powf(0.8)
            };
            let mut a: f32 = 0.22;
            a = a.max(((r - 0.32) / 0.8).clamp(0.0, 1.0).powf(1.2) * 0.97);
            a = a.max(((v - 0.45) / 0.55).clamp(0.0, 1.0) * 0.7);
            a = a.max(soft(0.0, -0.53, 0.62, 0.26) * 0.55);
            a = a.max(soft(0.0, 0.28, 0.32, 0.42) * 0.42);
            let d = (BAYER[(y % 4) as usize][(x % 4) as usize] + 0.5) / 16.0 - 0.5;
            let q = ((a * 8.0 + d).round() / 8.0).clamp(0.0, 1.0);
            px.extend([6, 2, 3, (q * 255.0) as u8]);
        }
    }
    let mut img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.sampler = bevy::image::ImageSampler::linear();
    img
}

/// Camera drift, the Eye's mood, embers and the title card.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub(super) fn animate(
    time: Res<Time>,
    model: Res<MenuModel>,
    mut clock: ResMut<VistaClock>,
    mut view: ResMut<GazeView>,
    mut camera: Query<&mut Transform, With<MainCamera>>,
    mut embers: Query<(&mut Ember, &mut Node, &mut BackgroundColor)>,
    mut title: Query<&mut Visibility, With<TitleRoot>>,
    mut title_text: Query<(&mut TextColor, &mut TextShadow), (With<TitleText>, Without<Tagline>)>,
    mut tagline: Query<(&mut TextColor, &mut TextShadow), (With<Tagline>, Without<TitleText>)>,
    mut rule: Query<&mut ImageNode, With<TitleRule>>,
) {
    if !model.vista {
        return;
    }
    let dt = time.delta_secs().min(0.1);
    clock.0 += dt;
    let t = time.elapsed_secs();

    // Slow drift around the cairn; the in-game follow camera takes over once playing.
    if let Ok(mut c) = camera.single_mut() {
        let cell = VISTA_CELL + Vec2::new((t * DRIFT_RATE.x).sin() * DRIFT.x, (t * DRIFT_RATE.y).cos() * DRIFT.y);
        let p = iso::to_screen(cell);
        c.translation.x = p.x;
        c.translation.y = p.y;
    }
    // The Eye: half-lidded, slowly widening and settling.
    let open = 0.3 + 0.28 * (0.5 + 0.5 * (t * 0.09).sin()).powf(2.0);
    view.active = true;
    view.openness = open;
    view.target_openness = open;
    view.strain = 0.0;
    view.flare = 0.0;

    for (mut e, mut node, mut bg) in &mut embers {
        e.age += dt;
        if e.age > e.life {
            e.age = 0.0;
            e.y = 0.95 + (e.phase * 7.3).fract() * 0.15;
            e.x = (e.x + 0.37 + e.phase * 0.1).fract();
        }
        let y = e.y - e.speed * e.age;
        let x = e.x * 100.0;
        let a = (e.age / 1.5).min(1.0) * ((e.life - e.age) / 2.5).clamp(0.0, 1.0);
        let flicker = 0.65 + 0.35 * (t * 7.0 + e.phase * 3.0).sin().abs();
        node.left = Val::Percent(x);
        node.top = Val::Percent(y * 100.0);
        node.margin.left = Val::Px((t * 0.6 + e.phase).sin() * e.sway);
        node.width = Val::Px(e.size);
        node.height = Val::Px(e.size);
        let hot = (e.phase * 5.1).fract();
        let col = if hot < 0.3 {
            Color::srgb(0.98, 0.62, 0.25)
        } else {
            EMBER.mix(&Color::srgb(0.6, 0.08, 0.04), (hot - 0.3) * 0.8)
        };
        bg.0 = col.with_alpha(a * flicker);
    }

    // Title: fades in once, flickers like a banked fire afterwards.
    let show = model.screen == Screen::Main;
    if let Ok(mut v) = title.single_mut() {
        v.set_if_neq(if show { Visibility::Inherited } else { Visibility::Hidden });
    }
    let c = clock.0;
    let fade = |start: f32| ((c - start) / 1.4).clamp(0.0, 1.0);
    let flicker = 0.88 + 0.08 * (t * 2.3).sin() * (t * 3.7 + 1.0).sin() + 0.04 * (t * 11.0).sin();
    if let Ok((mut col, mut sh)) = title_text.single_mut() {
        let a = fade(0.4);
        let warm = BONE.mix(&Color::srgb(0.98, 0.80, 0.62), 0.25 * (1.0 - flicker));
        col.0 = warm.with_alpha(a);
        sh.color = Color::srgb(0.30 + 0.18 * flicker, 0.03, 0.02).with_alpha(a * 0.95);
    }
    if let Ok(mut img) = rule.single_mut() {
        img.color = Color::WHITE.with_alpha(fade(1.2));
    }
    if let Ok((mut col, mut sh)) = tagline.single_mut() {
        let a = fade(1.8);
        col.0 = DIM.with_alpha(a);
        sh.color = Color::BLACK.with_alpha(a * 0.9);
    }
}
