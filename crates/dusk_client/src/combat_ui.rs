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
use dusk_protocol::{ClientMsg, EntityId};

pub struct CombatUiPlugin;

impl Plugin for CombatUiPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, load_font)
            .add_systems(OnEnter(crate::state::AppState::InGame), (spawn_hud, spawn_target_ring))
            .add_systems(
                Update,
                (click_target, tab_target, sync_targeted, update_death_notice).run_if(crate::state::in_game),
            )
            .add_systems(Update, animate_floating_text)
            .add_systems(Update, (xp_feedback, low_health_warning).run_if(crate::state::in_game))
            .add_systems(PostUpdate, follow_target_ring.run_if(crate::state::in_game))
            .add_systems(
                Update,
                autoplay.run_if(|| std::env::var_os("DUSK_AUTOPLAY").is_some()).run_if(crate::state::in_game),
            );
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

/// Two left clicks on the same unit within this many seconds attack it.
const DOUBLE_CLICK_SECS: f32 = 0.35;
/// Tab only cycles through hostiles this close (cells) that are also on screen.
const TAB_RANGE: f32 = 12.0;

/// Selects `id`. An auto-attack already running follows the new target.
fn select(state: &mut PlayerState, net: &Net, id: EntityId) {
    state.target = Some(id);
    if state.attacking {
        net.send(ClientMsg::Attack { target: id });
    }
}

fn attack(state: &mut PlayerState, net: &Net, id: EntityId) {
    state.target = Some(id);
    state.attacking = true;
    net.send(ClientMsg::Attack { target: id });
}

/// Left click on a hostile/neutral NPC selects it (spells land on it, no walking);
/// right click or a double click attacks it (walk up + auto-attack). Escape clears.
#[allow(clippy::too_many_arguments)]
fn click_target(
    mouse: Res<ButtonInput<MouseButton>>,
    time: Res<Time>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::player::MainCamera>>,
    data: Res<GameData>,
    net: Res<Net>,
    mut state: ResMut<PlayerState>,
    units: Query<(Entity, &Unit, &Npc, &Transform), (Without<Dead>, Without<Player>)>,
    captured: Res<UiInputCaptured>,
    mut last_click: Local<Option<(EntityId, f32)>>,
    esc: Res<crate::windows::EscAction>,
) {
    if *esc == crate::windows::EscAction::ClearTarget && state.target.is_some() {
        if state.attacking {
            net.send(ClientMsg::StopAttack);
        }
        state.clear_target();
    }
    let (left, right) = (mouse.just_pressed(MouseButton::Left), mouse.just_pressed(MouseButton::Right));
    if !(left || right) || state.dead || captured.pointer {
        return;
    }
    let (Ok(window), Ok((cam, cam_tf))) = (window.single(), camera.single()) else { return };
    let Some(world) = window.cursor_position().and_then(|c| cam.viewport_to_world_2d(cam_tf, c).ok()) else { return };
    let Some((e, npc)) = pick_npc(world, units.iter().map(|(e, u, n, t)| (e, u, n, t))) else { return };
    let attackable = data.npc_templates.get(&npc.entry).is_some_and(|t| t.faction != faction::FRIENDLY);
    let Some(id) = net.entity_id(e).filter(|_| attackable) else { return };
    let now = time.elapsed_secs();
    let double = left && last_click.is_some_and(|(prev, t)| prev == id && now - t < DOUBLE_CLICK_SECS);
    *last_click = left.then_some((id, now));
    if right || double {
        attack(&mut state, &net, id);
    } else {
        select(&mut state, &net, id);
    }
}

