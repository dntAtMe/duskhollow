//! The Eye's gaze as seen by the client: strain, cover and eye state of the local player
//! (docs/demo-plan.md). Written from server messages, read by the HUD, visuals and audio.
//!
//! Visuals:
//! - **Crimson grade** (`gaze_grade.wgsl`): a camera-sized quad that tints the world
//!   (alpha-blended tint): crimson from above on open ground, bruised violet-black under shade, darkest in
//!   deep shelter, warm holes around lights and cairns, dithered between cover zones. The cover
//!   grid comes from the map's `.cover` sidecar (also read by the server).
//! - **Gaze spot**: a slow pale-crimson wash with a faint ring at the server's spot position,
//!   drawn by the same shader in cell space.
//! - **Strain HUD** under the player frame, an Eye glyph on the minimap, a red vignette that
//!   beats with strain, a grey veil when overwhelmed and a sky flare when the Eye opens.

use crate::{
    audio::PlaySfx,
    combat_ui::UiFont,
    data::GameData,
    map_render::{CurrentMap, MapLoaded},
    particles,
    player::MainCamera,
};
use bevy::asset::RenderAssetUsages;
use bevy::camera::visibility::NoFrustumCulling;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, Extent3d, ShaderType, TextureDimension, TextureFormat};
use bevy::shader::ShaderRef;
use bevy::sprite_render::{AlphaMode2d, Material2d, Material2dPlugin};
use dusk_formats::custom::{Cover, CoverGrid};
use dusk_protocol::ServerMsg;

pub struct GazePlugin;

impl Plugin for GazePlugin {
    fn build(&self, app: &mut App) {
        bevy::asset::embedded_asset!(app, "gaze_grade.wgsl");
        app.init_resource::<GazeView>()
            .add_message::<GazeNet>()
            .add_plugins(Material2dPlugin::<GradeMaterial>::default())
            .add_systems(Startup, (spawn_grade, spawn_hud.after(crate::combat_ui::load_font)))
            .add_systems(Update, (apply_net, load_cover, update_hud, update_screen_fx).chain())
            .add_systems(PostUpdate, update_grade.after(particles::debug_camera));
    }
}

/// The Eye's lid, broadcast by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EyeState {
    #[default]
    Lidded,
    Opening,
    Open,
    Closing,
}

/// Cover under the local player (`.cover` sidecar of the map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverKind {
    #[default]
    Open,
    Shade,
    Shelter,
    Cairn,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct GazeView {
    /// False on maps without a `.cover` sidecar: no strain, no crimson grade.
    pub active: bool,
    pub eye: EyeState,
    /// 0 = half-lidded, 1 = wide open (smoothed for visuals).
    pub openness: f32,
    /// 0..=100.
    pub strain: f32,
    /// 0..=100.
    pub corruption: f32,
    pub cover: CoverKind,
    pub in_combat: bool,
    /// Inside the wandering gaze spot.
    pub in_spot: bool,
    /// Openness as last sent by the server (unsmoothed).
    pub target_openness: f32,
    /// Centre of the wandering gaze spot (cells).
    pub spot: Option<Vec2>,
    pub spot_radius: f32,
    /// Cairn the player is bound to (respawn point), cell centre.
    pub bound_cairn: Option<Vec2>,
    /// 1 right when the Eye starts opening, decays to 0 (sky flare).
    pub flare: f32,
    /// Seconds since the last `Eye` message; the view goes inactive after a while.
    pub since_eye: f32,
}

impl GazeView {
    pub fn weary(&self) -> bool {
        self.strain >= 40.0
    }

    pub fn gaze_sick(&self) -> bool {
        self.strain >= 70.0
    }

    pub fn overwhelmed(&self) -> bool {
        self.strain >= 100.0
    }

    /// "Weary" / "Gaze-sick" / "Overwhelmed", or empty.
    pub fn state_label(&self) -> &'static str {
        if self.overwhelmed() {
            "Overwhelmed"
        } else if self.gaze_sick() {
            "Gaze-sick"
        } else if self.weary() {
            "Weary"
        } else {
            ""
        }
    }
}

