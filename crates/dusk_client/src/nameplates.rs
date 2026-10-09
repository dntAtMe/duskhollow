//! Name plates over units: name (coloured by faction) and the `nameplate_bg` /
//! `nameplate_hp` bar. Which parts show follows [`HudConfig`]:
//! enemy bars and NPC/player names on by default, friendly bars and our own bar off.
//! The bar also appears on the current target and on damaged units (except our own).

use crate::{
    combat_ui::UiFont,
    data::GameData,
    hud::{FRIENDLY_GREEN, HudConfig, faction_color},
    minimap::overlay_layer,
    player::Player,
    unit::{Dead, Health, Npc, Targeted, Unit},
};
use bevy::prelude::*;
use bevy::sprite::Anchor;
use dusk_formats::content::types::faction;

pub struct NameplatePlugin;

impl Plugin for NameplatePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (spawn_nameplates, update_nameplates).chain());
    }
}

/// `nameplate_bg.png` / `nameplate_hp.png` are 102x12; the fill well is x 2..100, y 2..10.
const PLATE: Vec2 = Vec2::new(102.0, 12.0);
const WELL_X: f32 = 2.0;
const WELL_W: f32 = 98.0;
/// Gap between the head and the bar, in screen pixels.
const HEAD_GAP: f32 = 10.0;

#[derive(Component)]
struct Nameplate {
    /// Bar shown even at full health (config).
    always_bar: bool,
    /// Our own plate: the bar follows `YourNameplateTick` only (the HUD frame shows our health).
    own: bool,
}

#[derive(Component)]
struct PlateBar;

#[derive(Component)]
struct PlateFill;

#[derive(Component)]
struct PlateName;

#[allow(clippy::type_complexity)]
fn spawn_nameplates(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Option<Res<UiFont>>,
    config: Res<HudConfig>,
    units: Query<(Entity, &Unit, Option<&Npc>, Option<&Name>, Has<Player>), Added<Unit>>,
) {
    let Some(font) = font else { return };
    let load = |n: &str| data.asset_path(n).map(|p| assets.load(p)).unwrap_or_default();
    for (e, unit, npc, name, is_player) in &units {
        let tpl = npc.and_then(|n| data.npc_templates.get(&n.entry));
        let (show_name, always_bar, color) = match (tpl, is_player) {
            (_, true) => (config.your_name, config.your_nameplate, FRIENDLY_GREEN),
            (Some(t), _) => {
                let enemy = t.faction == faction::HOSTILE || t.faction == faction::NEUTRAL;
                let bar = if enemy { config.enemy_nameplates } else { config.friendly_nameplates };
                (config.show_npc_names, bar, faction_color(t.faction))
            }
            (None, _) => (config.show_player_names, config.friendly_nameplates, FRIENDLY_GREEN),
        };
        let friendly = tpl.is_none_or(|t| t.faction == faction::FRIENDLY || t.faction == faction::PLAYER_DEFAULT);
        let hp_art = if friendly { "nameplate_hp_party.png" } else { "nameplate_hp.png" };

        // Counter-scaled so every unit gets the same on-screen size; drawn above the world.
        let s = unit.scale;
        let root = commands
            .spawn((
                ChildOf(e),
                Nameplate { always_bar, own: is_player },
                Transform::from_xyz(0.0, unit.height + HEAD_GAP / s, 500.0).with_scale(Vec3::splat(1.0 / s)),
                Visibility::default(),
            ))
            .id();
        let bar = commands
            .spawn((
                ChildOf(root),
                PlateBar,
                Sprite { image: load("nameplate_bg.png"), ..default() },
                Transform::from_xyz(0.0, PLATE.y / 2.0, 0.0),
                Visibility::Hidden,
                overlay_layer(),
            ))
            .id();
        commands.spawn((
            ChildOf(bar),
            PlateFill,
            Sprite { image: load(hp_art), rect: Some(Rect::new(WELL_X, 0.0, WELL_X + WELL_W, PLATE.y)), ..default() },
            Anchor::CENTER_LEFT,
            Transform::from_xyz(-PLATE.x / 2.0 + WELL_X, 0.0, 0.01),
            overlay_layer(),
        ));
        let label = name.map(|n| n.to_string()).or_else(|| tpl.map(|t| t.name.clone())).unwrap_or_default();
        if show_name && !label.is_empty() {
            commands.spawn((
                ChildOf(root),
                PlateName,
                Text2d(label),
                TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() },
                TextColor(color),
                TextShadow { offset: Vec2::new(1.0, -1.0), color: Color::BLACK },
                Anchor::BOTTOM_CENTER,
                Transform::from_xyz(0.0, PLATE.y + 2.0, 0.02),
                overlay_layer(),
            ));
        }
    }
}

#[allow(clippy::type_complexity)]
fn update_nameplates(
    units: Query<(&Health, Has<Dead>, Has<Targeted>)>,
    plates: Query<(&ChildOf, &Nameplate, &Children, &mut Visibility), Without<PlateBar>>,
    mut bars: Query<(&mut Visibility, &Children), With<PlateBar>>,
    mut fills: Query<&mut Sprite, With<PlateFill>>,
) {
    for (child_of, plate, children, mut vis) in plates {
        let Ok((health, dead, targeted)) = units.get(child_of.parent()) else { continue };
        vis.set_if_neq(if dead { Visibility::Hidden } else { Visibility::Inherited });
        let ratio = (health.hp as f32 / health.max.max(1) as f32).clamp(0.0, 1.0);
        let show_bar = plate.always_bar || (!plate.own && (targeted || ratio < 1.0));
        for child in children.iter() {
            let Ok((mut bar_vis, bar_children)) = bars.get_mut(child) else { continue };
            bar_vis.set_if_neq(if show_bar { Visibility::Inherited } else { Visibility::Hidden });
            for fill in bar_children.iter() {
                if let Ok(mut s) = fills.get_mut(fill) {
                    let rect = Rect::new(WELL_X, 0.0, WELL_X + (WELL_W * ratio).round(), PLATE.y);
                    if s.rect != Some(rect) {
                        s.rect = Some(rect);
                    }
                }
            }
        }
    }
}
