//! Presentation of the demo run: quest tracker (under the minimap), quest toasts and sounds,
//! the boss bar of scripted encounters, the title card on `duskhollow` and the end card.
//!
//! Debug: `DUSK_BOSS_TEST=1` shows the boss bar (Corvin's template, if any, at 62 %),
//! `DUSK_TITLE_TEST=1` plays the title card on any map, `DUSK_END_TEST=1` shows the end card,
//! `DUSK_QUEST_TEST=<stage>` (server side) starts the run at a stage, filling the tracker.

use crate::journal::{Journal, objective_line};
use crate::ui_input::CapturesPointer;
use crate::windows::{Hint, WindowCommand};
use crate::{
    audio::PlaySfx,
    chat::ChatSystemLine,
    combat_ui::UiFont,
    data::GameData,
    dialogue::{Dialogue, Modal},
    map_render::{CurrentMap, MapLoaded},
    net::{DirectorNet, Net},
    unit::{Dead, Health},
};
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use dusk_protocol::{EntityId, QuestStatus, ServerMsg};
use std::collections::HashSet;

pub struct DirectorUiPlugin;

impl Plugin for DirectorUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Quests>()
            .init_resource::<Untracked>()
            .init_resource::<Boss>()
            .init_resource::<Cards>()
            .add_systems(OnEnter(crate::state::AppState::InGame), spawn_ui)
            .add_systems(
                OnEnter(crate::state::AppState::Connecting),
                (
                    crate::state::reset::<Quests>,
                    crate::state::reset::<Untracked>,
                    crate::state::reset::<Boss>,
                    crate::state::reset::<Cards>,
                ),
            )
            .add_systems(
                Update,
                (
                    receive,
                    (update_tracker, tracker_clicks, animate_toast),
                    update_boss_bar,
                    (start_title, animate_title, show_end_card, animate_end_card).chain(),
                )
                    .chain()
                    .run_if(crate::state::in_game),
            );
    }
}

/// The title card plays on this map.
pub const TITLE_MAP: &str = "duskhollow";
const TOAST_SECS: f32 = 3.6;
const BOSS_FADE_SECS: f32 = 3.0;
/// Boss bar segments (oldschool notches).
const BOSS_SEGMENTS: usize = 10;
const BOSS_W: f32 = 460.0;
/// The lag chip drains this fraction of the bar per second.
const CHIP_DRAIN: f32 = 0.35;

const BONE: Color = Color::srgb(0.88, 0.83, 0.72);
const GOLD: Color = Color::srgb(0.95, 0.78, 0.42);
const DIM: Color = Color::srgb(0.62, 0.56, 0.48);
const IRON: Color = Color::srgb(0.30, 0.26, 0.22);
const BRONZE: Color = Color::srgb(0.55, 0.42, 0.24);
const CRIMSON: Color = Color::srgb(0.60, 0.07, 0.05);
const CHIP: Color = Color::srgb(0.85, 0.62, 0.42);

fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.9) }
}

// ---------------------------------------------------------------- state

/// The local player's quests as last reported, in the order they were taken.
#[derive(Resource, Default)]
pub struct Quests(pub Vec<dusk_protocol::QuestInfo>);

/// Quests the player hid from the tracker (journal Track / Untrack); everything else active is
/// tracked.
#[derive(Resource, Default)]
pub struct Untracked(pub HashSet<u32>);

/// A tracker entry: click opens the journal on it.
#[derive(Component)]
struct TrackerRow(u32);

#[derive(Resource, Default)]
struct Boss {
    id: Option<EntityId>,
    /// Seconds left of the fade-out after the boss fell.
    fading: Option<f32>,
    /// Displayed health ratio of the trailing chip.
    chip: f32,
}

#[derive(Resource, Default)]
struct Cards {
    /// Seconds since the title card started.
    title: Option<f32>,
    title_shown: bool,
    /// `DemoEnd` waiting for the dialogue to close: (secs, deaths).
    end_pending: Option<(u32, u32)>,
    /// Seconds since the end card appeared, and whether it is fading out.
    end: Option<(f32, bool)>,
}

