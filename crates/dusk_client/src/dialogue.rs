//! Talking to NPCs: click (left or right) a friendly NPC to walk up and `Interact`; the server
//! answers with `ServerMsg::Dialogue`, shown in an oldschool window with the NPC's portrait,
//! type-on text and numbered choices (keys `1`-`4` or click; `Esc` closes). Also draws the
//! quest givers' `!` / `?` head markers.
//!
//! Debug: `DUSK_DIALOGUE_TEST=<npc entry>` walks up to the nearest such NPC and talks to it.

use crate::{
    audio::PlaySfx,
    combat_ui::{UiFont, pick_npc},
    data::GameData,
    hud::Portraits,
    minimap::overlay_layer,
    net::{DirectorNet, Net, PlayerState},
    player::{MainCamera, Player},
    ui_input::{CapturesPointer, UiInputCaptured},
    unit::{Dead, Npc, Unit},
};
use bevy::input::InputSystems;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dusk_formats::db::faction;
use dusk_protocol::{ClientMsg, EntityId, QuestMarker, ServerMsg};
use std::collections::HashMap;

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<InteractTarget>()
            .init_resource::<Dialogue>()
            .init_resource::<Modal>()
            .init_resource::<QuestMarkers>()
            .add_systems(Startup, spawn_window.after(crate::combat_ui::load_font))
            .add_systems(PreUpdate, capture_keyboard.after(InputSystems).after(crate::chat::type_chat))
            .add_systems(
                Update,
                (
                    (click_npc, approach, receive, keys, choice_clicks, auto_close).chain(),
                    (render, type_on).chain(),
                    head_markers,
                ),
            )
            .add_systems(Update, debug_talk.run_if(|| std::env::var_os("DUSK_DIALOGUE_TEST").is_some()));
    }
}

/// Server id of the NPC the player is walking up to talk to (`player::move_player` walks there).
#[derive(Resource, Default)]
pub struct InteractTarget(pub Option<EntityId>);

/// Full-screen or keyboard-owning UI that is open (dialogue, title/end cards): while set, the
/// keyboard belongs to it, not to gameplay.
#[derive(Resource, Default)]
pub struct Modal {
    pub dialogue: bool,
    pub card: bool,
    /// The chat input had the keyboard this frame (keys go there, not to us).
    pub chat_typing: bool,
}

/// Start talking from this far (cells); the server accepts up to 3.5.
const TALK_RANGE: f32 = 2.4;
/// The window closes when the speaker gets further than this.
const CLOSE_RANGE: f32 = 6.0;
/// Type-on speed, characters per second.
const TYPE_CPS: f32 = 110.0;
/// After a choice, wait this long for a follow-up page before closing.
const FOLLOW_UP_SECS: f32 = 0.4;
const MAX_CHOICES: usize = 4;
const WINDOW_W: f32 = 452.0;

const PANEL_BG: Color = Color::srgba(0.055, 0.045, 0.04, 0.96);
const IRON: Color = Color::srgb(0.30, 0.26, 0.22);
const BRONZE: Color = Color::srgb(0.55, 0.42, 0.24);
const OXBLOOD: Color = Color::srgb(0.30, 0.07, 0.06);
const BONE: Color = Color::srgb(0.88, 0.83, 0.72);
const GOLD: Color = Color::srgb(0.95, 0.78, 0.42);
const DIM: Color = Color::srgb(0.55, 0.50, 0.44);
const CHOICE_HOVER: Color = Color::srgba(0.45, 0.10, 0.08, 0.55);

/// The open dialogue.
#[derive(Resource, Default)]
pub struct Dialogue {
    open: Option<Page>,
}

impl Dialogue {
    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }
}

struct Page {
    speaker: EntityId,
    entry: Option<i64>,
    text: String,
    choices: Vec<String>,
    /// Characters shown so far.
    shown: f32,
    /// A choice was sent; close if no follow-up arrives before this runs out.
    awaiting: Option<f32>,
    /// Contents changed: rebuild the window.
    dirty: bool,
}

impl Page {
    fn revealed(&self) -> bool {
        self.shown as usize >= self.text.chars().count()
    }
}