/// Tab / Shift+Tab: cycle through living hostiles near the player, nearest first.
#[allow(clippy::too_many_arguments)]
fn tab_target(
    keys: Res<ButtonInput<KeyCode>>,
    data: Res<GameData>,
    net: Res<Net>,
    mut state: ResMut<PlayerState>,
    player: Query<&Unit, With<Player>>,
    units: Query<(Entity, &Unit, &Npc), (Without<Dead>, Without<Player>)>,
    camera: Query<(&Camera, &GlobalTransform), With<crate::player::MainCamera>>,
    captured: Res<UiInputCaptured>,
) {
    if captured.keyboard || state.dead || !keys.just_pressed(KeyCode::Tab) {
        return;
    }
    let (Ok(me), Ok((cam, cam_tf))) = (player.single(), camera.single()) else { return };
    let viewport = cam.logical_viewport_size().unwrap_or(Vec2::new(1280.0, 720.0));
    let on_screen = |u: &Unit| {
        cam.world_to_viewport(cam_tf, iso::to_screen(u.pos).extend(0.0))
            .is_ok_and(|p| p.x >= 0.0 && p.y >= 0.0 && p.x <= viewport.x && p.y <= viewport.y)
    };
    let mut near: Vec<(f32, EntityId)> = units
        .iter()
        .filter(|(_, _, n)| data.npc_templates.get(&n.entry).is_some_and(|t| t.faction != faction::FRIENDLY))
        .filter(|(_, u, _)| u.pos.distance(me.pos) <= TAB_RANGE && on_screen(u))
        .filter_map(|(e, u, _)| Some((u.pos.distance(me.pos), net.entity_id(e)?)))
        .collect();
    if near.is_empty() {
        return;
    }
    near.sort_by(|a, b| a.0.total_cmp(&b.0));
    let back = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
    let next = match state.target.and_then(|t| near.iter().position(|(_, id)| *id == t)) {
        Some(i) if back => (i + near.len() - 1) % near.len(),
        Some(i) => (i + 1) % near.len(),
        None => 0,
    };
    select(&mut state, &net, near[next].1);
}

/// Front-most NPC whose rough body box contains the world-space point `at`.
pub fn pick_npc<'a>(
    at: Vec2,
    units: impl Iterator<Item = (Entity, &'a Unit, &'a Npc, &'a Transform)>,
) -> Option<(Entity, &'a Npc)> {
    units
        .filter(|(_, u, ..)| {
            let feet = iso::to_screen(u.pos);
            let half_w = 22.0 * u.scale;
            (at.x - feet.x).abs() <= half_w && at.y >= feet.y - 10.0 && at.y <= feet.y + u.height * u.scale
        })
        .max_by(|a, b| a.3.translation.z.total_cmp(&b.3.translation.z))
        .map(|(e, _, n, _)| (e, n))
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
        attack(&mut state, &net, id);
    }
}

/// Ring under the selected unit, drawn just behind it (over grain and clutter behind it).
const RING_Z: f32 = 0.6;
/// Bouncing chevron over the selected unit's head.
const CHEVRON_Z: f32 = 600.0;

#[derive(Component)]
struct TargetRing;

#[derive(Component)]
struct TargetChevron;

/// A flat iso ellipse ring, dithered rim, white (tinted per use).
fn spawn_target_ring(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    // Ring: flat iso ellipse, dithered inner rim.
    let (w, h) = (56u32, 28u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let d = Vec2::new(
                (x as f32 + 0.5 - w as f32 / 2.0) / (w as f32 / 2.0),
                (y as f32 + 0.5 - h as f32 / 2.0) / (h as f32 / 2.0),
            )
            .length();
            let a = if (0.8..0.97).contains(&d) {
                255
            } else if (0.7..0.8).contains(&d) && (x + y) % 2 == 0 {
                140
            } else {
                0
            };
            data.extend([255, 255, 255, a]);
        }
    }
    let img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    commands.spawn((
        TargetRing,
        Sprite { image: images.add(img), ..default() },
        Transform::from_xyz(0.0, 0.0, RING_Z),
        Visibility::Hidden,
        crate::minimap::overlay_layer(),
    ));
    // Downward chevron, 2 px outline in darker tint for contrast.
    let (w, h) = (13u32, 9u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let dx = (x - 6).abs();
            let inner = y >= 1 && y <= 6 && dx <= 6 - y && dx >= 4 - y.min(4);
            let edge = dx <= 7 - y && !inner && y <= 7;
            let a = if inner {
                [255, 255, 255, 255]
            } else if edge {
                [70, 20, 20, 230]
            } else {
                [0, 0, 0, 0]
            };
            data.extend(a);
        }
    }
    let img = Image::new(
        Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    commands.spawn((
        TargetChevron,
        Sprite { image: images.add(img), ..default() },
        Transform::from_xyz(0.0, 0.0, CHEVRON_Z).with_scale(Vec3::splat(2.0)),
        Visibility::Hidden,
        crate::minimap::overlay_layer(),
    ));
}

