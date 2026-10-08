//! Animated 8-directional units (NPCs and the paper-doll player) driven by
//! the original sprite scripts. Name plates / health bars are in `nameplates.rs`.

use crate::{data::GameData, iso};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use dusk_formats::sprite_script::SpriteScript;
use std::sync::Arc;

pub struct UnitPlugin;

impl Plugin for UnitPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (tick_units, sync_unit_transforms, sync_unit_layers).chain());
    }
}

/// Default visual height (px) of a player character, for bars and floating text.
const PLAYER_HEIGHT: f32 = 75.0;

/// A unit standing on the map, in cell coordinates.
#[derive(Component, Debug)]
pub struct Unit {
    pub pos: Vec2,
    pub dir: u8,
    /// Looping/base animation (stance, run, die...).
    pub anim: &'static str,
    pub elapsed_ms: f32,
    /// One-shot animation layered over `anim` (swing, hit, block...).
    action: Option<(&'static str, f32, f32)>,
    pub scale: f32,
    /// Visual height in unscaled pixels (from `npc_models.height`).
    pub height: f32,
    body: Option<Arc<SpriteScript>>,
}

impl Unit {
    pub fn new(pos: Vec2, dir: u8, scale: f32, height: f32) -> Self {
        Self { pos, dir, anim: "stance", elapsed_ms: 0.0, action: None, scale, height, body: None }
    }

    pub fn set_anim(&mut self, anim: &'static str) {
        if self.anim != anim {
            self.anim = anim;
            self.elapsed_ms = 0.0;
        }
    }

    /// Plays a one-shot animation if the unit has it; the base animation resumes after.
    pub fn play_action(&mut self, anim: &'static str) {
        let len = self.body.as_ref().and_then(|b| b.animations.get(anim)).map(|a| a.duration_ms as f32);
        if let Some(len) = len {
            self.action = Some((anim, 0.0, len.max(100.0)));
        }
    }

    pub fn is_acting(&self) -> bool {
        self.action.is_some()
    }

    /// Shows the last frame of the death animation (e.g. corpses already dead on spawn).
    pub fn set_dead_pose(&mut self) {
        self.action = None;
        self.anim = "die";
        self.elapsed_ms = 1.0e9;
    }

    fn current(&self) -> (&'static str, f32) {
        match self.action {
            Some((a, t, _)) => (a, t),
            None => (self.anim, self.elapsed_ms),
        }
    }
}

/// One sprite-sheet layer of a unit (the whole body for NPCs, one gear piece for players).
#[derive(Component)]
pub struct UnitLayer {
    pub script: Arc<SpriteScript>,
    /// Draw order within the unit.
    pub order: f32,
}

#[derive(Component)]
pub struct Npc {
    pub entry: i64,
}

#[derive(Component, Clone, Copy)]
pub struct Health {
    pub hp: i32,
    pub max: i32,
}

/// Unit level as last reported by the server.
#[derive(Component, Clone, Copy)]
pub struct Level(pub u32);

/// Unit is dead (shows the death pose, can't be targeted).
#[derive(Component)]
pub struct Dead;

/// The local player's current target (bar always visible).
#[derive(Component)]
pub struct Targeted;

/// Default naked look plus a starter weapon; draw order back-to-front.
/// TODO: per-direction layer order (weapon behind body when facing away).
const PAPER_DOLL: &[&str] =
    &["default_legs", "default_feet", "default_chest", "default_hands", "head_short", "shortsword"];

/// Spawns a unit with the given sprite-script layers; returns the parent entity.
pub fn spawn_unit(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    mut unit: Unit,
    layers: &[(&str, &str)],
) -> Option<Entity> {
    let scripts: Vec<_> = layers
        .iter()
        .filter_map(|(dir, name)| {
            let script = data.sprite_script(dir, name).or_else(|| {
                warn!("missing sprite script {dir}/{name}");
                None
            })?;
            let path = data.asset_path(&script.image).or_else(|| {
                warn!("missing sheet {} for {dir}/{name}", script.image);
                None
            })?;
            Some((script, path))
        })
        .collect();
    if scripts.is_empty() {
        return None;
    }
    unit.body = Some(scripts[0].0.clone());
    let scale = unit.scale;
    let parent = commands
        .spawn((
            unit,
            Health { hp: 1, max: 1 },
            Transform::from_scale(Vec3::new(scale, scale, 1.0)),
            Visibility::default(),
        ))
        .id();
    for (i, (script, path)) in scripts.into_iter().enumerate() {
        let order = i as f32 * 0.001;
        commands.spawn((
            ChildOf(parent),
            UnitLayer { script, order },
            Sprite { image: assets.load(path), ..default() },
            Anchor::TOP_LEFT,
            Transform::from_xyz(0.0, 0.0, order),
        ));
    }
    Some(parent)
}

/// `scripts/npc/custom/<model>.txt` (our generated monster, tools/artgen/creatures.py) when running
/// with `--art custom` and one exists, else the original `scripts/npc/<model>.txt`.
fn npc_script_dir(data: &GameData, model: &str) -> &'static str {
    let custom = data.custom_art && data.root.join("scripts/npc/custom").join(format!("{model}.txt")).exists();
    if custom { "npc/custom" } else { "npc" }
}