#[derive(Component)]
struct Window;
#[derive(Component)]
struct SpeakerName;
#[derive(Component)]
struct SpeakerTitle;
#[derive(Component)]
struct PortraitNode;
/// Body text: first span revealed, second span still hidden (keeps the wrap stable).
#[derive(Component)]
struct BodyText;
#[derive(Component)]
struct HiddenSpan;
#[derive(Component)]
struct ChoiceList;
#[derive(Component)]
struct Choice(usize);
#[derive(Component)]
struct CloseButton;

fn abs(left: f32, top: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

fn spawn_window(mut commands: Commands, font: Res<UiFont>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let shadow = TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.9) };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(24.0),
                top: Val::Px(126.0),
                width: Val::Px(WINDOW_W),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(Val::Px(2.0)),
                ..default()
            },
            BackgroundColor(PANEL_BG),
            BorderColor::all(IRON),
            BoxShadow::new(Color::BLACK.with_alpha(0.7), Val::Px(4.0), Val::Px(6.0), Val::Px(0.0), Val::Px(8.0)),
            CapturesPointer,
            Visibility::Hidden,
            GlobalZIndex(20),
            Window,
        ))
        .with_children(|w| {
            // Header strip: name + title, close box.
            w.spawn((
                Node {
                    height: Val::Px(30.0),
                    padding: UiRect::axes(Val::Px(104.0), Val::Px(0.0)),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(OXBLOOD),
                BorderColor::all(BRONZE.with_alpha(0.6)),
            ))
            .with_children(|h| {
                h.spawn((Text::new(""), f(16.0), TextColor(GOLD), shadow, SpeakerName));
                h.spawn((Text::new(""), f(12.0), TextColor(DIM), shadow, SpeakerTitle));
            });
            // Rivets in the corners, like the rest of the skin.
            for (x, y) in [(3.0, 3.0), (WINDOW_W - 11.0, 3.0)] {
                w.spawn((abs(x, y, 4.0, 4.0), BackgroundColor(BRONZE)));
            }
            w.spawn((abs(WINDOW_W - 30.0, 4.0, 22.0, 20.0), Button, CapturesPointer, CloseButton)).with_child((
                Text::new("x"),
                f(15.0),
                TextColor(BONE),
                Node { margin: UiRect::left(Val::Px(6.0)), ..default() },
            ));
            // Portrait in a bronze ring, hanging over the header.
            w.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(12.0),
                    top: Val::Px(-14.0),
                    width: Val::Px(84.0),
                    height: Val::Px(84.0),
                    border: UiRect::all(Val::Px(3.0)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(Color::BLACK),
                BorderColor::all(BRONZE),
                GlobalZIndex(21),
            ))
            .with_child((
                Node { width: Val::Px(78.0), height: Val::Px(78.0), ..default() },
                ImageNode::default(),
                Visibility::Hidden,
                PortraitNode,
            ));
            // Body text.
            w.spawn(Node {
                padding: UiRect {
                    left: Val::Px(104.0),
                    right: Val::Px(16.0),
                    top: Val::Px(12.0),
                    bottom: Val::Px(10.0),
                },
                min_height: Val::Px(70.0),
                ..default()
            })
            .with_child((Text::new(""), f(14.0), TextColor(BONE), TextLayout::default(), shadow, BodyText))
            .with_children(|_| {});
            // Choices.
            w.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect {
                        left: Val::Px(14.0),
                        right: Val::Px(14.0),
                        top: Val::Px(6.0),
                        bottom: Val::Px(4.0),
                    },
                    border: UiRect::top(Val::Px(1.0)),
                    row_gap: Val::Px(2.0),
                    ..default()
                },
                BorderColor::all(IRON),
                ChoiceList,
            ));
            w.spawn((
                Node { padding: UiRect::new(Val::Px(14.0), Val::Px(14.0), Val::Px(2.0), Val::Px(6.0)), ..default() },
                Text::new("1-4 choose    Esc close"),
                f(11.0),
                TextColor(DIM.with_alpha(0.7)),
            ));
        });
}