#[derive(Component)]
struct Tracker;
#[derive(Component)]
struct Toast(f32);
#[derive(Component)]
struct ToastTitle;
#[derive(Component)]
struct ToastLine;
#[derive(Component)]
struct BossRoot;
#[derive(Component)]
struct BossName;
#[derive(Component)]
struct BossTitle;
#[derive(Component)]
struct BossFill;
#[derive(Component)]
struct BossChip;
#[derive(Component)]
struct TitleCard;
#[derive(Component)]
struct TitleText(f32);
#[derive(Component)]
struct EndCard;
#[derive(Component)]
struct EndStats;
#[derive(Component)]
struct EndHint;
#[derive(Component)]
struct EndRule;

fn spawn_ui(mut commands: Commands, font: Res<UiFont>, bold: Res<crate::combat_ui::UiFontBold>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let title_font = bold.0.clone();
    let tf = |size: f32| TextFont { font: title_font.clone().into(), font_size: size.into(), ..default() };

    // Quest tracker, under the minimap (minimap frame is 241x293 at the top-right).
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(8.0),
            top: Val::Px(300.0),
            width: Val::Px(226.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            ..default()
        },
        Tracker,
    ));

    // Toast, upper centre.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(21.0),
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(4.0),
                ..default()
            },
            Visibility::Hidden,
            Toast(TOAST_SECS),
        ))
        .with_children(|t| {
            t.spawn((Text::new(""), tf(24.0), TextColor(GOLD), shadow(), ToastTitle));
            t.spawn((Text::new(""), f(15.0), TextColor(BONE), shadow(), ToastLine));
        });

    // Boss bar, top centre under the unit frames.
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(136.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-BOSS_W / 2.0)),
                width: Val::Px(BOSS_W),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            BossRoot,
        ))
        .with_children(|b| {
            b.spawn(Node { column_gap: Val::Px(8.0), align_items: AlignItems::Baseline, ..default() }).with_children(
                |n| {
                    n.spawn((Text::new(""), tf(17.0), TextColor(GOLD), shadow(), BossName));
                    n.spawn((Text::new(""), f(12.0), TextColor(DIM), shadow(), BossTitle));
                },
            );
            b.spawn((
                Node {
                    width: Val::Px(BOSS_W),
                    height: Val::Px(16.0),
                    margin: UiRect::top(Val::Px(3.0)),
                    border: UiRect::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.03, 0.02, 0.02, 0.9)),
                BorderColor::all(IRON),
                BoxShadow::new(Color::BLACK.with_alpha(0.6), Val::Px(0.0), Val::Px(2.0), Val::Px(0.0), Val::Px(4.0)),
            ))
            .with_children(|bar| {
                let inner = BOSS_W - 4.0;
                let fill = |w: f32| Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    top: Val::Px(0.0),
                    width: Val::Px(w),
                    height: Val::Px(12.0),
                    ..default()
                };
                bar.spawn((fill(inner), BackgroundColor(CHIP.with_alpha(0.75)), BossChip));
                bar.spawn((fill(inner), BackgroundColor(CRIMSON), BossFill)).with_child((
                    // Darker lower half: cheap bevel.
                    Node {
                        position_type: PositionType::Absolute,
                        bottom: Val::Px(0.0),
                        width: Val::Percent(100.0),
                        height: Val::Px(5.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.3)),
                ));
                for i in 1..BOSS_SEGMENTS {
                    let x = (inner * i as f32 / BOSS_SEGMENTS as f32).round();
                    bar.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x - 1.0),
                            top: Val::Px(0.0),
                            width: Val::Px(2.0),
                            height: Val::Px(12.0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.05, 0.03, 0.02, 0.85)),
                    ));
                }
                for x in [0.0, inner - 4.0] {
                    bar.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(x),
                            top: Val::Px(4.0),
                            width: Val::Px(4.0),
                            height: Val::Px(4.0),
                            ..default()
                        },
                        BackgroundColor(BRONZE),
                    ));
                }
            });
        });

    // Title card.
    let full = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        flex_direction: FlexDirection::Column,
        align_items: AlignItems::Center,
        justify_content: JustifyContent::Center,
        row_gap: Val::Px(14.0),
        ..default()
    };
    commands
        .spawn((full.clone(), BackgroundColor(Color::BLACK), Visibility::Hidden, GlobalZIndex(200), TitleCard))
        .with_children(|c| {
            c.spawn((
                Text::new("D U S K H O L L O W"),
                tf(54.0),
                TextColor(BONE.with_alpha(0.0)),
                TextShadow { offset: Vec2::new(0.0, 3.0), color: Color::srgba(0.35, 0.03, 0.02, 0.0) },
                TitleText(0.6),
            ));
            c.spawn((
                Text::new("Another sword under the Eye."),
                f(20.0),
                TextColor(DIM.with_alpha(0.0)),
                TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.0) },
                TitleText(2.0),
            ));
        });

    // End card.
    commands.spawn((full, BackgroundColor(Color::NONE), Visibility::Hidden, GlobalZIndex(190), EndCard)).with_children(
        |c| {
            c.spawn((Text::new("The Eye half-closes. It never shuts."), tf(30.0), TextColor(BONE), shadow()));
            c.spawn((
                Node { height: Val::Px(1.0), width: Val::Px(320.0), ..default() },
                BackgroundColor(BRONZE),
                EndRule,
            ));
            c.spawn((Text::new(""), f(17.0), TextColor(GOLD), shadow(), EndStats));
            c.spawn((
                Text::new("Press any key to keep wandering"),
                f(13.0),
                TextColor(DIM),
                shadow(),
                Node { margin: UiRect::top(Val::Px(26.0)), ..default() },
                EndHint,
            ));
        },
    );
}

