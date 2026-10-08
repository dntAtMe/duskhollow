//! `.psi` particle systems: the client's `ParticleSystem` rules (`dusk_formats::psi`)
//! simulated on the CPU and drawn as one quad mesh per emitter, like the original's
//! SFML vertex array. Additive systems use a material with SFML's `BlendAdd`
//! (src alpha, one), the rest plain alpha blending.
//!
//! Owners spawn a [`ParticleEmitter`] and keep its `pos` up to date; the emitter despawns
//! itself once stopped and empty. Like the original (which only updates systems while
//! their sprite is drawn), emitters away from the camera are neither simulated nor drawn.

use crate::data::GameData;
use crate::iso;
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::mesh::{Indices, MeshVertexBufferLayoutRef, PrimitiveTopology};
use bevy::prelude::*;
use bevy::render::render_resource::{
    AsBindGroup, BlendComponent, BlendFactor, BlendOperation, BlendState, RenderPipelineDescriptor,
    SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dKey, Material2dPlugin};
use dusk_formats::psi::{CELL, ParticleSystem, ParticleSystemInfo};
use std::collections::HashMap;

pub struct ParticlesPlugin;

impl Plugin for ParticlesPlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "particles.wgsl");
        app.add_plugins(Material2dPlugin::<FxMaterial>::default())
            .init_resource::<PsiCache>()
            .add_systems(Startup, init_materials)
            .add_systems(PostUpdate, (debug_camera, init_emitters, simulate_emitters).chain());
    }
}

/// Textured, vertex-coloured mesh with SFML-style blending: `BlendAdd` or `BlendAlpha`.
/// Used for particles and for the `light_source.png` glows.
#[derive(Asset, TypePath, AsBindGroup, Clone)]
#[bind_group_data(FxMaterialKey)]
pub struct FxMaterial {
    #[texture(0)]
    #[sampler(1)]
    pub texture: Handle<Image>,
    pub additive: bool,
}

#[repr(C)]
#[derive(Eq, PartialEq, Hash, Copy, Clone)]
pub struct FxMaterialKey {
    additive: bool,
}

impl From<&FxMaterial> for FxMaterialKey {
    fn from(m: &FxMaterial) -> Self {
        Self { additive: m.additive }
    }
}

impl Material2d for FxMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dusk_client/particles.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }

    fn specialize(
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        key: Material2dKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        if key.bind_group_data.additive
            && let Some(fragment) = descriptor.fragment.as_mut()
        {
            // sf::BlendAdd = (SrcAlpha, One, Add) for colour, (One, One, Add) for alpha.
            let add = |src_factor| BlendComponent {
                src_factor,
                dst_factor: BlendFactor::One,
                operation: BlendOperation::Add,
            };
            for target in fragment.targets.iter_mut().flatten() {
                target.blend = Some(BlendState { color: add(BlendFactor::SrcAlpha), alpha: add(BlendFactor::One) });
            }
        }
        Ok(())
    }
}

/// Shared materials over `particles.png`.
#[derive(Resource)]
pub struct FxMaterials {
    pub particles_add: Handle<FxMaterial>,
    pub particles_alpha: Handle<FxMaterial>,
}

fn init_materials(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut materials: ResMut<Assets<FxMaterial>>,
) {
    let texture: Handle<Image> =
        data.asset_path(dusk_formats::psi::TEXTURE).map(|p| assets.load(p)).unwrap_or_default();
    commands.insert_resource(FxMaterials {
        particles_add: materials.add(FxMaterial { texture: texture.clone(), additive: true }),
        particles_alpha: materials.add(FxMaterial { texture, additive: false }),
    });
}

/// Parsed `.psi` files by lowercase name (`None` = missing/broken, warned once).
#[derive(Resource, Default)]
struct PsiCache(HashMap<String, Option<ParticleSystemInfo>>);

/// A particle system in the world. Positions are Bevy world coordinates.
#[derive(Component)]
pub struct ParticleEmitter {
    pub psi: String,
    /// Emitter location.
    pub pos: Vec2,
    pub z: f32,
    /// Live particles follow the emitter (`setPosition(true, ..)`: sprite/unit effects)
    /// instead of staying where they were emitted (projectile trails).
    pub attached: bool,
    /// Stop emitting; the entity despawns once its last particle died.
    pub stopped: bool,
    sys: Option<ParticleSystem>,
}

impl ParticleEmitter {
    pub fn new(psi: impl Into<String>, pos: Vec2, z: f32, attached: bool) -> Self {
        Self { psi: psi.into(), pos, z, attached, stopped: false, sys: None }
    }
}