/// Keeps the keyboard away from gameplay while a modal is open. Runs after the chat input
/// decided whether it is typing.
fn capture_keyboard(mut modal: ResMut<Modal>, dialogue: Res<Dialogue>, mut captured: ResMut<UiInputCaptured>) {
    modal.chat_typing = captured.keyboard;
    modal.dialogue = dialogue.is_open();
    if (modal.dialogue || modal.card) && !captured.keyboard {
        captured.keyboard = true;
    }
}

/// Left or right click on a friendly NPC: walk up to it and talk.
#[allow(clippy::too_many_arguments)]
fn click_npc(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Query<&bevy::window::Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    data: Res<GameData>,
    net: Res<Net>,
    mut state: ResMut<PlayerState>,
    mut target: ResMut<InteractTarget>,
    units: Query<(Entity, &Unit, &Npc, &Transform), (Without<Dead>, Without<Player>)>,
    captured: Res<UiInputCaptured>,
) {
    let clicked = mouse.just_pressed(MouseButton::Left) || mouse.just_pressed(MouseButton::Right);
    if !clicked || state.dead || captured.pointer {
        return;
    }
    let (Ok(window), Ok((cam, cam_tf))) = (window.single(), camera.single()) else { return };
    let Some(at) = window.cursor_position().and_then(|c| cam.viewport_to_world_2d(cam_tf, c).ok()) else { return };
    let Some((e, npc)) = pick_npc(at, units.iter()) else { return };
    let friendly = data.npc_templates.get(&npc.entry).is_some_and(|t| t.faction == faction::FRIENDLY);
    if let Some(id) = net.entity_id(e).filter(|_| friendly) {
        if state.target.take().is_some() {
            net.send(ClientMsg::StopAttack);
        }
        target.0 = Some(id);
    }
}

/// Talks as soon as the player is close enough to the NPC they walk to.
fn approach(
    net: Res<Net>,
    state: Res<PlayerState>,
    mut target: ResMut<InteractTarget>,
    player: Query<&Unit, With<Player>>,
    units: Query<&Unit, Without<Player>>,
) {
    let Some(id) = target.0 else { return };
    let (Ok(me), Some(other)) = (player.single(), net.entities.get(&id).and_then(|e| units.get(*e).ok())) else {
        target.0 = None;
        return;
    };
    if state.dead {
        target.0 = None;
    } else if me.pos.distance(other.pos) <= TALK_RANGE {
        net.send(ClientMsg::Interact { target: id });
        target.0 = None;
    }
}

fn receive(
    mut msgs: MessageReader<DirectorNet>,
    mut dialogue: ResMut<Dialogue>,
    mut markers: ResMut<QuestMarkers>,
    net: Res<Net>,
    npcs: Query<&Npc>,
    mut sfx: MessageWriter<PlaySfx>,
) {
    for DirectorNet(msg) in msgs.read() {
        match msg {
            ServerMsg::Dialogue { speaker, text, choices } => {
                let fresh = dialogue.open.as_ref().is_none_or(|p| p.speaker != *speaker);
                if fresh {
                    sfx.write(PlaySfx::ui("dialogue_open.wav"));
                }
                let entry = net.entities.get(speaker).and_then(|e| npcs.get(*e).ok()).map(|n| n.entry);
                let mut choices: Vec<String> = choices.iter().take(MAX_CHOICES).cloned().collect();
                if choices.is_empty() {
                    choices.push("Goodbye.".into());
                }
                dialogue.open = Some(Page {
                    speaker: *speaker,
                    entry,
                    text: text.clone(),
                    choices,
                    shown: 0.0,
                    awaiting: None,
                    dirty: true,
                });
            }
            ServerMsg::QuestMarker { npc, marker } => {
                markers.0.insert(*npc, *marker);
            }
            _ => {}
        }
    }
}

fn choose(dialogue: &mut Dialogue, net: &Net, index: usize) {
    let Some(page) = dialogue.open.as_mut().filter(|p| p.awaiting.is_none()) else { return };
    if index >= page.choices.len() {
        return;
    }
    net.send(ClientMsg::DialogueChoice { speaker: page.speaker, index: index as u8 });
    page.awaiting = Some(FOLLOW_UP_SECS);
}

