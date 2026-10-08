//! Lighting that respects the scene. Ground overlays (the crimson grade, fire glows) are drawn
//! just above the floor, under upright sprites; walls, props and units are lit per cell
//! instead: a multiply tint from the cover they stand in, warmed by nearby fires, deepened
//! when the Eye opens. Units also carry hit flashes and fades ([`FeelColor`]) through here.

use crate::{
    data::GameData,
    gaze::GazeView,
    iso,
    lights::DarknessHole,
    map_render::{CurrentMap, MapLoaded},
    unit::{Unit, UnitLayer},
};
use bevy::prelude::*;
use dusk_formats::custom::{Cover, CoverGrid};

pub struct EnvLightPlugin;

impl Plugin for EnvLightPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<EnvLight>().add_systems(
            Update,
            (load_cover, collect_lights, tint_uprights, tint_units, fade_roofs).chain().after(crate::feel::tick_tints),
        );
    }
}

/// An upright map sprite (layer 2), lit by the cell it stands on.
#[derive(Component)]
pub struct Upright {
    pub cell: Vec2,
}

/// A roof sprite over the cells `cell ..= cell + extent`; faded while the player is beneath.
#[derive(Component)]
pub struct Roof {
    pub cell: IVec2,
    pub extent: IVec2,
}

/// DESIGN: roof opacity while the player stands under it.
const ROOF_UNDER_ALPHA: f32 = 0.45;
/// Roof pieces within this many cells of a player standing under a roof fade with it.
const ROOF_FADE_REACH: f32 = 6.0;

/// Colour the game-feel effects want on a unit (hit flash, fades); multiplied with the light.
#[derive(Component, Clone, Copy)]
pub struct FeelColor(pub LinearRgba);

/// DESIGN: multiply tints per cover kind (the vale art is already lit crimson from above).
const OPEN: Vec3 = Vec3::new(1.0, 0.94, 0.94);
const OPEN_EYE: Vec3 = Vec3::new(1.0, 0.8, 0.78);
const SHADE: Vec3 = Vec3::new(0.74, 0.64, 0.8);
const SHELTER: Vec3 = Vec3::new(0.52, 0.44, 0.58);
const FIRE: Vec3 = Vec3::new(1.0, 0.84, 0.64);
/// Fire reach in cells per unit of `sprite_light` scale.
const FIRE_REACH: f32 = 3.8;

#[derive(Resource, Default)]
struct EnvLight {
    map: String,
    grid: Option<CoverGrid>,
    /// (cell, reach in cells)
    fires: Vec<(Vec2, f32)>,
    fires_seen: usize,
    /// Openness the uprights were last tinted with (`None` = retint).
    tinted_at: Option<f32>,
}

impl EnvLight {
    fn tint(&self, view: &GazeView, cell: Vec2) -> Vec3 {
        let Some(grid) = &self.grid else { return Vec3::ONE };
        let base = match grid.get(cell.x.floor() as i32, cell.y.floor() as i32) {
            Cover::Open => OPEN.lerp(OPEN_EYE, view.openness.clamp(0.0, 1.0)),
            Cover::Shade | Cover::Cairn => SHADE,
            Cover::Shelter => SHELTER,
        };
        let warm =
            self.fires.iter().map(|(p, reach)| (1.0 - p.distance(cell) / reach).clamp(0.0, 1.0)).fold(0.0f32, f32::max);
        base.lerp(FIRE, (warm * warm * 0.9).min(1.0))
    }
}

fn load_cover(
    data: Res<GameData>,
    map: Res<CurrentMap>,
    mut loaded: MessageReader<MapLoaded>,
    mut env: ResMut<EnvLight>,
) {
    if loaded.read().count() == 0 || env.map == map.name {
        return;
    }
    env.map = map.name.clone();
    env.grid = std::fs::read_to_string(data.root.join("maps").join(format!("{}.cover", map.name)))
        .ok()
        .and_then(|t| CoverGrid::parse(&t));
    env.fires.clear();
    env.fires_seen = 0;
    env.tinted_at = None;
}

/// Fires are the map's light cut-outs; they appear a frame after the map loads.
fn collect_lights(holes: Query<(&DarknessHole, &Transform)>, mut env: ResMut<EnvLight>) {
    let n = holes.iter().len();
    if n == env.fires_seen {
        return;
    }
    env.fires_seen = n;
    env.fires = holes.iter().map(|(h, t)| (iso::to_cell(t.translation.truncate()), FIRE_REACH * h.scale)).collect();
    env.tinted_at = None;
}

fn tint_uprights(view: Res<GazeView>, mut env: ResMut<EnvLight>, mut sprites: Query<(&Upright, &mut Sprite)>) {
    let lit = view.active && env.grid.is_some();
    let key = if lit { view.openness } else { -1.0 };
    if env.tinted_at.is_some_and(|k| (k - key).abs() < 0.02) {
        return;
    }
    env.tinted_at = Some(key);
    for (u, mut s) in &mut sprites {
        let c = if lit { env.tint(&view, u.cell) } else { Vec3::ONE };
        s.color = Color::linear_rgb(c.x, c.y, c.z);
    }
}

fn tint_units(
    view: Res<GazeView>,
    env: Res<EnvLight>,
    units: Query<(&Unit, Option<&FeelColor>, &Children)>,
    mut layers: Query<&mut Sprite, With<UnitLayer>>,
) {
    let lit = view.active && env.grid.is_some();
    for (u, feel, children) in &units {
        let light = if lit { env.tint(&view, u.pos) } else { Vec3::ONE };
        let feel = feel.map_or(LinearRgba::WHITE, |f| f.0);
        let color = LinearRgba::new(light.x * feel.red, light.y * feel.green, light.z * feel.blue, feel.alpha);
        for c in children.iter() {
            if let Ok(mut s) = layers.get_mut(c) {
                s.color = color.into();
            }
        }
    }
}

fn fade_roofs(
    time: Res<Time>,
    player: Query<&Unit, With<crate::player::Player>>,
    mut roofs: Query<(&Roof, &mut Sprite)>,
) {
    let covers = |r: &Roof, c: IVec2| {
        let d = c - r.cell;
        d.x >= 0 && d.y >= 0 && d.x <= r.extent.x && d.y <= r.extent.y
    };
    // Under any roof: fade every roof piece nearby (a lane is many overlapping segments).
    let pos = player.single().ok().map(|u| u.pos);
    let under = pos.filter(|p| roofs.iter().any(|(r, _)| covers(r, p.floor().as_ivec2())));
    let k = 1.0 - (-time.delta_secs() * 8.0).exp();
    for (r, mut s) in &mut roofs {
        let centre = r.cell.as_vec2() + (r.extent.as_vec2() + 1.0) * 0.5;
        let near = under.is_some_and(|p| p.distance(centre) < ROOF_FADE_REACH);
        let target = if near { ROOF_UNDER_ALPHA } else { 1.0 };
        let a = s.color.alpha();
        s.color.set_alpha(a + (target - a) * k);
    }
}