/// Gaze server messages (`Eye`, `Gaze`, `CairnBound`), forwarded by `net`.
#[derive(Message, Clone)]
pub struct GazeNet(pub ServerMsg);

/// Without an `Eye` message for this long the gaze is considered inactive (map change).
const STALE_SECS: f32 = 2.0;

fn apply_net(
    time: Res<Time>,
    mut msgs: MessageReader<GazeNet>,
    mut view: ResMut<GazeView>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    let dt = time.delta_secs();
    view.since_eye += dt;
    for GazeNet(msg) in msgs.read() {
        match *msg {
            ServerMsg::Eye { state, openness, spot, spot_radius } => {
                let eye = match state {
                    dusk_protocol::EyeState::Lidded => EyeState::Lidded,
                    dusk_protocol::EyeState::Opening => EyeState::Opening,
                    dusk_protocol::EyeState::Open => EyeState::Open,
                    dusk_protocol::EyeState::Closing => EyeState::Closing,
                };
                if view.active && eye != view.eye {
                    match eye {
                        EyeState::Opening => {
                            sfx.write(PlaySfx::ui("gaze_open.wav"));
                            view.flare = 1.0;
                        }
                        EyeState::Closing => {
                            sfx.write(PlaySfx::ui("gaze_close.wav"));
                        }
                        _ => {}
                    }
                }
                if !view.active {
                    view.openness = openness; // no fade when entering a map
                }
                view.active = true;
                view.since_eye = 0.0;
                view.eye = eye;
                view.target_openness = openness;
                view.spot = spot.map(|p| Vec2::new(p.x, p.y));
                view.spot_radius = spot_radius;
            }
            ServerMsg::Gaze { strain, corruption, cover, in_combat, in_spot } => {
                if view.active && in_spot && !view.in_spot {
                    sfx.write(PlaySfx::ui("gaze_spot_enter.wav"));
                }
                if view.active && strain >= 100.0 && view.strain < 100.0 {
                    sfx.write(PlaySfx::ui("strain_overwhelm.wav"));
                }
                view.strain = strain;
                view.corruption = corruption;
                view.cover = match cover {
                    dusk_protocol::GazeCover::Open => CoverKind::Open,
                    dusk_protocol::GazeCover::Shade => CoverKind::Shade,
                    dusk_protocol::GazeCover::Shelter => CoverKind::Shelter,
                    dusk_protocol::GazeCover::Cairn => CoverKind::Cairn,
                };
                view.in_combat = in_combat;
                view.in_spot = in_spot;
            }
            ServerMsg::CairnBound { pos } => {
                sfx.write(PlaySfx::ui("cairn_kindle.wav"));
                view.bound_cairn = Some(Vec2::new(pos.x, pos.y));
            }
            _ => {}
        }
    }
    if view.active && view.since_eye > STALE_SECS {
        let keep = view.bound_cairn;
        *view = GazeView { bound_cairn: keep, ..default() };
    }
    let k = 1.0 - (-dt * 4.0).exp();
    view.openness += (view.target_openness - view.openness) * k;
    view.flare = (view.flare - dt / 3.5).max(0.0);
}

// ---------------------------------------------------------------- crimson grade

/// Above upright sprites (depth <= ~1 + map size), below nameplates (~500) and ground glows.
const GRADE_Z: f32 = 450.0;
/// Lights cut warm holes into the grade (nearest first).
pub const MAX_GRADE_LIGHTS: usize = 32;

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct GradeMaterial {
    #[uniform(0)]
    pub data: GradeUniform,
    #[texture(1)]
    #[sampler(2)]
    pub cover: Handle<Image>,
}

#[derive(ShaderType, Clone)]
pub struct GradeUniform {
    /// x: strength (fade in/out), y: openness, z: flare, w: time (s).
    pub params: Vec4,
    /// x, y: cover grid size (cells), z: light count, w: 1 if a cover grid is bound.
    pub map: Vec4,
    /// xy: spot centre (cells), z: radius (cells), w: spot visibility.
    pub spot: Vec4,
    /// xy: light centre (world), z: scale.
    pub lights: [Vec4; MAX_GRADE_LIGHTS],
}

impl Material2d for GradeMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://dusk_client/gaze_grade.wgsl".into()
    }

    fn alpha_mode(&self) -> AlphaMode2d {
        AlphaMode2d::Blend
    }
}