fn keys(keys: Res<ButtonInput<KeyCode>>, modal: Res<Modal>, net: Res<Net>, mut dialogue: ResMut<Dialogue>) {
    if modal.chat_typing {
        return;
    }
    let Some(page) = dialogue.open.as_mut() else { return };
    if keys.just_pressed(KeyCode::Escape) {
        dialogue.open = None;
        return;
    }
    if !page.revealed() && keys.any_just_pressed([KeyCode::Space, KeyCode::Enter]) {
        page.shown = f32::MAX;
        return;
    }
    const DIGITS: [KeyCode; MAX_CHOICES] = [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3, KeyCode::Digit4];
    const NUMPAD: [KeyCode; MAX_CHOICES] = [KeyCode::Numpad1, KeyCode::Numpad2, KeyCode::Numpad3, KeyCode::Numpad4];
    if let Some(i) = (0..MAX_CHOICES).find(|i| keys.just_pressed(DIGITS[*i]) || keys.just_pressed(NUMPAD[*i])) {
        choose(&mut dialogue, &net, i);
    }
}

#[allow(clippy::type_complexity)]
fn choice_clicks(
    net: Res<Net>,
    mut dialogue: ResMut<Dialogue>,
    mut choices: Query<(&Interaction, &Choice, &mut BackgroundColor), Changed<Interaction>>,
    close: Query<&Interaction, (Changed<Interaction>, With<CloseButton>)>,
    body: Query<&Interaction, (Changed<Interaction>, With<Window>)>,
) {
    for (interaction, choice, mut bg) in &mut choices {
        bg.0 = if *interaction == Interaction::None { Color::NONE } else { CHOICE_HOVER };
        if *interaction == Interaction::Pressed {
            choose(&mut dialogue, &net, choice.0);
        }
    }
    if close.iter().any(|i| *i == Interaction::Pressed) {
        dialogue.open = None;
    }
    // Clicking the window skips the type-on.
    if body.iter().any(|i| *i == Interaction::Pressed) {
        if let Some(p) = dialogue.open.as_mut() {
            p.shown = f32::MAX;
        }
    }
}