/// Bundle-ish helper: everything an emitter entity needs.
pub fn emitter(psi: impl Into<String>, pos: Vec2, z: f32, attached: bool) -> impl Bundle {
    (ParticleEmitter::new(psi, pos, z, attached), Transform::from_xyz(0.0, 0.0, z), Visibility::Hidden)
}

/// Debug aid: `DUSK_CAMERA_CELL=x,y` pins the camera on a map cell (to look at effects).
pub fn debug_camera(
    mut camera: Query<&mut Transform, With<crate::player::MainCamera>>,
    mut cell: Local<Option<Option<Vec2>>>,
) {
    let cell = *cell.get_or_insert_with(|| {
        let v = std::env::var("DUSK_CAMERA_CELL").ok()?;
        let (x, y) = v.split_once(',')?;
        Some(Vec2::new(x.trim().parse().ok()?, y.trim().parse().ok()?))
    });
    if let (Some(c), Ok(mut t)) = (cell, camera.single_mut()) {
        let p = iso::to_screen(c + 0.5);
        t.translation = p.extend(t.translation.z);
    }
}

/// Bevy world -> the system's y-down pixel space.
fn to_sys(p: Vec2) -> [f32; 2] {
    [p.x, -p.y]
}

#[allow(clippy::too_many_arguments)]
fn init_emitters(
    mut commands: Commands,
    data: Res<GameData>,
    mats: Option<Res<FxMaterials>>,
    mut cache: ResMut<PsiCache>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut q: Query<(Entity, &mut ParticleEmitter), Added<ParticleEmitter>>,
    mut seed: Local<u64>,
) {
    let Some(mats) = mats else { return };
    for (e, mut em) in &mut q {
        let key = em.psi.to_lowercase();
        let info = *cache.0.entry(key).or_insert_with(|| {
            let path = data.root.join("scripts/particles").join(&em.psi);
            ParticleSystemInfo::load(&path).map_err(|err| warn!("{}: {err}", path.display())).ok()
        });
        let Some(info) = info else {
            commands.entity(e).despawn();
            continue;
        };
        *seed += 1;
        let mut sys = ParticleSystem::new(info, *seed);
        let p = to_sys(em.pos);
        sys.set_position(p[0], p[1], em.attached);
        em.sys = Some(sys);
        let material = if info.additive() { mats.particles_add.clone() } else { mats.particles_alpha.clone() };
        commands.entity(e).insert((
            Mesh2d(meshes.add(empty_mesh())),
            MeshMaterial2d(material),
            // Vertices are rewritten every frame; the AABB would go stale.
            NoFrustumCulling,
        ));
    }
}

fn empty_mesh() -> Mesh {
    // One degenerate quad so the GPU buffers are never empty.
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, vec![[0.0f32; 3]; 4]);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0f32; 2]; 4]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![[0.0f32; 4]; 4]);
    m.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    m
}

/// Visible world rectangle (with a margin) of the 2D camera.
pub fn camera_view(
    camera: &Query<(&Transform, &Projection), With<crate::player::MainCamera>>,
    window: &Query<&Window>,
    margin: f32,
) -> Option<Rect> {
    let (t, proj) = camera.single().ok()?;
    let w = window.single().ok()?;
    let scale = match proj {
        Projection::Orthographic(o) => o.scale,
        _ => 1.0,
    };
    let half = Vec2::new(w.width(), w.height()) * scale / 2.0 + Vec2::splat(margin);
    let c = t.translation.truncate();
    Some(Rect::from_corners(c - half, c + half))
}

#[allow(clippy::too_many_arguments)]
fn simulate_emitters(
    mut commands: Commands,
    time: Res<Time>,
    mut meshes: ResMut<Assets<Mesh>>,
    camera: Query<(&Transform, &Projection), With<crate::player::MainCamera>>,
    window: Query<&Window>,
    mut q: Query<
        (Entity, &mut ParticleEmitter, &Mesh2d, &mut Visibility, &mut Transform),
        Without<crate::player::MainCamera>,
    >,
) {
    let dt = time.delta_secs().min(0.1);
    // Systems with large spreads (fog, smoke) reach a few hundred px from their emitter.
    let view = camera_view(&camera, &window, 400.0);
    for (e, mut em, mesh, mut vis, mut transform) in &mut q {
        let (pos, attached, stopped, z) = (em.pos, em.attached, em.stopped, em.z);
        let Some(sys) = em.sys.as_mut() else { continue };
        if view.is_some_and(|v| !v.contains(pos)) {
            if stopped {
                commands.entity(e).despawn();
            } else {
                *vis = Visibility::Hidden;
            }
            continue;
        }
        let p = to_sys(pos);
        sys.set_position(p[0], p[1], attached);
        sys.stopped = stopped;
        sys.update(dt);
        if sys.finished() {
            commands.entity(e).despawn();
            continue;
        }
        transform.translation.z = z;
        if sys.particles.is_empty() {
            *vis = Visibility::Hidden;
            continue;
        }
        *vis = Visibility::Visible;
        let Some(mut mesh) = meshes.get_mut(&mesh.0) else { continue };
        write_quads(&mut mesh, sys);
    }
}