#[derive(Component)]
struct GradeOverlay;

/// The cover grid of the current map, uploaded as a texture for the grade.
#[derive(Resource, Default)]
struct CoverTexture {
    map: String,
    size: Vec2,
    image: Option<Handle<Image>>,
}

fn spawn_grade(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<GradeMaterial>>,
    mut images: ResMut<Assets<Image>>,
) {
    let blank = images.add(cover_image(&CoverGrid { width: 1, height: 1, cells: vec![Cover::Open] }));
    let material = materials.add(GradeMaterial {
        data: GradeUniform {
            params: Vec4::ZERO,
            map: Vec4::new(1.0, 1.0, 0.0, 0.0),
            spot: Vec4::ZERO,
            lights: [Vec4::ZERO; MAX_GRADE_LIGHTS],
        },
        cover: blank,
    });
    commands.init_resource::<CoverTexture>();
    commands.spawn((
        GradeOverlay,
        Mesh2d(meshes.add(Rectangle::new(1.0, 1.0))),
        MeshMaterial2d(material),
        Transform::from_xyz(0.0, 0.0, GRADE_Z),
        Visibility::Hidden,
        NoFrustumCulling,
        crate::minimap::overlay_layer(),
    ));
}

/// RGBA8 (linear), one texel per cell: r = shade, g = deep shelter, b = cairn warmth.
fn cover_image(grid: &CoverGrid) -> Image {
    let (w, h) = (grid.width.max(1), grid.height.max(1));
    let cairns = grid.cairns();
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let c = grid.get(x as i32, y as i32);
            let (cx, cy) = (x as f32 + 0.5, y as f32 + 0.5);
            let warm = cairns
                .iter()
                .map(|&(ax, ay)| (1.0 - ((ax - cx).powi(2) + (ay - cy).powi(2)).sqrt() / 4.0).clamp(0.0, 1.0))
                .fold(0.0f32, f32::max);
            let i = (y * w + x) * 4;
            px[i] = if c != Cover::Open { 255 } else { 0 };
            px[i + 1] = if matches!(c, Cover::Shelter) { 255 } else { 0 };
            px[i + 2] = (warm * 255.0) as u8;
            px[i + 3] = 255;
        }
    }
    let mut image = Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    image
}

