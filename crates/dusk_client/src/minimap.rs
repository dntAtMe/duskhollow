//! Minimap, top-right on the original `minimap.png` frame: a second camera renders the live
//! world around the player into a texture (so it matches whatever map is loaded), faded at the
//! edges with `miniamp_decal.png` and overlaid with unit dots (`minimap_enemy/neutral/friendly/
//! dead.png`) and a player arrow. Mouse wheel over it, numpad `+`/`-` or the button under it zoom.
//!
//! Render layers: the world is on layer 0 (seen by both cameras); name plates, floating combat
//! text and other world-space overlays go on [`OVERLAY_LAYER`], which only the main camera sees.

use crate::{
    data::GameData,
    hud::HudConfig,
    iso,
    player::{MainCamera, Player, PlayerMotion},
    ui_input::CapturesPointer,
    unit::{Dead, Npc, Unit},
};
use bevy::asset::RenderAssetUsages;
use bevy::camera::RenderTarget;
use bevy::camera::visibility::RenderLayers;
use bevy::image::ImageSampler;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use dusk_formats::db::faction;
use std::collections::HashMap;

pub struct MinimapPlugin;

impl Plugin for MinimapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MinimapZoom>()
            .add_systems(Startup, spawn_minimap.after(crate::combat_ui::load_font))
            .add_systems(Update, (setup_main_camera, zoom, follow_player, update_dots, bake_fade).chain());
    }
}

/// Render layer for world-space overlays the minimap must not show.
pub const OVERLAY_LAYER: usize = 1;

pub fn overlay_layer() -> RenderLayers {
    RenderLayers::layer(OVERLAY_LAYER)
}

/// World pixels per minimap pixel, per zoom level (`MinimapZoom` in config.ini picks the start).
const ZOOM_LEVELS: [f32; 5] = [3.0, 4.5, 6.0, 9.0, 13.0];
/// The texture is rendered at this multiple of its display size (cheap anti-aliasing).
const SUPERSAMPLE: f32 = 2.0;
const FRAME_SIZE: Vec2 = Vec2::new(241.0, 293.0);
/// The map view inside the frame (= `miniamp_decal.png`).
const VIEW_POS: Vec2 = Vec2::new(6.0, 46.0);
const VIEW_SIZE: Vec2 = Vec2::new(230.0, 227.0);
const DOT: f32 = 18.0;

#[derive(Resource)]
struct MinimapZoom(usize);

impl Default for MinimapZoom {
    fn default() -> Self {
        Self(1)
    }
}

#[derive(Component)]
struct MinimapCamera;
#[derive(Component)]
struct DotLayer;
#[derive(Component)]
struct PlayerArrow;
/// The button under the map (idle/hover/press art); cycles the zoom level.
#[derive(Component)]
struct ZoomButton([Handle<Image>; 3]);
/// The fade overlay, waiting for `miniamp_decal.png` to load.
#[derive(Component)]
struct Fade(Handle<Image>);

fn img(data: &GameData, assets: &AssetServer, name: &str) -> Handle<Image> {
    data.asset_path(name).map(|p| assets.load(p)).unwrap_or_default()
}

fn abs(pos: Vec2, size: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(pos.x),
        top: Val::Px(pos.y),
        width: Val::Px(size.x),
        height: Val::Px(size.y),
        ..default()
    }
}

fn spawn_minimap(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    config: Option<Res<HudConfig>>,
    mut zoom: ResMut<MinimapZoom>,
    mut images: ResMut<Assets<Image>>,
) {
    zoom.0 = config.map(|c| c.minimap_zoom).unwrap_or(1).min(ZOOM_LEVELS.len() - 1);
    let size = (VIEW_SIZE * SUPERSAMPLE).as_uvec2();
    let mut target = Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
    target.sampler = ImageSampler::linear();
    let target = images.add(target);

    commands.spawn((
        Camera2d,
        Camera { order: -1, clear_color: ClearColorConfig::Custom(Color::BLACK), ..default() },
        RenderTarget::Image(target.clone().into()),
        Projection::Orthographic(OrthographicProjection {
            scale: ZOOM_LEVELS[zoom.0] / SUPERSAMPLE,
            ..OrthographicProjection::default_2d()
        }),
        RenderLayers::layer(0),
        MinimapCamera,
    ));

    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Px(FRAME_SIZE.x),
                height: Val::Px(FRAME_SIZE.y),
                ..default()
            },
            ImageNode::new(img(&data, &assets, "minimap.png")),
            CapturesPointer,
        ))
        .with_children(|p| {
            p.spawn((abs(VIEW_POS, VIEW_SIZE), ImageNode::new(target)));
            let decal = img(&data, &assets, "miniamp_decal.png");
            p.spawn((abs(VIEW_POS, VIEW_SIZE), ImageNode::default(), Visibility::Hidden, Fade(decal)));
            let mut dots = abs(VIEW_POS, VIEW_SIZE);
            dots.overflow = Overflow::clip();
            p.spawn((dots, DotLayer)).with_children(|d| {
                d.spawn((
                    abs(VIEW_SIZE / 2.0 - Vec2::splat(8.0), Vec2::splat(16.0)),
                    ImageNode::new(images.add(arrow_image())),
                    UiTransform::default(),
                    ZIndex(10),
                    PlayerArrow,
                ));
            });
            let art = ["idle", "hover", "press"].map(|s| img(&data, &assets, &format!("minimap_button_{s}.png")));
            p.spawn((
                abs(Vec2::new(99.5, 250.0), Vec2::new(42.0, 41.0)),
                Button,
                ImageNode::new(art[0].clone()),
                ZoomButton(art),
            ));
        });
}

