//! Quest journal (`J`, `ui_journal.png` from `tools/artgen/windows_ui.py`): active and
//! completed quests on the left, the selected one on the right (objective + progress, log
//! text, giver, where to go, reward) and a Track / Untrack button feeding the quest tracker.
//!
//! Quest data is the director's `ServerMsg::Quest` stream, kept in `director_ui::Quests`.

use crate::{
    combat_ui::UiFont,
    data::GameData,
    director_ui::{Quests, Untracked},
    ui_input::CapturesPointer,
    windows::{self, ButtonArt, Hint, HoverTint, TintBase, UiWindow, WindowCommand, WindowId, Windows},
};
use bevy::prelude::*;
use dusk_protocol::{QuestInfo, QuestStatus};

pub struct JournalPlugin;

impl Plugin for JournalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Journal>()
            .add_systems(OnEnter(crate::state::AppState::InGame), spawn_journal)
            .add_systems(OnEnter(crate::state::AppState::Connecting), crate::state::reset::<Journal>)
            .add_systems(
                Update,
                (clicks, follow_new_quests, rebuild)
                    .chain()
                    .after(windows::WindowSystems)
                    .run_if(crate::state::in_game),
            );
    }
}

/// The selected quest.
#[derive(Resource, Default)]
pub struct Journal {
    pub selected: Option<u32>,
}

const W: f32 = 580.0;
const H: f32 = 460.0;
/// Inner rects of the two wells on the art.
const LIST: Rect = Rect { min: Vec2::new(24.0, 74.0), max: Vec2::new(210.0, 392.0) };
const DETAIL: Rect = Rect { min: Vec2::new(226.0, 74.0), max: Vec2::new(556.0, 392.0) };

const GOLD: Color = Color::srgb(0.95, 0.78, 0.42);
const BONE: Color = Color::srgb(0.84, 0.79, 0.68);
const DIM: Color = Color::srgb(0.55, 0.50, 0.44);
const SELECTED: Color = Color::srgba(0.45, 0.10, 0.08, 0.55);

#[derive(Component)]
struct QuestList;
#[derive(Component)]
struct QuestRow(u32);
#[derive(Component)]
struct DetailTitle;
#[derive(Component)]
struct DetailBody;
#[derive(Component)]
struct TrackButton;
#[derive(Component)]
struct TrackNote;

fn rect_node(r: Rect) -> Node {
    windows::abs(r.min.x, r.min.y, r.width(), r.height())
}

fn spawn_journal(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>, font: Res<UiFont>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let img = |n: &str| data.asset_path(n).map(|p| assets.load(p)).unwrap_or_default();
    let track = ButtonArt::load(&data, &assets, "ui_btn_track");
    commands
        .spawn((windows::abs(0.0, 0.0, W, H), ImageNode::new(img("ui_journal.png")), UiWindow(WindowId::Journal)))
        .with_children(|w| {
            windows::spawn_drag_handle(w, WindowId::Journal, 14.0, 14.0, 500.0, 44.0);
            windows::spawn_close_button(w, &data, &assets, WindowId::Journal, 530.0, 23.0, false);
            let mut list = rect_node(LIST);
            list.flex_direction = FlexDirection::Column;
            list.padding = UiRect::all(Val::Px(6.0));
            list.row_gap = Val::Px(2.0);
            list.overflow = Overflow::clip();
            w.spawn((list, QuestList));
            // Title above the rule (y 120), body below it.
            w.spawn((
                windows::abs(DETAIL.min.x + 12.0, DETAIL.min.y + 10.0, DETAIL.width() - 24.0, 34.0),
                Text::new(""),
                f(18.0),
                TextColor(GOLD),
                TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
                DetailTitle,
            ));
            let mut body = windows::abs(DETAIL.min.x + 12.0, 130.0, DETAIL.width() - 24.0, DETAIL.max.y - 136.0);
            body.flex_direction = FlexDirection::Column;
            body.row_gap = Val::Px(6.0);
            body.overflow = Overflow::clip();
            w.spawn((body, DetailBody));
            w.spawn((
                windows::abs(226.0, 406.0, 120.0, 30.0),
                ImageNode::new(track.idle.clone()),
                track,
                Button,
                CapturesPointer,
                TrackButton,
                Hint::new("Track").with_body("Tracked quests show under the minimap."),
            ));
            w.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(358.0), top: Val::Px(414.0), ..default() },
                Text::new(""),
                f(11.0),
                TextColor(DIM),
                TrackNote,
            ));
        });
}