/// Reads `maps/<name>.cover` whenever a map finishes loading.
fn load_cover(
    data: Res<GameData>,
    map: Res<CurrentMap>,
    mut loaded: MessageReader<MapLoaded>,
    mut cover: ResMut<CoverTexture>,
    mut images: ResMut<Assets<Image>>,
    overlay: Query<&MeshMaterial2d<GradeMaterial>, With<GradeOverlay>>,
    mut materials: ResMut<Assets<GradeMaterial>>,
) {
    if loaded.read().count() == 0 || cover.map == map.name {
        return;
    }
    cover.map = map.name.clone();
    let grid = std::fs::read_to_string(data.root.join("maps").join(format!("{}.cover", map.name)))
        .ok()
        .and_then(|t| CoverGrid::parse(&t));
    let Ok(mat) = overlay.single() else { return };
    let Some(mut m) = materials.get_mut(&mat.0) else { return };
    match grid {
        Some(g) => {
            cover.size = Vec2::new(g.width as f32, g.height as f32);
            let handle = images.add(cover_image(&g));
            m.cover = handle.clone();
            cover.image = Some(handle);
            m.data.map = Vec4::new(cover.size.x, cover.size.y, 0.0, 1.0);
        }
        None => {
            cover.image = None;
            m.data.map.w = 0.0;
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn update_grade(
    time: Res<Time>,
    view: Res<GazeView>,
    mut strength: Local<f32>,
    camera: Query<(&Transform, &Projection), With<MainCamera>>,
    window: Query<&Window>,
    holes: Query<(&crate::lights::DarknessHole, &Transform), Without<GradeOverlay>>,
    mut overlay: Query<
        (&mut Transform, &mut Visibility, &MeshMaterial2d<GradeMaterial>),
        (With<GradeOverlay>, Without<MainCamera>),
    >,
    mut materials: ResMut<Assets<GradeMaterial>>,
) {
    let target = if view.active { 1.0 } else { 0.0 };
    *strength += (target - *strength) * (1.0 - (-time.delta_secs() * 2.0).exp());
    let Ok((mut t, mut vis, mat)) = overlay.single_mut() else { return };
    if *strength < 0.01 && !view.active {
        *vis = Visibility::Hidden;
        return;
    }
    let Some(view_rect) = particles::camera_view(&camera, &window, 2.0) else { return };
    *vis = Visibility::Visible;
    t.translation = view_rect.center().extend(GRADE_Z);
    t.scale = view_rect.size().extend(1.0);
    let Some(mut m) = materials.get_mut(&mat.0) else { return };
    let mut lights: Vec<(f32, Vec4)> = holes
        .iter()
        .map(|(h, t)| (t.translation.truncate(), h.scale))
        .filter(|(p, s)| view_rect.inflate(400.0 * s).contains(*p))
        .map(|(p, s)| (p.distance_squared(view_rect.center()), Vec4::new(p.x, p.y, s, 0.0)))
        .collect();
    lights.sort_by(|a, b| a.0.total_cmp(&b.0));
    lights.truncate(MAX_GRADE_LIGHTS);
    m.data.params = Vec4::new(*strength, view.openness, view.flare, time.elapsed_secs() % 1000.0);
    m.data.map.z = lights.len() as f32;
    m.data.spot = match view.spot {
        Some(s) => Vec4::new(s.x, s.y, view.spot_radius.max(0.1), 1.0),
        None => Vec4::ZERO,
    };
    for (i, (_, l)) in lights.into_iter().enumerate() {
        m.data.lights[i] = l;
    }
}

// ---------------------------------------------------------------- HUD

/// Strain widget (top-left), under the player frame and its aura row.
const STRAIN_POS: Vec2 = Vec2::new(8.0, 148.0);
/// Fill area inside `gaze_strain_frame.png`.
const GAZE_FILL: (Vec2, Vec2) = (Vec2::new(4.0, 4.0), Vec2::new(188.0, 8.0));
const FRAME_AT: Vec2 = Vec2::new(36.0, 2.0);
/// Eye glyph on the minimap frame: left end of its title band (offset from the screen's top-right).
const MINIMAP_EYE: Vec2 = Vec2::new(163.0, 13.0);

#[derive(Component)]
struct StrainRoot;
#[derive(Component)]
struct StrainClip;
#[derive(Component)]
struct CorruptionClip;
#[derive(Component)]
struct StrainLabel;
#[derive(Component)]
struct StrainEye;
#[derive(Component)]
struct MinimapEye;
#[derive(Component)]
struct Vignette;
#[derive(Component)]
struct Veil;
#[derive(Component)]
struct Flare;

/// Eye glyphs: lidded, half, open.
#[derive(Resource)]
struct EyeArt([Handle<Image>; 3]);

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

fn full_screen() -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    }
}

fn spawn_hud(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    mut images: ResMut<Assets<Image>>,
) {
    let img = |name: &str| data.asset_path(name).map(|p| assets.load(p)).unwrap_or_default();
    let art = EyeArt(["gaze_eye_lidded.png", "gaze_eye_half.png", "gaze_eye_open.png"].map(img));

    // Screen feel first (lowest), all under the HUD.
    commands.spawn((
        full_screen(),
        ImageNode::new(images.add(vignette_image())).with_color(Color::NONE),
        GlobalZIndex(-3),
        Pickable::IGNORE,
        Vignette,
    ));
    commands.spawn((full_screen(), BackgroundColor(Color::NONE), GlobalZIndex(-2), Pickable::IGNORE, Veil));
    commands.spawn((full_screen(), BackgroundColor(Color::NONE), GlobalZIndex(-1), Pickable::IGNORE, Flare));

    let text_font = TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() };
    commands.spawn((abs(STRAIN_POS, Vec2::new(340.0, 26.0)), Visibility::Hidden, StrainRoot)).with_children(|p| {
        p.spawn((abs(Vec2::ZERO, Vec2::new(32.0, 20.0)), ImageNode::new(art.0[0].clone()), StrainEye));
        p.spawn((abs(FRAME_AT, Vec2::new(196.0, 16.0)), ImageNode::new(img("gaze_strain_frame.png"))));
        let fill_at = FRAME_AT + GAZE_FILL.0;
        let mut clip = abs(fill_at, GAZE_FILL.1);
        clip.overflow = Overflow::clip();
        p.spawn((clip, StrainClip))
            .with_child((abs(Vec2::ZERO, GAZE_FILL.1), ImageNode::new(img("gaze_strain_fill.png"))));
        // Threshold ticks at 40 / 70.
        for t in [0.4, 0.7] {
            let x = fill_at.x + (GAZE_FILL.1.x * t).round() - 1.0;
            p.spawn((abs(Vec2::new(x, fill_at.y - 3.0), Vec2::new(2.0, 14.0)), BackgroundColor(TICK)));
        }
        // Corruption: thin bar under the frame.
        let corr_at = Vec2::new(fill_at.x, FRAME_AT.y + 17.0);
        p.spawn((abs(corr_at - Vec2::ONE, Vec2::new(GAZE_FILL.1.x + 2.0, 5.0)), BackgroundColor(WELL)));
        let mut clip = abs(corr_at, Vec2::new(0.0, 3.0));
        clip.overflow = Overflow::clip();
        p.spawn((clip, CorruptionClip)).with_child((
            abs(Vec2::ZERO, Vec2::new(GAZE_FILL.1.x, 3.0)),
            ImageNode::new(img("gaze_corruption_fill.png")),
        ));
        p.spawn(abs(Vec2::new(FRAME_AT.x + 202.0, 0.0), Vec2::new(110.0, 20.0))).with_child((
            Text::new(""),
            text_font,
            TextColor(Color::srgb(0.85, 0.6, 0.35)),
            TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.9) },
            StrainLabel,
        ));
    });
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(MINIMAP_EYE.x),
            top: Val::Px(MINIMAP_EYE.y),
            width: Val::Px(32.0),
            height: Val::Px(20.0),
            ..default()
        },
        ImageNode::new(art.0[0].clone()),
        GlobalZIndex(5),
        Visibility::Hidden,
        MinimapEye,
    ));
    commands.insert_resource(art);
}