fn write_quads(mesh: &mut Mesh, sys: &ParticleSystem) {
    let n = sys.particles.len();
    let (tx, ty) = sys.info.texture_origin();
    let uv0 = Vec2::new(tx as f32, ty as f32) / 128.0;
    let uv1 = uv0 + Vec2::splat(CELL / 128.0);
    let (mut pos, mut uv, mut col, mut idx) =
        (Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 4), Vec::with_capacity(n * 6));
    for (i, p) in sys.particles.iter().enumerate() {
        // Quad of side size*32 centred on the particle (spin is not rendered by the client).
        let h = p.size * CELL * 0.5;
        let (x, y) = (p.pos[0], -p.pos[1]);
        pos.extend([[x - h, y + h, 0.0], [x + h, y + h, 0.0], [x + h, y - h, 0.0], [x - h, y - h, 0.0]]);
        uv.extend([[uv0.x, uv0.y], [uv1.x, uv0.y], [uv1.x, uv1.y], [uv0.x, uv1.y]]);
        let [r, g, b, a] = p.rgba8();
        let c = Color::srgba_u8(r, g, b, a).to_linear().to_f32_array();
        col.extend([c; 4]);
        let k = i as u32 * 4;
        idx.extend([k, k + 1, k + 2, k, k + 2, k + 3]);
    }
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, pos);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uv);
    mesh.insert_attribute(Mesh::ATTRIBUTE_COLOR, col);
    mesh.insert_indices(Indices::U32(idx));
}

/// A single textured quad (`size` px, centred on the origin) with a vertex colour.
pub fn quad_mesh(size: Vec2, color: Color) -> Mesh {
    let h = size / 2.0;
    let c = color.to_linear().to_f32_array();
    let mut m = Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default());
    m.insert_attribute(
        Mesh::ATTRIBUTE_POSITION,
        vec![[-h.x, h.y, 0.0], [h.x, h.y, 0.0], [h.x, -h.y, 0.0], [-h.x, -h.y, 0.0]],
    );
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, vec![c; 4]);
    m.insert_indices(Indices::U32(vec![0, 1, 2, 0, 2, 3]));
    m
}

/// Position (Bevy world) and draw depth of a map sprite's `sprite_psi` emitter:
/// `sprite position - hotspot + offset` in the original's y-down pixels.
pub fn sprite_emitter_pos(sprite_pos: Vec2, hotspot: Vec2, offset: IVec2) -> Vec2 {
    Vec2::new(sprite_pos.x - hotspot.x + offset.x as f32, sprite_pos.y + hotspot.y - offset.y as f32)
}

/// Resolves a spell visual kit's psystem offset expression (`-20`, `-height/2`, ...).
pub fn kit_offset(expr: &str, unit_height: f32) -> f32 {
    use dusk_formats::spell::{FormulaVars, eval_formula};
    if expr.trim().is_empty() {
        return 0.0;
    }
    let expr = expr.replace("height", &format!("({unit_height})"));
    eval_formula(&expr, &FormulaVars::default()).unwrap_or(0.0) as f32
}

/// Feet position + kit offset (y-down px) in Bevy world space.
pub fn kit_pos(feet_cell: Vec2, x: &str, y: &str, unit_height: f32) -> Vec2 {
    iso::to_screen(feet_cell) + Vec2::new(kit_offset(x, unit_height), -kit_offset(y, unit_height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offsets() {
        assert_eq!(kit_offset("-height", 60.0), -60.0);
        assert_eq!(kit_offset("-height/2", 60.0), -30.0);
        assert_eq!(kit_offset("", 60.0), 0.0);
        assert_eq!(kit_offset("-20", 60.0), -20.0);
        // Emitter 33 px right / 14 px down from the image's top-left; hotspot (40, 60).
        let p = sprite_emitter_pos(Vec2::new(100.0, 50.0), Vec2::new(40.0, 60.0), IVec2::new(33, 14));
        assert_eq!(p, Vec2::new(93.0, 96.0));
    }
}