/// Closes on walking away, death, the speaker vanishing, or a choice without follow-up.
fn auto_close(
    time: Res<Time>,
    net: Res<Net>,
    state: Res<PlayerState>,
    mut dialogue: ResMut<Dialogue>,
    player: Query<&Unit, With<Player>>,
    units: Query<&Unit, (Without<Player>, Without<Dead>)>,
) {
    let Some(page) = dialogue.open.as_mut() else { return };
    if let Some(t) = page.awaiting.as_mut() {
        *t -= time.delta_secs();
    }
    let speaker = net.entities.get(&page.speaker).and_then(|e| units.get(*e).ok());
    let far = match (player.single(), speaker) {
        (Ok(me), Some(s)) => me.pos.distance(s.pos) > CLOSE_RANGE,
        _ => true,
    };
    if far || state.dead || page.awaiting.is_some_and(|t| t <= 0.0) {
        dialogue.open = None;
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn render(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    mut portraits: ResMut<Portraits>,
    mut dialogue: ResMut<Dialogue>,
    mut window: Query<&mut Visibility, With<Window>>,
    mut texts: ParamSet<(Query<&mut Text, With<SpeakerName>>, Query<&mut Text, With<SpeakerTitle>>)>,
    mut portrait: Query<(&mut ImageNode, &mut Visibility), (With<PortraitNode>, Without<Window>)>,
    body: Query<Entity, With<BodyText>>,
    list: Query<Entity, With<ChoiceList>>,
) {
    let Ok(mut vis) = window.single_mut() else { return };
    let Some(page) = dialogue.open.as_mut() else {
        vis.set_if_neq(Visibility::Hidden);
        return;
    };
    vis.set_if_neq(Visibility::Inherited);
    // The portrait may finish baking after the page opened.
    if let Ok((mut img, mut pvis)) = portrait.single_mut() {
        match page.entry.and_then(|e| portraits.npc(&data, &assets, e)) {
            Some(h) => {
                if img.image != h {
                    img.image = h;
                }
                pvis.set_if_neq(Visibility::Inherited);
            }
            None => {
                pvis.set_if_neq(Visibility::Hidden);
            }
        }
    }
    if !page.dirty {
        return;
    }
    page.dirty = false;
    let tpl = page.entry.and_then(|e| data.npc_templates.get(&e));
    if let Ok(mut t) = texts.p0().single_mut() {
        t.0 = tpl.map(|t| t.name.clone()).unwrap_or_default();
    }
    if let Ok(mut t) = texts.p1().single_mut() {
        t.0 = tpl.map(|t| t.subname.clone()).unwrap_or_default();
    }
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    if let Ok(body) = body.single() {
        commands.entity(body).despawn_related::<Children>();
        commands.entity(body).with_child((
            TextSpan::new(page.text.clone()),
            f(14.0),
            TextColor(Color::NONE),
            HiddenSpan,
        ));
    }
    if let Ok(list) = list.single() {
        commands.entity(list).despawn_related::<Children>();
        commands.entity(list).with_children(|l| {
            for (i, c) in page.choices.iter().enumerate() {
                l.spawn((
                    Node { padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)), ..default() },
                    Button,
                    CapturesPointer,
                    BackgroundColor(Color::NONE),
                    Visibility::Hidden,
                    Choice(i),
                ))
                .with_children(|b| {
                    b.spawn((
                        Text::new(format!("{}.", i + 1)),
                        f(14.0),
                        TextColor(BRONZE),
                        Node { width: Val::Px(22.0), ..default() },
                    ));
                    b.spawn((Text::new(c.clone()), f(14.0), TextColor(GOLD)));
                });
            }
        });
    }
}

/// Reveals the body text a few characters per frame; choices appear once it is all there.
fn type_on(
    time: Res<Time>,
    mut dialogue: ResMut<Dialogue>,
    mut body: Query<&mut Text, With<BodyText>>,
    mut hidden: Query<&mut TextSpan, With<HiddenSpan>>,
    mut choices: Query<&mut Visibility, With<Choice>>,
) {
    let Some(page) = dialogue.open.as_mut() else { return };
    let total = page.text.chars().count();
    let before = page.shown as usize;
    page.shown = (page.shown + TYPE_CPS * time.delta_secs()).min(total as f32);
    let n = page.shown as usize;
    let (Ok(mut text), Ok(mut rest)) = (body.single_mut(), hidden.single_mut()) else { return };
    if n != before || text.0.is_empty() != (n == 0) || rest.0.chars().count() + text.0.chars().count() != total {
        let split = page.text.char_indices().nth(n).map_or(page.text.len(), |(i, _)| i);
        text.0 = page.text[..split].to_string();
        rest.0 = page.text[split..].to_string();
    }
    let vis = if page.revealed() { Visibility::Inherited } else { Visibility::Hidden };
    for mut v in &mut choices {
        v.set_if_neq(vis);
    }
}

// ---------------------------------------------------------------- head markers

/// Server id -> marker for this player (`ServerMsg::QuestMarker`).
#[derive(Resource, Default)]
pub struct QuestMarkers(pub HashMap<EntityId, QuestMarker>);

#[derive(Component)]
struct HeadMarker(QuestMarker);

/// Height of the marker above the head, in screen pixels (clears the name plate).
const MARKER_GAP: f32 = 34.0;
/// Glyph pixels are drawn this big.
const MARKER_SCALE: f32 = 3.0;

/// `!` and `?` as pixel glyphs (`#` gold, `+` dark outline added around them).
const GLYPH_BANG: [&str; 9] = [".##.", ".##.", ".##.", ".##.", ".##.", "....", ".##.", ".##.", "...."];
const GLYPH_ASK: [&str; 9] = [".###.", "##.##", "...##", "..##.", ".##..", ".....", ".##..", ".##..", "....."];