// ---------------------------------------------------------------- messages

#[allow(clippy::too_many_arguments)]
fn receive(
    mut msgs: MessageReader<DirectorNet>,
    mut quests: ResMut<Quests>,
    mut boss: ResMut<Boss>,
    mut cards: ResMut<Cards>,
    mut sfx: MessageWriter<PlaySfx>,
    mut chat: MessageWriter<ChatSystemLine>,
    mut toast: Query<&mut Toast>,
    mut toast_text: ParamSet<(Query<&mut Text, With<ToastTitle>>, Query<&mut Text, With<ToastLine>>)>,
) {
    let mut popped: Option<(String, String)> = None;
    let mut pop = |title: String, line: String| popped = Some((title, line));
    for DirectorNet(msg) in msgs.read() {
        match msg {
            ServerMsg::Quest(q) => {
                let old = quests.0.iter().position(|o| o.id == q.id);
                let prev = old.map(|i| quests.0[i].clone());
                match (&prev, q.status) {
                    (None, QuestStatus::Active) => {
                        sfx.write(PlaySfx::ui("quest_accept.wav"));
                        chat.write(ChatSystemLine(format!("Quest accepted: {}", q.title)));
                        pop(q.title.clone(), "Quest accepted".into());
                    }
                    // Completed quests replayed on join (no previous state) stay quiet.
                    (Some(p), QuestStatus::Done) if p.status != QuestStatus::Done => {
                        sfx.write(PlaySfx::ui("quest_complete.wav"));
                        chat.write(ChatSystemLine(format!("Quest complete: {}", q.title)));
                        pop(q.title.clone(), "Quest complete".into());
                    }
                    (_, QuestStatus::Ready) if prev.as_ref().is_none_or(|p| p.status != QuestStatus::Ready) => {
                        sfx.write(PlaySfx::ui("quest_progress.wav"));
                        chat.write(ChatSystemLine(format!("{}: {}", q.title, q.objective)));
                        pop(q.objective.clone(), q.title.clone());
                    }
                    (Some(p), QuestStatus::Active) if q.count > p.count => {
                        sfx.write(PlaySfx::ui("quest_progress.wav"));
                        let line = format!("{} {}/{}", q.objective, q.count, q.need);
                        chat.write(ChatSystemLine(line.clone()));
                        pop(String::new(), line);
                    }
                    _ => {}
                }
                match old {
                    Some(i) => quests.0[i] = q.clone(),
                    None => quests.0.push(q.clone()),
                }
            }
            ServerMsg::BossBar { boss: id } => match id {
                Some(id) => {
                    *boss = Boss { id: Some(*id), fading: None, chip: 1.0 };
                }
                // Dropped after a kill: let the empty bar linger; after a reset: hide now.
                None => {
                    if boss.fading.is_none() {
                        boss.id = None;
                    }
                }
            },
            ServerMsg::DemoEnd { secs, deaths } => {
                cards.end_pending = Some((*secs, *deaths));
            }
            _ => {}
        }
    }
    let Some((title, line)) = popped else { return };
    if let Ok(mut t) = toast_text.p0().single_mut() {
        t.0 = title;
    }
    if let Ok(mut t) = toast_text.p1().single_mut() {
        t.0 = line;
    }
    if let Ok(mut t) = toast.single_mut() {
        t.0 = 0.0;
    }
}