/// 16x16 arrow pointing up (the frame has no player marker art).
fn arrow_image() -> Image {
    let mut image = Image::new_fill(
        Extent3d { width: 16, height: 16, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    for y in 0..16u32 {
        for x in 0..16u32 {
            // Arrow head: triangle from the tip (8, 1) to the base y = 14, notched at the back.
            let (fx, fy) = (x as f32 + 0.5 - 8.0, y as f32 + 0.5);
            let half = (fy - 1.0) * 0.5;
            let notch = fy > 10.0 && fx.abs() < (fy - 10.0) * 1.2;
            let inside = fy >= 1.0 && fy <= 14.5 && fx.abs() <= half && !notch;
            let edge = inside && (fx.abs() > half - 1.3 || fy > 13.5);
            let c = if edge {
                Color::srgb(0.1, 0.07, 0.02)
            } else if inside {
                Color::srgb(1.0, 0.86, 0.35)
            } else {
                continue;
            };
            let _ = image.set_color_at(x, y, c);
        }
    }
    image
}

/// Turns `miniamp_decal.png` (alpha = how much map shows) into a dark overlay with the inverse
/// alpha, so the map fades into the frame at its brushed edges.
fn bake_fade(mut fades: Query<(&Fade, &mut ImageNode, &mut Visibility)>, mut images: ResMut<Assets<Image>>) {
    for (fade, mut node, mut vis) in &mut fades {
        if *vis != Visibility::Hidden {
            continue;
        }
        let Some(decal) = images.get(&fade.0) else { continue };
        let (w, h) = (decal.width(), decal.height());
        let mut out = Image::new_fill(
            Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            TextureDimension::D2,
            &[0, 0, 0, 0],
            TextureFormat::Rgba8UnormSrgb,
            RenderAssetUsages::default(),
        );
        for y in 0..h {
            for x in 0..w {
                let a = decal.get_color_at(x, y).map(|c| c.alpha()).unwrap_or(0.0);
                let _ = out.set_color_at(x, y, Color::srgba(0.03, 0.02, 0.015, 1.0 - a));
            }
        }
        node.image = images.add(out);
        *vis = Visibility::Inherited;
    }
}

/// The main camera sees the world and the overlay layer and owns the UI.
fn setup_main_camera(mut commands: Commands, cameras: Query<Entity, Added<MainCamera>>) {
    for e in &cameras {
        commands.entity(e).insert((RenderLayers::from_layers(&[0, OVERLAY_LAYER]), IsDefaultUiCamera));
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn zoom(
    keys: Res<ButtonInput<KeyCode>>,
    mut button: Query<(&Interaction, &ZoomButton, &mut ImageNode), Changed<Interaction>>,
    mut wheel: MessageReader<MouseWheel>,
    over: Query<&Interaction, (With<CapturesPointer>, With<ImageNode>, Without<Button>)>,
    frame: Query<Entity, With<DotLayer>>,
    parents: Query<&ChildOf>,
    captured: Res<crate::ui_input::UiInputCaptured>,
    mut level: ResMut<MinimapZoom>,
    mut camera: Query<&mut Projection, With<MinimapCamera>>,
) {
    let mut delta = 0i32;
    for (interaction, art, mut image) in &mut button {
        let i = match interaction {
            Interaction::None => 0,
            Interaction::Hovered => 1,
            Interaction::Pressed => 2,
        };
        image.image = art.0[i].clone();
        if *interaction == Interaction::Pressed {
            // Cycle: zoom out step by step, then back to the closest view.
            delta = if level.0 + 1 < ZOOM_LEVELS.len() { 1 } else { -(level.0 as i32) };
        }
    }
    if !captured.keyboard {
        if keys.just_pressed(KeyCode::NumpadAdd) {
            delta -= 1;
        }
        if keys.just_pressed(KeyCode::NumpadSubtract) {
            delta += 1;
        }
    }
    // Wheel only while hovering the minimap frame.
    let hovered = frame
        .single()
        .ok()
        .and_then(|d| parents.get(d).ok())
        .and_then(|p| over.get(p.parent()).ok())
        .is_some_and(|i| *i != Interaction::None);
    for w in wheel.read() {
        if hovered {
            let y = if w.unit == MouseScrollUnit::Pixel { w.y / 16.0 } else { w.y };
            delta -= y.signum() as i32;
        }
    }
    if delta == 0 {
        return;
    }
    level.0 = (level.0 as i32 + delta).clamp(0, ZOOM_LEVELS.len() as i32 - 1) as usize;
    if let Ok(mut p) = camera.single_mut() {
        if let Projection::Orthographic(o) = &mut *p {
            o.scale = ZOOM_LEVELS[level.0] / SUPERSAMPLE;
        }
    }
}

fn follow_player(
    player: Query<&Transform, With<Player>>,
    mut camera: Query<&mut Transform, (With<MinimapCamera>, Without<Player>)>,
) {
    let (Ok(p), Ok(mut c)) = (player.single(), camera.single_mut()) else { return };
    c.translation.x = p.translation.x;
    c.translation.y = p.translation.y;
}

/// One dot per unit in view, keyed by unit entity.
#[derive(Default)]
struct Dots(HashMap<Entity, Entity>);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn update_dots(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    level: Res<MinimapZoom>,
    mut dots: Local<Dots>,
    layer: Query<Entity, With<DotLayer>>,
    me: Query<(&Unit, &PlayerMotion), With<Player>>,
    units: Query<(Entity, &Unit, Option<&Npc>, Has<Dead>), Without<Player>>,
    mut nodes: Query<(&mut Node, &mut ImageNode), Without<PlayerArrow>>,
    mut arrow: Query<&mut UiTransform, With<PlayerArrow>>,
) {
    let (Ok(layer), Ok((me, motion))) = (layer.single(), me.single()) else { return };
    let scale = ZOOM_LEVELS[level.0];
    let centre = iso::to_screen(me.pos);

    // Arrow: facing in screen space (UI rotation is clockwise, art points up).
    if let Ok(mut t) = arrow.single_mut() {
        let dir = iso::to_screen(Vec2::from_angle(motion.orientation)) - iso::to_screen(Vec2::ZERO);
        let angle = std::f32::consts::FRAC_PI_2 - dir.y.atan2(dir.x);
        t.rotation = Rot2::radians(angle);
    }

    let mut seen = Vec::with_capacity(dots.0.len());
    for (e, unit, npc, dead) in &units {
        let d = (iso::to_screen(unit.pos) - centre) / scale;
        let p = VIEW_SIZE / 2.0 + Vec2::new(d.x, -d.y);
        if p.x < -DOT || p.y < -DOT || p.x > VIEW_SIZE.x + DOT || p.y > VIEW_SIZE.y + DOT {
            continue;
        }
        let art = if dead {
            "minimap_dead.png"
        } else {
            match npc.and_then(|n| data.npc_templates.get(&n.entry)).map(|t| t.faction) {
                Some(faction::HOSTILE) => "minimap_enemy.png",
                Some(faction::NEUTRAL) => "minimap_neutral.png",
                _ => "minimap_friendly.png",
            }
        };
        let handle = img(&data, &assets, art);
        let pos = (p - Vec2::splat(DOT / 2.0)).round();
        seen.push(e);
        match dots.0.get(&e).and_then(|d| nodes.get_mut(*d).ok()) {
            Some((mut node, mut image)) => {
                node.left = Val::Px(pos.x);
                node.top = Val::Px(pos.y);
                if image.image != handle {
                    image.image = handle;
                }
            }
            None => {
                let dot = commands.spawn((ChildOf(layer), abs(pos, Vec2::splat(DOT)), ImageNode::new(handle))).id();
                dots.0.insert(e, dot);
            }
        }
    }
    dots.0.retain(|unit, dot| {
        let keep = seen.contains(unit);
        if !keep {
            commands.entity(*dot).try_despawn();
        }
        keep
    });
}