/// Baked marker images.
#[derive(Default)]
struct MarkerImages(Option<[Handle<Image>; 2]>);

/// Rasterises a glyph with a 1-pixel dark outline and a darker lower half (oldschool bevel).
fn glyph_image(rows: &[&str]) -> Image {
    use bevy::asset::RenderAssetUsages;
    use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
    let (gw, gh) = (rows[0].len() as i32, rows.len() as i32);
    let (w, h) = (gw + 2, gh + 2);
    let on = |x: i32, y: i32| x >= 0 && y >= 0 && x < gw && y < gh && rows[y as usize].as_bytes()[x as usize] == b'#';
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (gx, gy) = (x - 1, y - 1);
            let px = if on(gx, gy) {
                if gy < gh / 2 { [255, 214, 120, 255] } else { [222, 160, 70, 255] }
            } else if (-1..=1).any(|dy| (-1..=1).any(|dx| on(gx + dx, gy + dy))) {
                [30, 12, 8, 255]
            } else {
                [0, 0, 0, 0]
            };
            data.extend(px);
        }
    }
    Image::new(
        Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    )
}

#[allow(clippy::too_many_arguments)]
fn head_markers(
    mut commands: Commands,
    time: Res<Time>,
    net: Res<Net>,
    mut images: ResMut<Assets<Image>>,
    mut baked: Local<MarkerImages>,
    markers: Res<QuestMarkers>,
    units: Query<&Unit, Without<Dead>>,
    mut existing: Query<(Entity, &HeadMarker, &ChildOf, &mut Transform)>,
) {
    let glyphs =
        baked.0.get_or_insert_with(|| [images.add(glyph_image(&GLYPH_BANG)), images.add(glyph_image(&GLYPH_ASK))]);
    let mut have: HashMap<Entity, (Entity, QuestMarker)> = HashMap::new();
    let bob = ((time.elapsed_secs() * 2.5).sin() * 2.0).round();
    for (e, m, parent, mut t) in &mut existing {
        have.insert(parent.parent(), (e, m.0));
        if let Ok(u) = units.get(parent.parent()) {
            t.translation.y = u.height + (MARKER_GAP + bob) / u.scale;
        }
    }
    for (id, marker) in &markers.0 {
        let Some(&unit) = net.entities.get(id) else { continue };
        let current = have.remove(&unit);
        if current.map(|c| c.1) == Some(*marker) {
            continue;
        }
        if let Some((old, _)) = current {
            commands.entity(old).despawn();
        }
        let Ok(u) = units.get(unit) else { continue };
        let image = match marker {
            QuestMarker::None => continue,
            QuestMarker::Available => glyphs[0].clone(),
            QuestMarker::TurnIn => glyphs[1].clone(),
        };
        let s = u.scale;
        commands.spawn((
            ChildOf(unit),
            HeadMarker(*marker),
            Sprite { image, ..default() },
            bevy::sprite::Anchor::BOTTOM_CENTER,
            Transform::from_xyz(0.0, u.height + MARKER_GAP / s, 501.0).with_scale(Vec3::splat(MARKER_SCALE / s)),
            overlay_layer(),
        ));
    }
}

/// `DUSK_DIALOGUE_TEST=<entry>`: walk to the nearest NPC of that entry (once) and talk.
fn debug_talk(
    net: Res<Net>,
    time: Res<Time>,
    mut done: Local<bool>,
    mut target: ResMut<InteractTarget>,
    player: Query<&Unit, With<Player>>,
    units: Query<(Entity, &Unit, &Npc)>,
) {
    let want: i64 = std::env::var("DUSK_DIALOGUE_TEST").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let Ok(me) = player.single() else { return };
    if *done || time.elapsed_secs() < 2.0 {
        return;
    }
    let nearest = units
        .iter()
        .filter(|(_, _, n)| n.entry == want)
        .min_by(|a, b| a.1.pos.distance(me.pos).total_cmp(&b.1.pos.distance(me.pos)));
    if let Some(id) = nearest.and_then(|n| net.entity_id(n.0)) {
        target.0 = Some(id);
        *done = true;
    }
}