fn update_tracker(
    mut commands: Commands,
    quests: Res<Quests>,
    untracked: Res<Untracked>,
    font: Res<UiFont>,
    tracker: Query<Entity, With<Tracker>>,
) {
    if !quests.is_changed() && !untracked.is_changed() {
        return;
    }
    let Ok(root) = tracker.single() else { return };
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    commands.entity(root).despawn_related::<Children>();
    commands.entity(root).with_children(|t| {
        for q in quests.0.iter().filter(|q| q.status != QuestStatus::Done && !untracked.0.contains(&q.id)) {
            let ready = q.status == QuestStatus::Ready;
            t.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::new(Val::Px(8.0), Val::Px(6.0), Val::Px(4.0), Val::Px(5.0)),
                    border: UiRect::left(Val::Px(2.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.04, 0.03, 0.03, 0.62)),
                BorderColor::all(if ready { GOLD } else { BRONZE }),
                Button,
                CapturesPointer,
                TrackerRow(q.id),
                Hint::new(q.title.clone()).with_body("Click to open the journal (J)"),
            ))
            .with_children(|b| {
                b.spawn((Text::new(q.title.clone()), f(14.0), TextColor(GOLD), shadow()));
                b.spawn((
                    Text::new(format!("- {}", objective_line(q))),
                    f(12.0),
                    TextColor(if ready { GOLD } else { BONE }),
                    shadow(),
                ));
            });
        }
    });
}

fn tracker_clicks(
    rows: Query<(&Interaction, &TrackerRow), Changed<Interaction>>,
    mut journal: ResMut<Journal>,
    mut out: MessageWriter<WindowCommand>,
) {
    for (i, r) in &rows {
        if *i == Interaction::Pressed {
            crate::journal::open_on(&mut journal, &mut out, r.0);
        }
    }
}

fn animate_toast(
    time: Res<Time>,
    mut toast: Query<(&mut Toast, &mut Visibility)>,
    mut texts: Query<&mut TextColor, Or<(With<ToastTitle>, With<ToastLine>)>>,
) {
    let Ok((mut t, mut vis)) = toast.single_mut() else { return };
    if t.0 >= TOAST_SECS {
        vis.set_if_neq(Visibility::Hidden);
        return;
    }
    t.0 += time.delta_secs();
    vis.set_if_neq(Visibility::Inherited);
    let a = (t.0 / 0.25).min(1.0) * ((TOAST_SECS - t.0) / 0.8).clamp(0.0, 1.0);
    for mut c in &mut texts {
        c.0.set_alpha(a);
    }
}

