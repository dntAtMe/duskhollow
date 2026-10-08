//! Local player: WASD movement in screen space (client-predicted), walking up to the
//! attack (or talk) target. The follow camera lives in `feel`. The player entity is spawned by `net` on `Welcome`.

use crate::{
    iso,
    map_render::CurrentMap,
    net::{Net, PlayerState},
    ui_input::UiInputCaptured,
    unit::{Dead, Unit},
};
use bevy::prelude::*;
use dusk_protocol::ClientMsg;

pub struct PlayerPlugin;

impl Plugin for PlayerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, |mut commands: Commands| {
            commands.spawn((Camera2d, MainCamera));
        })
        .add_systems(Update, move_player);
    }
}

#[derive(Component)]
pub struct Player;

/// The camera that draws the game view (the minimap has its own `Camera2d`; query this marker,
/// not `Camera2d`).
#[derive(Component)]
pub struct MainCamera;

/// Cells per second while running.
pub const RUN_SPEED: f32 = 4.0;
/// Stop approaching a target at this distance (server melee range is 1.6).
const APPROACH_RANGE: f32 = 1.3;

/// Current facing/movement of the local player in protocol terms.
#[derive(Component, Default)]
pub struct PlayerMotion {
    pub orientation: f32,
    pub moving: bool,
}

/// Cached route to the attack target.
#[derive(Default)]
pub(crate) struct Approach {
    route: Vec<Vec2>,
    goal: Vec2,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn move_player(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    map: Res<CurrentMap>,
    net: Res<Net>,
    mut state: ResMut<PlayerState>,
    mut approach: Local<Approach>,
    mut player: Query<(&mut Unit, &mut PlayerMotion), (With<Player>, Without<Dead>)>,
    others: Query<&Unit, Without<Player>>,
    captured: Res<UiInputCaptured>,
    mut interact: ResMut<crate::dialogue::InteractTarget>,
) {
    let Ok((mut unit, mut motion)) = player.single_mut() else { return };
    if state.stunned || state.rooted {
        unit.set_anim("stance");
        motion.moving = false;
        return;
    }
    let mut screen_dir = Vec2::ZERO;
    for (key, d) in
        [(KeyCode::KeyW, Vec2::Y), (KeyCode::KeyS, -Vec2::Y), (KeyCode::KeyA, -Vec2::X), (KeyCode::KeyD, Vec2::X)]
    {
        if keys.pressed(key) && !captured.keyboard {
            screen_dir += d;
        }
    }

    let step = if screen_dir != Vec2::ZERO {
        // Manual movement cancels auto-attack (Diablo-style).
        if state.target.take().is_some() {
            net.send(ClientMsg::StopAttack);
        }
        interact.0 = None;
        approach.route.clear();
        let origin = iso::to_cell(Vec2::ZERO);
        Some((iso::to_cell(screen_dir.normalize() * iso::TILE_H) - origin).normalize_or_zero())
    } else if let Some(tpos) = state
        .target
        .or(interact.0)
        .and_then(|id| net.entities.get(&id))
        .and_then(|e| others.get(*e).ok())
        .map(|u| u.pos)
    {
        let to = tpos - unit.pos;
        if to.length() <= APPROACH_RANGE {
            approach.route.clear();
            unit.dir = iso::direction_from_orientation(iso::orientation_of(to));
            None
        } else {
            if approach.route.is_empty() || approach.goal.distance(tpos) > 1.0 {
                approach.goal = tpos;
                approach.route = map
                    .grid
                    .find_path((unit.pos.x, unit.pos.y), (tpos.x, tpos.y), 4000)
                    .map(|r| r.into_iter().map(|(x, y)| Vec2::new(x, y)).collect())
                    .unwrap_or_default();
            }
            while approach.route.first().is_some_and(|w| w.distance(unit.pos) < 0.1) {
                approach.route.remove(0);
            }
            approach.route.first().map(|w| (*w - unit.pos).normalize_or_zero())
        }
    } else {
        None
    };

    let Some(step) = step.filter(|s| *s != Vec2::ZERO) else {
        unit.set_anim("stance");
        motion.moving = false;
        return;
    };
    motion.orientation = iso::orientation_of(step);
    motion.moving = true;
    unit.dir = iso::direction_from_orientation(motion.orientation);
    unit.set_anim("run");
    let delta = step * RUN_SPEED * state.speed_mult * time.delta_secs();
    // Slide along walls: try each axis separately.
    let mut pos = unit.pos;
    if map.is_walkable(pos + Vec2::new(delta.x, 0.0)) {
        pos.x += delta.x;
    }
    if map.is_walkable(pos + Vec2::new(0.0, delta.y)) {
        pos.y += delta.y;
    }
    unit.pos = pos;
}