/// Spawns an NPC by `npc_template.entry` at a cell position.
pub fn spawn_npc(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    entry: i64,
    pos: Vec2,
    orientation: f32,
) -> Option<Entity> {
    let template = data.npc_templates.get(&entry)?;
    let model = data.npc_models.get(&template.model_id)?;
    let scale = if template.model_scale > 0 { template.model_scale as f32 / 100.0 } else { 1.0 };
    let height = if model.height > 0 { model.height as f32 } else { 60.0 };
    let unit = Unit::new(pos, iso::direction_from_orientation(orientation), scale, height);
    let e = spawn_unit(commands, data, assets, unit, &[(npc_script_dir(data, &model.name), &model.name)])?;
    commands.entity(e).insert((Npc { entry }, Name::new(template.name.clone())));
    Some(e)
}

/// Spawns a player-character paper doll.
pub fn spawn_paper_doll(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    name: &str,
    pos: Vec2,
    orientation: f32,
) -> Option<Entity> {
    let layers: Vec<_> = if data.custom_art {
        vec![("player/custom", "adventurer")]
    } else {
        PAPER_DOLL.iter().map(|n| ("player/male", *n)).collect()
    };
    let unit = Unit::new(pos, iso::direction_from_orientation(orientation), 1.0, PLAYER_HEIGHT);
    let e = spawn_unit(commands, data, assets, unit, &layers)?;
    commands.entity(e).insert(Name::new(name.to_string()));
    Some(e)
}

fn tick_units(time: Res<Time>, mut units: Query<&mut Unit>) {
    let dt = time.delta_secs() * 1000.0;
    for mut u in &mut units {
        u.elapsed_ms += dt;
        if let Some((_, t, len)) = &mut u.action {
            *t += dt;
            if *t >= *len {
                u.action = None;
            }
        }
    }
}

fn sync_unit_transforms(mut units: Query<(&Unit, &mut Transform), Changed<Unit>>) {
    for (u, mut t) in &mut units {
        let s = iso::to_screen(u.pos);
        t.translation = Vec3::new(s.x, s.y, iso::depth(u.pos));
    }
}

fn sync_unit_layers(units: Query<&Unit>, mut layers: Query<(&ChildOf, &UnitLayer, &mut Sprite, &mut Transform)>) {
    for (child_of, layer, mut sprite, mut transform) in &mut layers {
        let Ok(unit) = units.get(child_of.parent()) else { continue };
        let (name, t) = unit.current();
        let anim = layer.script.animations.get(name).or_else(|| layer.script.animations.get("stance"));
        let Some(anim) = anim.filter(|a| !a.frames.is_empty()) else {
            sprite.rect = Some(Rect::new(0.0, 0.0, 0.0, 0.0));
            continue;
        };
        let f = &anim.frames[anim.frame_at(t as u32)][unit.dir as usize % 8];
        sprite.rect = Some(Rect::new(f.x as f32, f.y as f32, (f.x + f.w) as f32, (f.y + f.h) as f32));
        transform.translation = Vec3::new(-f.pivot_x as f32, f.pivot_y as f32, layer.order);
    }
}