// ---------------------------------------------------------------- boss bar

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_boss_bar(
    time: Res<Time>,
    data: Res<GameData>,
    net: Res<Net>,
    mut boss: ResMut<Boss>,
    units: Query<(&crate::unit::Npc, &Health, Has<Dead>)>,
    mut root: Query<&mut Visibility, With<BossRoot>>,
    mut texts: ParamSet<(Query<&mut Text, With<BossName>>, Query<&mut Text, With<BossTitle>>)>,
    mut fills: ParamSet<(Query<&mut Node, With<BossFill>>, Query<&mut Node, With<BossChip>>)>,
    mut test: Local<Option<f32>>,
) {
    let Ok(mut vis) = root.single_mut() else { return };
    let testing = std::env::var_os("DUSK_BOSS_TEST").is_some();
    let (entry, ratio) = match boss.id.and_then(|id| net.entities.get(&id)).and_then(|e| units.get(*e).ok()) {
        Some((npc, h, dead)) => {
            let r = if dead { 0.0 } else { (h.hp.max(0) as f32 / h.max.max(1) as f32).clamp(0.0, 1.0) };
            (Some(npc.entry), r)
        }
        None if testing => {
            // Debug: drain from 100% to 62% so the chip shows.
            let t = test.get_or_insert(1.0);
            *t = (*t - time.delta_secs() * 0.2).max(0.62);
            (Some(50004), *t)
        }
        None => {
            boss.id = None;
            vis.set_if_neq(Visibility::Hidden);
            return;
        }
    };
    if ratio <= 0.0 && boss.fading.is_none() {
        boss.fading = Some(BOSS_FADE_SECS);
    }
    if let Some(f) = boss.fading.as_mut() {
        *f -= time.delta_secs();
        if *f <= 0.0 {
            *boss = Boss::default();
            vis.set_if_neq(Visibility::Hidden);
            return;
        }
    }
    vis.set_if_neq(Visibility::Inherited);
    let tpl = entry.and_then(|e| data.npc_templates.get(&e));
    let name = tpl.map(|t| t.name.clone()).unwrap_or_else(|| "Hollowed Warden Corvin".into());
    let title = tpl.map(|t| t.subname.clone()).unwrap_or_default();
    if let Ok(mut t) = texts.p0().single_mut() {
        if t.0 != name {
            t.0 = name;
        }
    }
    if let Ok(mut t) = texts.p1().single_mut() {
        if t.0 != title {
            t.0 = title;
        }
    }
    // The chip trails the real value, so big hits read as a chunk falling away.
    boss.chip = if boss.chip < ratio { ratio } else { (boss.chip - CHIP_DRAIN * time.delta_secs()).max(ratio) };
    let inner = BOSS_W - 4.0;
    if let Ok(mut n) = fills.p0().single_mut() {
        n.width = Val::Px((inner * ratio).round());
    }
    if let Ok(mut n) = fills.p1().single_mut() {
        n.width = Val::Px((inner * boss.chip).round());
    }
}

// ---------------------------------------------------------------- title + end cards

fn start_title(
    mut loaded: MessageReader<MapLoaded>,
    map: Res<CurrentMap>,
    mut cards: ResMut<Cards>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    if loaded.read().count() == 0 || cards.title_shown {
        return;
    }
    if map.name == TITLE_MAP || std::env::var_os("DUSK_TITLE_TEST").is_some() {
        cards.title = Some(0.0);
        cards.title_shown = true;
        sfx.write(PlaySfx::ui("title_sting.wav"));
    }
}

