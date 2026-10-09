//! Loads a `.map` file on request and spawns its terrain and tile sprites.
//! Units are not touched here; whoever requested the map populates it after
//! [`MapLoaded`] (offline: from game.db, online: from the server).

use crate::{data::GameData, iso, lights, particles::FxMaterial};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use dusk_formats::map::{FLAG_UNWALKABLE, MapFile, TERRAIN_CHUNK, WalkGrid};

pub struct MapRenderPlugin;

impl Plugin for MapRenderPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<MapLoaded>()
            .add_systems(PreUpdate, load_map.run_if(|m: Res<CurrentMap>| m.requested.is_some()));
    }
}

/// Fired once the requested map's tiles are spawned and collision is ready.
#[derive(Message)]
pub struct MapLoaded;

#[derive(Resource, Default)]
pub struct CurrentMap {
    /// Set this to load (or switch to) a map by name.
    pub requested: Option<String>,
    pub name: String,
    /// `map.id` in game.db, if the map is listed there.
    pub id: Option<i64>,
    pub grid: WalkGrid,
    /// Row-major: cell has a ground tile or terrain under it.
    pub floor: Vec<bool>,
    /// Default spawn point (db start or a walkable cell near the middle).
    pub start: Vec2,
}

impl CurrentMap {
    pub fn request(&mut self, name: impl Into<String>) {
        self.requested = Some(name.into());
    }

    pub fn is_walkable(&self, cell: Vec2) -> bool {
        self.grid.is_walkable(cell.x, cell.y)
    }
}

/// Marker for every tile sprite of the current map.
#[derive(Component)]
pub struct MapTile;

#[allow(clippy::too_many_arguments)]
fn load_map(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut fx_materials: ResMut<Assets<FxMaterial>>,
    mut current: ResMut<CurrentMap>,
    old_tiles: Query<Entity, With<MapTile>>,
    mut loaded: MessageWriter<MapLoaded>,
) {
    let name = current.requested.take().unwrap();
    let path =
        data.find_file(&format!("maps/{name}.map")).unwrap_or_else(|| data.root.join(format!("maps/{name}.map")));
    let map = match MapFile::load(&path) {
        Ok(m) => m,
        Err(e) => {
            error!("cannot load {}: {e}", path.display());
            return;
        }
    };
    for e in &old_tiles {
        commands.entity(e).despawn();
    }
    let info = data.maps.iter().find(|m| m.name == name).cloned();

    current.name = name;
    current.id = info.as_ref().map(|i| i.id);
    current.grid = map.walk_grid();
    current.floor = vec![false; (map.size * map.size) as usize];

    // Resolve each texture once: handle + pivot.
    let textures: Vec<Option<(Handle<Image>, Vec2)>> = map
        .textures
        .iter()
        .map(|name| {
            if name.to_lowercase().ends_with(".psi") {
                return None; // invisible sprite carrying only sprite_fx effects (fireflies)
            }
            let rel = data.asset_path(name)?;
            if !rel.ends_with(".png") {
                return None;
            }
            Some((assets.load(rel), data.hotspot(name)?))
        })
        .collect();

    let (mut sprites, mut effects, mut glow) = (0, 0, None);

    // Terrain: one big texture per 13x13 chunk, centred on cell (col*13, row*13), under everything.
    let tw = map.terrain_width();
    for &(id, tex) in &map.terrain {
        let Some(rel) = map.terrain_textures.get(tex as usize).and_then(|n| data.asset_path(n)) else { continue };
        let (col, row) = (id % tw, id / tw);
        let centre = iso::to_screen(Vec2::new((col * TERRAIN_CHUNK) as f32 + 0.5, (row * TERRAIN_CHUNK) as f32 + 0.5));
        commands.spawn((
            MapTile,
            Sprite { image: assets.load(rel), ..default() },
            Transform::from_xyz(centre.x, centre.y, -1.0),
        ));
        for y in row * TERRAIN_CHUNK..((row + 1) * TERRAIN_CHUNK).min(map.size) {
            for x in col * TERRAIN_CHUNK..((col + 1) * TERRAIN_CHUNK).min(map.size) {
                current.floor[(y * map.size + x) as usize] = true;
            }
        }
        sprites += 1;
    }
    for cell in &map.cells {
        current.floor[(cell.y * map.size + cell.x) as usize] |= cell.layers[0].is_some();
        let centre = Vec2::new(cell.x as f32 + 0.5, cell.y as f32 + 0.5);
        let screen = iso::to_screen(centre);
        for (layer_idx, layer) in cell.layers.iter().enumerate() {
            let Some(layer) = layer else { continue };
            // Layers 0/1 are flat ground + decals, layer 2 is upright (walls, trees...).
            let z = match layer_idx {
                0 => 0.0,
                1 => 0.1,
                _ => iso::depth(centre),
            };
            let texture = map.textures.get(layer.texture as usize).map(String::as_str).unwrap_or_default();
            let resolved = textures.get(layer.texture as usize).and_then(Option::as_ref);
            // Sprites without a texture get hotspot (1, 1) in `Sprite::renderScript`.
            let hotspot = resolved.map_or(Vec2::ONE, |(_, pivot)| *pivot);
            effects += lights::spawn_sprite_effects(
                &mut commands,
                &data,
                &assets,
                &mut meshes,
                &mut fx_materials,
                &mut glow,
                texture,
                hotspot,
                screen,
                z,
            );
            let Some((image, pivot)) = resolved else { continue };
            // TOP_LEFT anchor: shift so the sprite's pivot sits on the cell centre.
            // Roofs draw above everything standing under them (front-most roofed cell).
            let lower = texture.to_lowercase();
            let roof = (layer_idx >= 2)
                .then(|| data.roofs.iter().find(|(p, _)| lower.starts_with(p.as_str())).map(|(_, d)| *d))
                .flatten();
            let z = roof.map_or(z, |d| iso::depth(centre + d.as_vec2()) + 0.05);
            let mut tile = commands.spawn((
                MapTile,
                Sprite { image: image.clone(), ..default() },
                Anchor::TOP_LEFT,
                Transform::from_xyz(screen.x - pivot.x, screen.y + pivot.y, z),
            ));
            if layer_idx >= 2 {
                tile.insert(crate::env_light::Upright { cell: centre });
            }
            if let Some(extent) = roof {
                tile.insert(crate::env_light::Roof { cell: IVec2::new(cell.x as i32, cell.y as i32), extent });
            }
            sprites += 1;
        }
    }

    current.start = info
        .as_ref()
        .map(|i| Vec2::new(i.start.0, i.start.1))
        .filter(|s| *s != Vec2::ZERO)
        .unwrap_or_else(|| first_walkable(&current));
    info!(
        "map {} ({}x{}): {} cells, {} sprites, {} sprite effects, {} textures, {} terrain chunks",
        current.name,
        map.size,
        map.size,
        map.cells.len(),
        sprites,
        effects,
        map.textures.len(),
        map.terrain.len(),
    );
    loaded.write(MapLoaded);
}

fn first_walkable(map: &CurrentMap) -> Vec2 {
    // Walkable floor cell nearest the middle of the map.
    let size = map.grid.size;
    let mid = Vec2::splat(size as f32 / 2.0);
    (0..size * size)
        .filter(|&i| map.floor[i as usize] && map.grid.flags[i as usize] & FLAG_UNWALKABLE == 0)
        .map(|i| Vec2::new((i % size) as f32 + 0.5, (i / size) as f32 + 0.5))
        .min_by(|a, b| a.distance_squared(mid).total_cmp(&b.distance_squared(mid)))
        .unwrap_or(mid)
}
