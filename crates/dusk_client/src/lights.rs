//! Map sprite effects (`sprite_fx.txt` particle emitters and lights) and the map darkness
//! (`docs/visuals.md`).
//!
//! - Each light of a sprite sits at the cell's render position + offset.
//!   `top`: [`GLOW`] (centred, scaled, tinted with the light colour, additive) drawn in the
//!   sprite's depth slot just before the sprite. `ground`: the same glow at +(16, 8) px, drawn
//!   above the whole upright layer.
//! - If the map brightness is below 1, a camera-sized quad of black with alpha
//!   `1 - brightness` is drawn over the map, multiplied around every light by the alpha of
//!   [`MASK`] (1024x512, ~0 in the centre, 1 at the edges; scaled by the light's scale, at
//!   +(16, 8)). Brightness is `1 - MapInfo.darkness` of the current map (`DUSK_DARKNESS=0..1`
//!   overrides it for testing) and eases towards changes at rate 1/s.
//!
//! The darkness shader multiplies the cut-outs of up to [`MAX_LIGHTS`] visible lights.

use crate::{
    data::GameData,
    map_render::{CurrentMap, MapTile},
    particles::{self, FxMaterial},
    player::Player,
};
use bevy::camera::visibility::NoFrustumCulling;
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};

pub struct LightsPlugin;

impl Plugin for LightsPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "darkness.wgsl");
        app.add_plugins(Material2dPlugin::<DarknessMaterial>::default())
            .init_resource::<Brightness>()
            .add_systems(Startup, spawn_darkness)
            .add_systems(PostUpdate, (map_brightness, update_darkness).chain().after(particles::debug_camera));
    }
}

/// Additive light glow (tools/artgen/lightfx.py).
pub const GLOW: &str = "fx_light_glow.png";
/// Darkness cut-out around a light: alpha ~0 in the centre, 1 at the edges (tools/artgen/lightfx.py).
pub const MASK: &str = "fx_light_mask.png";

/// Lights cut into the darkness per frame (nearest to the camera first).
pub const MAX_LIGHTS: usize = 64;
/// Draw depth of `bool_applyground` glows: above all upright sprites, below the darkness.
const GROUND_GLOW_Z: f32 = 0.55;
const DARKNESS_Z: f32 = 990.0;

/// Map brightness (1 = no darkness) and its target.
#[derive(Resource)]
pub struct Brightness {
    pub current: f32,
    pub target: f32,
}

impl Default for Brightness {
    fn default() -> Self {
        Self { current: 1.0, target: 1.0 }
    }
}

/// A light's cut-out in the darkness (world position of the [`MASK`] centre).
#[derive(Component)]
pub struct DarknessHole {
    pub scale: f32,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct DarknessMaterial {
    #[uniform(0)]
    pub data: DarknessUniform,
    #[texture(1)]
    #[sampler(2)]
    pub light: Handle<Image>,
}

#[derive(ShaderType, Clone)]
pub struct DarknessUniform {
    /// x: darkness alpha (1 - brightness), y: light count, zw: [`MASK`] size.
    pub params: Vec4,
    /// xy: centre (world), z: scale.
    pub lights: [Vec4; MAX_LIGHTS],
}

impl Material2d for DarknessMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dusk_client/darkness.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Component)]
struct DarknessOverlay;

fn spawn_darkness(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<DarknessMaterial>>,
) {
    let light = data.asset_path(MASK).map(|p| assets.load(p)).unwrap_or_default();
    let size = data.image_size(MASK).unwrap_or(UVec2::new(1024, 512)).as_vec2();
    let material = materials.add(DarknessMaterial {
        data: DarknessUniform { params: Vec4::new(0.0, 0.0, size.x, size.y), lights: [Vec4::ZERO; MAX_LIGHTS] },
        light,
    });
    commands.spawn((
        DarknessOverlay,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, DARKNESS_Z),
        Visibility::Hidden,
        NoFrustumCulling,
        // Sized to the main view; keep it out of the minimap camera.
        crate::minimap::overlay_layer(),
    ));
}

