//! Player sprites reflect equipped gear: every equipped item with a `model` adds the sprite
//! layer `scripts/player/male/<model>.txt` on top of the naked body.
//!
//! Driven by `ServerMsg::Appearance` (item entries per equipment slot) for every player,
//! including ourselves; templates come from the client's item table, so the server only
//! sends numbers.

use crate::{
    data::GameData,
    items_ui::ItemDb,
    net::Net,
    unit::{SMEAR, Unit, UnitLayer},
};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use dusk_formats::item::slot;
use dusk_protocol::EntityId;
use std::collections::HashMap;

pub struct PaperDollPlugin;

impl Plugin for PaperDollPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Appearances>().add_systems(Update, (attach_gear, rebuild_layers).chain());
    }
}

/// Naked body, back to front (same art as `unit::PAPER_DOLL` minus the default weapon).
const BODY: &[&str] = &["default_legs", "default_feet", "default_chest", "default_hands", "head_short"];

/// DESIGN: gear draw order, back to front (legs under boots under chest under gloves; head,
/// shield and weapon on top). The original's per-direction ordering is not recovered.
const GEAR_ORDER: [usize; 7] =
    [slot::LEGS, slot::FEET, slot::CHEST, slot::HANDS, slot::HEAD, slot::OFFHAND, slot::WEAPON];

/// Weapon models (name fragments) swung two-handed: they favour the overhead chop.
const HEAVY_WEAPONS: [&str; 6] = ["greatsword", "zweihander", "maul", "battle_axe", "infantry_axe", "greatstaff"];

/// Last known appearance per server id (survives the unit being respawned locally).
#[derive(Resource, Default)]
pub struct Appearances(pub HashMap<EntityId, Vec<u32>>);

/// Equipped item entries per slot on a unit; changing it rebuilds the sprite layers.
#[derive(Component, PartialEq)]
pub struct Gear(pub Vec<u32>);

/// Keeps `Gear` on every unit in sync with the latest `Appearance`.
fn attach_gear(
    mut commands: Commands,
    appearances: Res<Appearances>,
    net: Res<Net>,
    units: Query<Option<&Gear>, With<Unit>>,
) {
    for (id, gear) in &appearances.0 {
        let Some(&e) = net.entities.get(id) else { continue };
        let Ok(current) = units.get(e) else { continue };
        if current.is_none_or(|g| &g.0 != gear) {
            commands.entity(e).try_insert(Gear(gear.clone()));
        }
    }
}

/// Model names to draw for an appearance, back to front. With `custom` (our generated art,
/// tools/artgen/gear.py) the base body is `custom_body` from `player/custom`.
pub fn layer_models(gear: &[u32], items: &ItemDb, custom: bool) -> Vec<String> {
    let model = |slot: usize| {
        let entry = *gear.get(slot)?;
        items.items.get(&(entry as i64)).filter(|t| t.has_model()).map(|t| t.model.clone())
    };
    let mut out: Vec<String> =
        if custom { vec!["custom_body".to_string()] } else { BODY.iter().map(|s| s.to_string()).collect() };
    for s in GEAR_ORDER {
        // DESIGN: with no melee weapon, a bow is shown in hand.
        let m = if s == slot::WEAPON { model(s).or_else(|| model(slot::RANGED)) } else { model(s) };
        out.extend(m);
    }
    out
}

fn rebuild_layers(
    mut commands: Commands,
    data: Res<GameData>,
    items: Option<Res<ItemDb>>,
    assets: Res<AssetServer>,
    changed: Query<(Entity, &Gear, Option<&Children>), Changed<Gear>>,
    layers: Query<(), With<UnitLayer>>,
    mut units: Query<&mut Unit>,
) {
    let Some(items) = items else { return };
    // Our generated art has its own layers for the item models it covers; others are skipped
    // rather than mixing in original sprites.
    let dir = if data.custom_art { "player/custom" } else { "player/male" };
    for (e, gear, children) in &changed {
        for child in children.into_iter().flatten() {
            if layers.contains(*child) {
                commands.entity(*child).despawn();
            }
        }
        let mut names = layer_models(&gear.0, &items, data.custom_art);
        if let Ok(mut u) = units.get_mut(e) {
            u.heavy_weapon = names.iter().any(|n| HEAVY_WEAPONS.iter().any(|h| n.contains(h)));
        }
        // Weapon smears (our art) go over everything.
        if data.custom_art && SMEAR {
            let smears: Vec<String> =
                names.iter().map(|n| format!("{n}_smear")).filter(|s| data.sprite_script(dir, s).is_some()).collect();
            names.extend(smears);
        }
        for (i, name) in names.iter().enumerate() {
            let Some(script) = data.sprite_script(dir, name) else { continue };
            let Some(path) = data.asset_path(&script.image) else { continue };
            let order = i as f32 * 0.001;
            commands.spawn((
                ChildOf(e),
                UnitLayer { script, order },
                Sprite { image: assets.load(path), ..default() },
                Anchor::TOP_LEFT,
                Transform::from_xyz(0.0, 0.0, order),
            ));
        }
    }
}