fn follow_target_ring(
    time: Res<Time>,
    data: Res<GameData>,
    targeted: Query<(&Unit, Option<&Npc>), (With<Targeted>, Without<Dead>)>,
    state: Res<PlayerState>,
    mut ring: Query<(&mut Transform, &mut Visibility, &mut Sprite), (With<TargetRing>, Without<TargetChevron>)>,
    mut chevron: Query<(&mut Transform, &mut Visibility, &mut Sprite), (With<TargetChevron>, Without<TargetRing>)>,
) {
    let Ok((mut t, mut vis, mut sprite)) = ring.single_mut() else { return };
    let Ok((mut ct, mut cvis, mut csprite)) = chevron.single_mut() else { return };
    let Ok((u, npc)) = targeted.single() else {
        *vis = Visibility::Hidden;
        *cvis = Visibility::Hidden;
        return;
    };
    *vis = Visibility::Visible;
    *cvis = Visibility::Visible;
    let feet = iso::to_screen(u.pos);
    t.translation = feet.extend(iso::depth(u.pos) - 0.02).max(Vec3::new(f32::MIN, f32::MIN, RING_Z));
    t.scale = Vec3::splat(1.35 * u.scale.max(0.6));
    let bob = 3.0 * (time.elapsed_secs() * 5.0).sin().abs();
    ct.translation = (feet + Vec2::new(0.0, u.height * u.scale + 50.0 + bob)).extend(CHEVRON_Z);
    let friendly = npc.and_then(|n| data.npc_templates.get(&n.entry)).is_some_and(|t| t.faction == faction::FRIENDLY);
    // Hostile: oxblood-crimson, brighter while attacking; pulses slowly.
    let pulse = 0.75 + 0.25 * (time.elapsed_secs() * 4.0).sin();
    let (r, g, b) = if friendly {
        (0.55, 0.8, 0.45)
    } else if state.attacking {
        (0.95, 0.3, 0.18)
    } else {
        (0.85, 0.22, 0.2)
    };
    sprite.color = Color::srgba(r, g, b, pulse);
    csprite.color = Color::srgb(r, g, b);
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
    /// Experience gained / level up (over the player).
    Xp,
}

#[derive(Component)]
pub struct FloatingText {
    age: f32,
    color: Color,
}

const FLOAT_SECS: f32 = 1.2;

impl FloatingText {
    pub fn bundle(text: String, kind: FloatKind, cell: Vec2, height: f32) -> impl Bundle {
        // Bone instead of pure white (no near-white in the palette); crits get a "!".
        let (color, size, text) = match kind {
            FloatKind::Outgoing => (Color::srgb(0.93, 0.88, 0.78), 17.0, text),
            FloatKind::Crit => (Color::srgb(1.0, 0.80, 0.25), 24.0, format!("{text}!")),
            FloatKind::Incoming => (Color::srgb(1.0, 0.32, 0.26), 17.0, text),
            FloatKind::Heal => (Color::srgb(0.42, 0.95, 0.38), 17.0, text),
            FloatKind::Xp => (Color::srgb(0.80, 0.55, 0.95), 15.0, text),
        };
        let s = iso::to_screen(cell);
        (
            FloatingText { age: 0.0, color },
            Text2d(text),
            TextFont { font_size: size.into(), ..default() },
            TextColor(color),
            bevy::sprite::Text2dShadow { offset: Vec2::new(1.5, -1.5), color: Color::BLACK.with_alpha(0.9) },
            Transform::from_xyz(s.x, s.y + height + 14.0, 950.0),
            // Main view only, not the minimap.
            crate::minimap::overlay_layer(),
        )
    }
}

#[allow(clippy::type_complexity)]
fn animate_floating_text(
    mut commands: Commands,
    time: Res<Time>,
    font: Option<Res<UiFont>>,
    mut texts: Query<(
        Entity,
        &mut FloatingText,
        &mut Transform,
        &mut TextColor,
        &mut TextFont,
        Option<&mut bevy::sprite::Text2dShadow>,
    )>,
) {
    let dt = time.delta_secs();
    for (e, mut f, mut t, mut c, mut tf, shadow) in &mut texts {
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
        let a = 1.0 - (f.age / FLOAT_SECS).powi(2);
        c.0 = f.color.with_alpha(a);
        if let Some(mut s) = shadow {
            s.color.set_alpha(0.9 * a);
        }
    }
}