/// Spawns the particle emitters and lights of one map sprite.
/// `pos` is where the sprite's hotspot is drawn (the cell centre), `z` its depth.
#[allow(clippy::too_many_arguments)]
pub fn spawn_sprite_effects(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<FxMaterial>,
    glow: &mut Option<Handle<FxMaterial>>,
    texture: &str,
    hotspot: Vec2,
    pos: Vec2,
    z: f32,
) -> usize {
    let key = texture.to_lowercase();
    let mut n = 0;
    for e in data.sprite_psi.get(&key).into_iter().flatten() {
        let at = particles::sprite_emitter_pos(pos, hotspot, IVec2::new(e.x, e.y));
        commands.spawn((MapTile, particles::emitter(e.psi.clone(), at, z + 0.0005, true)));
        n += 1;
    }
    for l in data.sprite_lights.get(&key).into_iter().flatten() {
        // Cell render position = sprite position - (0, 16) in y-down screen space.
        let at = pos + Vec2::new(l.x as f32, 16.0 - l.y as f32);
        let ground = at + Vec2::new(16.0, -8.0);
        commands.spawn((MapTile, DarknessHole { scale: l.scale }, Transform::from_xyz(ground.x, ground.y, 0.0)));
        let glow = glow
            .get_or_insert_with(|| {
                let texture = data.asset_path(GLOW).map(|p| assets.load(p)).unwrap_or_default();
                materials.add(FxMaterial { texture, additive: true })
            })
            .clone();
        let c = l.color;
        let color = Color::srgba_u8((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8);
        let size = data.image_size(GLOW).unwrap_or(UVec2::new(422, 193)).as_vec2() * l.scale;
        let mesh = meshes.add(particles::quad_mesh(size, color));
        for (on, p, z) in [(l.apply_top, at, z - 0.0005), (l.apply_ground, ground, GROUND_GLOW_Z)] {
            if on {
                commands.spawn((
                    MapTile,
                    Mesh2d(mesh.clone()),
                    MeshMaterial2d(glow.clone()),
                    Transform::from_xyz(p.x, p.y, z),
                ));
            }
        }
        n += 1;
    }
    n
}

/// Darkness of a map: `MapInfo.darkness`, or `DUSK_DARKNESS` when set.
fn map_darkness(data: &GameData, map: &str) -> f32 {
    std::env::var("DUSK_DARKNESS")
        .ok()
        .and_then(|v| v.parse().ok())
        .or_else(|| data.maps.iter().find(|m| m.name == map).map(|m| m.darkness))
        .unwrap_or(0.0)
}

/// Target brightness from the current map (once the local player is in it).
fn map_brightness(
    time: Res<Time>,
    data: Res<GameData>,
    map: Res<CurrentMap>,
    player: Query<(), With<Player>>,
    mut b: ResMut<Brightness>,
) {
    if player.single().is_ok() {
        let target = (1.0 - map_darkness(&data, &map.name)).clamp(0.0, 1.0);
        if map.is_changed() {
            b.current = target; // no fade on map change
        }
        b.target = target;
    }
    let step = time.delta_secs().min(0.1) * (b.target - b.current);
    b.current += step;
}

fn update_darkness(
    b: Res<Brightness>,
    camera: Query<(&Transform, &Projection), With<crate::player::MainCamera>>,
    window: Query<&Window>,
    holes: Query<(&DarknessHole, &Transform), Without<DarknessOverlay>>,
    mut overlay: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial2d<DarknessMaterial>),
        (With<DarknessOverlay>, Without<crate::player::MainCamera>),
    >,
    mut materials: ResMut<Assets<DarknessMaterial>>,
) {
    let Ok((mut t, mut vis, mat)) = overlay.single_mut() else { return };
    let Some(view) = particles::camera_view(&camera, &window, 2.0) else { return };
    if b.current >= 1.0 {
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Visible;
    t.translation = view.center().extend(DARKNESS_Z);
    t.scale = view.size().extend(1.0);
    let Some(mut m) = materials.get_mut(&mat.0) else { return };
    // Lights whose cut-out overlaps the view, nearest first.
    let mut lights: Vec<(f32, Vec4)> = holes
        .iter()
        .map(|(h, t)| (t.translation.truncate(), h.scale))
        .filter(|(p, s)| view.inflate(512.0 * s).contains(*p))
        .map(|(p, s)| (p.distance_squared(view.center()), Vec4::new(p.x, p.y, s, 0.0)))
        .collect();
    lights.sort_by(|a, b| a.0.total_cmp(&b.0));
    lights.truncate(MAX_LIGHTS);
    m.data.params.x = 1.0 - b.current;
    m.data.params.y = lights.len() as f32;
    for (i, (_, l)) in lights.into_iter().enumerate() {
        m.data.lights[i] = l;
    }
}