fn clicks(
    rows: Query<(&Interaction, &QuestRow), Changed<Interaction>>,
    track: Query<&Interaction, (Changed<Interaction>, With<TrackButton>)>,
    mut journal: ResMut<Journal>,
    mut untracked: ResMut<Untracked>,
) {
    for (i, r) in &rows {
        if *i == Interaction::Pressed && journal.selected != Some(r.0) {
            journal.selected = Some(r.0);
        }
    }
    if track.iter().any(|i| *i == Interaction::Pressed) {
        if let Some(id) = journal.selected {
            if !untracked.0.remove(&id) {
                untracked.0.insert(id);
            }
        }
    }
}

/// Newly accepted quests get selected; a stale selection falls back to the first quest.
fn follow_new_quests(
    quests: Res<Quests>,
    windows: Res<Windows>,
    mut journal: ResMut<Journal>,
    mut known: Local<usize>,
) {
    if quests.0.len() > *known {
        journal.selected = quests.0.last().map(|q| q.id);
    }
    *known = quests.0.len();
    let valid = journal.selected.is_some_and(|id| quests.0.iter().any(|q| q.id == id));
    if !valid && (windows.is_open(WindowId::Journal) || journal.selected.is_some()) {
        let first = ordered(&quests.0).first().map(|q| q.id);
        if journal.selected != first {
            journal.selected = first;
        }
    }
}

/// Active (newest first), then completed.
fn ordered(quests: &[QuestInfo]) -> Vec<&QuestInfo> {
    let mut v: Vec<&QuestInfo> = quests.iter().filter(|q| q.status != QuestStatus::Done).rev().collect();
    v.extend(quests.iter().filter(|q| q.status == QuestStatus::Done).rev());
    v
}