const TICK: Color = Color::srgb(0.76, 0.66, 0.5);
const WELL: Color = Color::srgb(0.03, 0.02, 0.02);

/// Dithered radial vignette (alpha only, tinted by the node colour), 320x180 like an
/// oldschool framebuffer so the dither pattern reads at 1280x720.
fn vignette_image() -> Image {
    const BAYER: [[f32; 4]; 4] =
        [[0.0, 8.0, 2.0, 10.0], [12.0, 4.0, 14.0, 6.0], [3.0, 11.0, 1.0, 9.0], [15.0, 7.0, 13.0, 5.0]];
    let (w, h) = (320u32, 180u32);
    let mut px = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
            let d = (u * u * 0.8 + v * v).sqrt();
            let a = ((d - 0.55) / 0.75).clamp(0.0, 1.0).powf(1.4);
            // Quantise to 6 levels with ordered dither.
            let b = (BAYER[(y % 4) as usize][(x % 4) as usize] + 0.5) / 16.0;
            let q = ((a * 6.0 + b - 0.5).round() / 6.0).clamp(0.0, 1.0);
            let i = ((y * w + x) * 4) as usize;
            px[i..i + 4].copy_from_slice(&[255, 255, 255, (q * 255.0) as u8]);
        }
    }
    Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        px,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[allow(clippy::type_complexity)]