/// Black hold, the gorge fades in under the title, then the title goes.
fn animate_title(
    time: Res<Time>,
    mut cards: ResMut<Cards>,
    mut card: Query<(&mut Visibility, &mut BackgroundColor), With<TitleCard>>,
    mut texts: Query<(&TitleText, &mut TextColor, &mut TextShadow)>,
) {
    const HOLD: f32 = 2.0;
    const FADE: f32 = 2.6;
    const END: f32 = 7.5;
    let Ok((mut vis, mut bg)) = card.single_mut() else { return };
    let Some(t) = cards.title.as_mut() else { return };
    *t += time.delta_secs().min(0.1);
    let t = *t;
    if t > END {
        cards.title = None;
        *vis = Visibility::Hidden;
        return;
    }
    *vis = Visibility::Inherited;
    bg.0 = Color::BLACK.with_alpha(1.0 - ((t - HOLD) / FADE).clamp(0.0, 1.0));
    for (start, mut c, mut s) in &mut texts {
        let a = ((t - start.0) / 1.0).clamp(0.0, 1.0) * ((END - 0.2 - t) / 1.4).clamp(0.0, 1.0);
        c.0.set_alpha(a);
        s.color.set_alpha(a * 0.9);
    }
}

#[allow(clippy::too_many_arguments)]
fn show_end_card(
    mut cards: ResMut<Cards>,
    dialogue: Res<Dialogue>,
    mut modal: ResMut<Modal>,
    mut sfx: MessageWriter<PlaySfx>,
    mut stats: Query<&mut Text, With<EndStats>>,
    time: Res<Time>,
    mut test: Local<bool>,
) {
    if !*test && std::env::var_os("DUSK_END_TEST").is_some() && time.elapsed_secs() > 2.5 {
        *test = true;
        cards.end_pending = Some((754, 2));
    }
    // Wait for Ysolde's last words to be read.
    if dialogue.is_open() || cards.end.is_some() {
        return;
    }
    let Some((secs, deaths)) = cards.end_pending.take() else { return };
    if let Ok(mut t) = stats.single_mut() {
        let died = match deaths {
            0 => "Never fell".to_string(),
            1 => "Fell once".to_string(),
            n => format!("Fell {n} times"),
        };
        t.0 = format!("{}:{:02}    -    {died}", secs / 60, secs % 60);
    }
    cards.end = Some((0.0, false));
    modal.card = true;
    sfx.write(PlaySfx::ui("end_sting.wav"));
}

#[allow(clippy::type_complexity)]
fn animate_end_card(
    time: Res<Time>,
    mut cards: ResMut<Cards>,
    mut modal: ResMut<Modal>,
    mut keys: MessageReader<KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut card: Query<(&mut Visibility, &mut BackgroundColor), With<EndCard>>,
    mut texts: Query<(&mut TextColor, Option<&mut TextShadow>, Has<EndHint>), Without<TitleText>>,
    children: Query<&Children, With<EndCard>>,
    mut lines: Query<&mut BackgroundColor, (With<EndRule>, Without<EndCard>)>,
) {
    let pressed = keys.read().count() > 0 || mouse.get_just_pressed().next().is_some();
    let Ok((mut vis, mut bg)) = card.single_mut() else { return };
    let Some((t, closing)) = cards.end.as_mut() else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    let dt = time.delta_secs().min(0.1);
    if *closing {
        *t -= dt * 2.0;
    } else {
        *t += dt;
        // A short guard so the key that finished the dialogue doesn't dismiss the card.
        if pressed && *t > 1.5 {
            *closing = true;
            *t = t.min(1.6);
        }
    }
    let (t, closing) = (*t, *closing);
    if closing && t <= 0.0 {
        cards.end = None;
        modal.card = false;
        vis.set_if_neq(Visibility::Hidden);
        return;
    }
    vis.set_if_neq(Visibility::Inherited);
    let a = (t / 1.6).clamp(0.0, 1.0);
    bg.0 = Color::BLACK.with_alpha(0.82 * a);
    let pulse = 0.55 + 0.45 * (time.elapsed_secs() * 2.2).sin().abs();
    if let Ok(kids) = children.single() {
        for &k in kids {
            if let Ok((mut c, s, hint)) = texts.get_mut(k) {
                c.0.set_alpha(if hint { a * pulse } else { a });
                if let Some(mut s) = s {
                    s.color.set_alpha(a * 0.9);
                }
            }
            if let Ok(mut b) = lines.get_mut(k) {
                b.0.set_alpha(a);
            }
        }
    }
}