// ---------------------------------------------------------------- death notice

#[derive(Component)]
struct DeathNotice;

#[derive(Component)]
struct LowHealthVignette;

/// Unit frames, XP bar etc. live in `hud.rs`; this is the death message and the low-health
/// warning.
fn spawn_hud(mut commands: Commands, font: Res<UiFont>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let shadow = TextShadow { offset: Vec2::splat(2.0), color: Color::BLACK.with_alpha(0.9) };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(36.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(6.0),
                ..default()
            },
            Visibility::Hidden,
            DeathNotice,
        ))
        .with_children(|d| {
            d.spawn((Text::new("You have died."), f(34.0), TextColor(Color::srgb(0.85, 0.15, 0.1)), shadow));
            d.spawn((
                Text::new("The fire remembers you. Reviving at your cairn..."),
                f(16.0),
                TextColor(Color::srgb(0.84, 0.79, 0.68)),
                shadow,
            ));
        });
    // Pulsing crimson edges below 30 % health.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundGradient::from(RadialGradient::new(
            UiPosition::CENTER,
            RadialGradientShape::FarthestCorner,
            vec![
                ColorStop::new(Color::NONE, Val::Percent(64.0)),
                ColorStop::new(Color::srgba(0.55, 0.02, 0.01, 0.4), Val::Percent(100.0)),
            ],
        )),
        Visibility::Hidden,
        GlobalZIndex(-5),
        LowHealthVignette,
    ));
}

fn update_death_notice(state: Res<PlayerState>, mut death: Query<&mut Visibility, With<DeathNotice>>) {
    if let Ok(mut v) = death.single_mut() {
        v.set_if_neq(if state.dead { Visibility::Visible } else { Visibility::Hidden });
    }
}

/// Low health below this fraction pulses the screen edges.
const LOW_HEALTH: f32 = 0.3;

fn low_health_warning(
    time: Res<Time>,
    state: Res<PlayerState>,
    mut vignette: Query<(&mut Visibility, &mut BackgroundGradient), With<LowHealthVignette>>,
) {
    let Ok((mut vis, mut grad)) = vignette.single_mut() else { return };
    let mut ratio = state.hp.max(0) as f32 / state.max_hp.max(1) as f32;
    // Debug aid: `DUSK_LOW_HP_TEST=1` shows the warning at 15 % health.
    if std::env::var_os("DUSK_LOW_HP_TEST").is_some() {
        ratio = 0.15;
    }
    if state.dead || state.max_hp <= 0 || ratio >= LOW_HEALTH {
        vis.set_if_neq(Visibility::Hidden);
        return;
    }
    vis.set_if_neq(Visibility::Inherited);
    // Faster and stronger the closer to death.
    let urgency = 1.0 - ratio / LOW_HEALTH;
    let beat = (time.elapsed_secs() * (3.0 + 3.0 * urgency)).sin() * 0.5 + 0.5;
    let a = 0.15 + 0.2 * urgency + 0.15 * beat;
    if let Some(Gradient::Radial(r)) = grad.0.first_mut() {
        if let Some(stop) = r.stops.last_mut() {
            stop.color = Color::srgba(0.55, 0.02, 0.01, a.min(0.5));
        }
    }
}

/// "+N XP" over the player when experience comes in, "Level N" on a level up.
fn xp_feedback(
    mut commands: Commands,
    state: Res<PlayerState>,
    player: Query<&Unit, With<Player>>,
    mut last: Local<Option<(u32, u32)>>,
) {
    if !state.is_changed() {
        return;
    }
    let now = (state.level, state.xp);
    let prev = last.replace(now);
    let (Some((lvl, xp)), Ok(me)) = (prev, player.single()) else { return };
    if lvl == 0 {
        return;
    }
    if state.level > lvl {
        commands.spawn(FloatingText::bundle(
            format!("Level {}", state.level),
            FloatKind::Crit,
            me.pos,
            me.height + 24.0,
        ));
    } else if state.xp > xp {
        commands.spawn(FloatingText::bundle(format!("+{} XP", state.xp - xp), FloatKind::Xp, me.pos, me.height + 12.0));
    }
}