fn update_hud(
    time: Res<Time>,
    view: Res<GazeView>,
    art: Option<Res<EyeArt>>,
    mut roots: Query<&mut Visibility, (With<StrainRoot>, Without<MinimapEye>)>,
    mut clips: Query<(&mut Node, Has<CorruptionClip>), Or<(With<StrainClip>, With<CorruptionClip>)>>,
    mut label: Query<(&mut Text, &mut TextColor), With<StrainLabel>>,
    mut eyes: Query<
        (&mut ImageNode, Has<MinimapEye>, Option<&mut Visibility>),
        (Or<(With<StrainEye>, With<MinimapEye>)>, Without<StrainRoot>),
    >,
) {
    let Some(art) = art else { return };
    let shown = if view.active { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut roots {
        v.set_if_neq(shown);
    }
    for (mut node, corruption) in &mut clips {
        let ratio = if corruption { view.corruption } else { view.strain } / 100.0;
        node.width = Val::Px((GAZE_FILL.1.x * ratio.clamp(0.0, 1.0)).round());
    }
    let t = time.elapsed_secs();
    for (mut text, mut color) in &mut label {
        let s = view.state_label();
        if text.0 != s {
            text.0 = s.to_string();
        }
        color.0 = if view.overwhelmed() {
            let p = 0.5 + 0.5 * (t * 6.0).sin();
            Color::srgb(0.85 + 0.15 * p, 0.2 + 0.1 * p, 0.15)
        } else if view.gaze_sick() {
            Color::srgb(0.85, 0.28, 0.2)
        } else {
            Color::srgb(0.85, 0.62, 0.35)
        };
    }
    let frame = match view.eye {
        EyeState::Lidded => 0,
        EyeState::Opening | EyeState::Closing => 1,
        EyeState::Open => 2,
    };
    for (mut img, minimap, vis) in &mut eyes {
        if img.image != art.0[frame] {
            img.image = art.0[frame].clone();
        }
        // The glyph by the bar watches harder as strain rises; the minimap one breathes when open.
        let glow = if minimap { view.openness } else { (view.strain / 100.0).clamp(0.0, 1.0) };
        let pulse = 0.85 + 0.15 * glow * (t * 2.5).sin();
        img.color = Color::srgb(pulse, pulse * (1.0 - 0.15 * glow), pulse * (1.0 - 0.15 * glow));
        if let Some(mut v) = vis.filter(|_| minimap) {
            v.set_if_neq(shown);
        }
    }
}

/// Heartbeat: two thumps per beat, faster when overwhelmed.
fn heartbeat(t: f32, bpm: f32) -> f32 {
    let phase = (t * bpm / 60.0).fract() * 60.0 / bpm;
    let thump = |at: f32| (-((phase - at) / 0.07).powi(2)).exp();
    thump(0.0) + 0.6 * thump(0.24)
}

#[allow(clippy::type_complexity)]
fn update_screen_fx(
    time: Res<Time>,
    view: Res<GazeView>,
    mut vignette: Query<&mut ImageNode, With<Vignette>>,
    mut veil: Query<&mut BackgroundColor, (With<Veil>, Without<Flare>)>,
    mut flare: Query<&mut BackgroundColor, (With<Flare>, Without<Veil>)>,
    mut veil_amount: Local<f32>,
) {
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    let s = if view.active { view.strain } else { 0.0 };
    // Base: creeps in from 30, solid by 70; heartbeat on top from 70.
    let mut a = ((s - 30.0) / 40.0).clamp(0.0, 1.0) * 0.45;
    if s >= 70.0 {
        let bpm = if s >= 100.0 { 96.0 } else { 70.0 + (s - 70.0) * 0.6 };
        a += 0.3 * heartbeat(t, bpm);
    }
    if s >= 100.0 {
        a += 0.15;
    }
    // Corruption above 50 bruises the vignette towards violet.
    let bruise = if view.active { ((view.corruption - 50.0) / 50.0).clamp(0.0, 1.0) } else { 0.0 };
    let base = Color::srgb(0.55 - 0.2 * bruise, 0.02, 0.04 + 0.25 * bruise);
    for mut img in &mut vignette {
        img.color = base.with_alpha(a.clamp(0.0, 0.9));
    }
    let want = if view.active && view.overwhelmed() { 1.0 } else { 0.0 };
    *veil_amount += (want - *veil_amount) * (1.0 - (-dt * 1.5).exp());
    for mut c in &mut veil {
        // Grey veil reads as a slight desaturation.
        c.0 = Color::srgb(0.3, 0.28, 0.28).with_alpha(0.24 * *veil_amount);
    }
    for mut c in &mut flare {
        let f = if view.active { view.flare } else { 0.0 };
        c.0 = Color::srgb(0.95, 0.5, 0.45).with_alpha(0.45 * f * f.sqrt());
    }
}
