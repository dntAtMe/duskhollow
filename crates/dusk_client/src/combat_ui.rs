//! Combat presentation and input: click-to-target, floating combat text, death notice.

use crate::{
    data::GameData,
    iso,
    net::{Net, PlayerState},
    player::Player,
    ui_input::UiInputCaptured,
    unit::{Dead, Npc, Targeted, Unit},
};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dusk_formats::db::faction;
use dusk_protocol::ClientMsg;

pub struct CombatUiPlugin;

impl Plugin for CombatUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (load_font, spawn_hud).chain())
            .add_systems(Update, (click_target, sync_targeted, animate_floating_text, update_death_notice))
            .add_systems(Update, autoplay.run_if(|| std::env::var_os("DUSK_AUTOPLAY").is_some()));
    }
}

/// Original UI font (Friz Quadrata, the classic MMO face shipped with the game).
#[derive(Resource)]
pub struct UiFont(pub Handle<Font>);

pub fn load_font(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>) {
    let path = data.asset_path("Friz Quadrata Regular.ttf").unwrap_or_else(|| "content/fonts/arial.ttf".into());
    commands.insert_resource(UiFont(assets.load(path)));
}

// ---------------------------------------------------------------- targeting

/// Left click on a hostile/neutral NPC: attack it. Escape: clear target.
#[allow(clippy::too_many_arguments)]
fn click_target(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::player::MainCamera>>,
    data: Res<GameData>,
    net: Res<Net>,
    mut state: ResMut<PlayerState>,
    units: Query<(Entity, &Unit, &Npc, &Transform), (Without<Dead>, Without<Player>)>,
    captured: Res<UiInputCaptured>,
) {
    if !captured.keyboard && keys.just_pressed(KeyCode::Escape) && state.target.take().is_some() {
        net.send(ClientMsg::StopAttack);
    }
    if !mouse.just_pressed(MouseButton::Left) || state.dead || captured.pointer {
        return;
    }
    let (Ok(window), Ok((cam, cam_tf))) = (window.single(), camera.single()) else { return };
    let Some(world) = window.cursor_position().and_then(|c| cam.viewport_to_world_2d(cam_tf, c).ok()) else { return };

    // Front-most unit whose rough body box contains the cursor.
    let hit = units
        .iter()
        .filter(|(_, u, ..)| {
            let feet = iso::to_screen(u.pos);
            let half_w = 22.0 * u.scale;
            (world.x - feet.x).abs() <= half_w && world.y >= feet.y - 10.0 && world.y <= feet.y + u.height * u.scale
        })
        .max_by(|a, b| a.3.translation.z.total_cmp(&b.3.translation.z));
    let Some((e, _, npc, _)) = hit else { return };
    let attackable = data.npc_templates.get(&npc.entry).is_some_and(|t| t.faction != faction::FRIENDLY);
    let Some(id) = net.entity_id(e) else { return };
    if attackable {
        state.target = Some(id);
        net.send(ClientMsg::Attack { target: id });
    }
}

/// Debug aid (`DUSK_AUTOPLAY=1`): keep attacking the nearest attackable NPC.
fn autoplay(
    data: Res<GameData>,
    net: Res<Net>,
    mut state: ResMut<PlayerState>,
    player: Query<&Unit, With<Player>>,
    units: Query<(Entity, &Unit, &Npc), (Without<Dead>, Without<Player>)>,
) {
    let Ok(me) = player.single() else { return };
    if state.target.is_some() || state.dead {
        return;
    }
    let nearest = units
        .iter()
        .filter(|(_, _, n)| data.npc_templates.get(&n.entry).is_some_and(|t| t.faction != faction::FRIENDLY))
        .min_by(|a, b| a.1.pos.distance(me.pos).total_cmp(&b.1.pos.distance(me.pos)));
    if let Some(id) = nearest.filter(|n| n.1.pos.distance(me.pos) < 60.0).and_then(|n| net.entity_id(n.0)) {
        state.target = Some(id);
        net.send(ClientMsg::Attack { target: id });
    }
}

/// Keeps the `Targeted` marker on the entity matching `PlayerState::target`.
fn sync_targeted(
    mut commands: Commands,
    state: Res<PlayerState>,
    net: Res<Net>,
    current: Query<Entity, With<Targeted>>,
) {
    let want = state.target.and_then(|id| net.entities.get(&id).copied());
    for e in &current {
        if Some(e) != want {
            commands.entity(e).remove::<Targeted>();
        }
    }
    if let Some(e) = want.filter(|e| !current.contains(*e)) {
        commands.entity(e).try_insert(Targeted);
    }
}

// ---------------------------------------------------------------- floating text

#[derive(Clone, Copy)]
pub enum FloatKind {
    /// Damage we dealt.
    Outgoing,
    Crit,
    /// Damage we took.
    Incoming,
    Heal,
}

#[derive(Component)]
pub struct FloatingText {
    age: f32,
    color: Color,
}

const FLOAT_SECS: f32 = 1.2;

impl FloatingText {
    pub fn bundle(text: String, kind: FloatKind, cell: Vec2, height: f32) -> impl Bundle {
        let (color, size) = match kind {
            FloatKind::Outgoing => (Color::srgb(1.0, 1.0, 1.0), 16.0),
            FloatKind::Crit => (Color::srgb(1.0, 0.85, 0.2), 22.0),
            FloatKind::Incoming => (Color::srgb(1.0, 0.3, 0.3), 16.0),
            FloatKind::Heal => (Color::srgb(0.35, 1.0, 0.35), 16.0),
        };
        let s = iso::to_screen(cell);
        (
            FloatingText { age: 0.0, color },
            Text2d(text),
            TextFont { font_size: size.into(), ..default() },
            TextColor(color),
            Transform::from_xyz(s.x, s.y + height + 14.0, 950.0),
            // Main view only, not the minimap.
            crate::minimap::overlay_layer(),
        )
    }
}

fn animate_floating_text(
    mut commands: Commands,
    time: Res<Time>,
    font: Option<Res<UiFont>>,
    mut texts: Query<(Entity, &mut FloatingText, &mut Transform, &mut TextColor, &mut TextFont)>,
) {
    let dt = time.delta_secs();
    for (e, mut f, mut t, mut c, mut tf) in &mut texts {
        if f.age == 0.0 {
            if let Some(font) = &font {
                tf.font = font.0.clone().into();
            }
        }
        f.age += dt;
        if f.age >= FLOAT_SECS {
            commands.entity(e).despawn();
            continue;
        }
        t.translation.y += 40.0 * dt;
        c.0 = f.color.with_alpha(1.0 - (f.age / FLOAT_SECS).powi(2));
    }
}

// ---------------------------------------------------------------- death notice

#[derive(Component)]
struct DeathNotice;

/// Unit frames, XP bar etc. live in `hud.rs`; this is just the death message.
fn spawn_hud(mut commands: Commands, font: Res<UiFont>) {
    commands.spawn((
        Text::new("You have died. Reviving..."),
        TextFont { font: font.0.clone().into(), font_size: 32.0.into(), ..default() },
        TextColor(Color::srgb(0.85, 0.15, 0.1)),
        Node { position_type: PositionType::Absolute, top: Val::Percent(40.0), left: Val::Percent(36.0), ..default() },
        Visibility::Hidden,
        DeathNotice,
    ));
}

fn update_death_notice(state: Res<PlayerState>, mut death: Query<&mut Visibility, With<DeathNotice>>) {
    if let Ok(mut v) = death.single_mut() {
        v.set_if_neq(if state.dead { Visibility::Visible } else { Visibility::Hidden });
    }
}