pub fn objective_line(q: &QuestInfo) -> String {
    if q.need > 0 { format!("{} {}/{}", q.objective, q.count.min(q.need), q.need) } else { q.objective.clone() }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn rebuild(
    mut commands: Commands,
    quests: Res<Quests>,
    journal: Res<Journal>,
    untracked: Res<Untracked>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    list: Query<Entity, With<QuestList>>,
    body: Query<Entity, With<DetailBody>>,
    mut title: Query<&mut Text, (With<DetailTitle>, Without<TrackNote>)>,
    mut note: Query<&mut Text, (With<TrackNote>, Without<DetailTitle>)>,
    mut button: Query<(&mut ButtonArt, &mut ImageNode, &mut Visibility, &mut Hint), With<TrackButton>>,
) {
    if !(quests.is_changed() || journal.is_changed() || untracked.is_changed()) {
        return;
    }
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let (Ok(list), Ok(body)) = (list.single(), body.single()) else { return };
    let order = ordered(&quests.0);
    commands.entity(list).despawn_related::<Children>();
    commands.entity(list).with_children(|l| {
        if order.is_empty() {
            l.spawn((
                Text::new("No quests yet.\n\nTalk to people with a ! over their heads."),
                f(12.0),
                TextColor(DIM),
                Node { margin: UiRect::all(Val::Px(4.0)), ..default() },
            ));
            return;
        }
        let mut section: Option<bool> = None;
        for q in &order {
            let done = q.status == QuestStatus::Done;
            if section != Some(done) {
                section = Some(done);
                l.spawn((
                    Node {
                        margin: UiRect::new(
                            Val::Px(0.0),
                            Val::Px(0.0),
                            Val::Px(if done { 10.0 } else { 0.0 }),
                            Val::Px(3.0),
                        ),
                        padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(0.0), Val::Px(2.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    BorderColor::all(Color::srgb(0.45, 0.34, 0.18)),
                ))
                .with_child((
                    Text::new(if done { "Completed" } else { "Active" }),
                    f(13.0),
                    TextColor(GOLD),
                ));
            }
            let selected = journal.selected == Some(q.id);
            l.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                    flex_shrink: 0.0,
                    ..default()
                },
                HoverTint,
                TintBase(if selected { SELECTED } else { Color::NONE }),
                BackgroundColor(if selected { SELECTED } else { Color::NONE }),
                CapturesPointer,
                QuestRow(q.id),
            ))
            .with_children(|r| {
                let color = if done { DIM } else { BONE };
                r.spawn((Text::new(q.title.clone()), f(13.0), TextColor(color)));
                let sub = match q.status {
                    QuestStatus::Ready => "Complete - return".to_string(),
                    QuestStatus::Done => "Done".to_string(),
                    QuestStatus::Active => objective_line(q),
                };
                r.spawn((Text::new(sub), f(11.0), TextColor(if q.status == QuestStatus::Ready { GOLD } else { DIM })));
            });
        }
    });

    let q = journal.selected.and_then(|id| quests.0.iter().find(|q| q.id == id));
    if let Ok(mut t) = title.single_mut() {
        t.0 = q.map(|q| q.title.clone()).unwrap_or_default();
    }
    commands.entity(body).despawn_related::<Children>();
    let tracked = q.is_some_and(|q| !untracked.0.contains(&q.id));
    if let Ok((mut art, mut image, mut vis, mut hint)) = button.single_mut() {
        let active = q.is_some_and(|q| q.status != QuestStatus::Done);
        vis.set_if_neq(if active { Visibility::Inherited } else { Visibility::Hidden });
        let base = if tracked { "ui_btn_untrack" } else { "ui_btn_track" };
        let want = ButtonArt::load(&data, &assets, base);
        if art.idle != want.idle {
            image.image = want.idle.clone();
            *art = want;
            hint.title = if tracked { "Untrack".into() } else { "Track".into() };
        }
    }
    if let Ok(mut n) = note.single_mut() {
        n.0 = match q {
            Some(q) if q.status != QuestStatus::Done && tracked => "Shown in the tracker".into(),
            Some(q) if q.status != QuestStatus::Done => "Hidden from the tracker".into(),
            _ => String::new(),
        };
    }
    let Some(q) = q else {
        commands.entity(body).with_child((Text::new("Select a quest on the left."), f(13.0), TextColor(DIM)));
        return;
    };
    commands.entity(body).with_children(|b| {
        let (label, color) = match q.status {
            QuestStatus::Active => (format!("- {}", objective_line(q)), BONE),
            QuestStatus::Ready => (format!("- {} (complete)", q.objective), GOLD),
            QuestStatus::Done => ("Completed".to_string(), DIM),
        };
        b.spawn((Text::new(label), f(14.0), TextColor(color)));
        b.spawn((
            Text::new(q.description.clone()),
            f(13.0),
            TextColor(BONE),
            Node { margin: UiRect::vertical(Val::Px(4.0)), ..default() },
        ));
        let place = match q.status {
            QuestStatus::Active => "Where",
            QuestStatus::Ready => "Turn in",
            QuestStatus::Done => "",
        };
        for (k, v) in [("Given by", &q.giver), (place, &q.location), ("Reward", &q.reward)] {
            if v.is_empty() || k.is_empty() {
                continue;
            }
            b.spawn(Node { column_gap: Val::Px(6.0), ..default() }).with_children(|r| {
                r.spawn((Text::new(format!("{k}:")), f(12.0), TextColor(GOLD.with_alpha(0.8))));
                r.spawn((Text::new(v.clone()), f(12.0), TextColor(DIM)));
            });
        }
    });
}

/// Opens the journal on a quest (tracker clicks).
pub fn open_on(journal: &mut Journal, out: &mut MessageWriter<WindowCommand>, id: u32) {
    journal.selected = Some(id);
    out.write(WindowCommand::Open(WindowId::Journal));
}
