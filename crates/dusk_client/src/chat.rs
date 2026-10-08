//! Chat panel on the original art (`game_chat_*`), bottom-left: scrollback, `Enter` to type and
//! send (`ClientMsg::Chat`), system lines (level up, deaths, unknown commands) and speech bubbles
//! (`saybox_*` 9-slice) over the speaker.
//!
//! While typing, [`UiInputCaptured::keyboard`] is set so gameplay hotkeys stay quiet.
//! Other modules can post lines with the [`ChatSystemLine`] message.

use crate::{
    combat_ui::UiFont,
    data::GameData,
    net::{Net, PlayerState, SpellNet},
    ui_input::{CapturesPointer, UiInputCaptured},
    unit::{Npc, Unit},
};
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::input::{ButtonState, InputSystems};
use bevy::prelude::*;
use bevy::ui::ComputedNode;
use dusk_protocol::{ClientMsg, ServerMsg};
use std::collections::VecDeque;

pub struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatLog>()
            .add_message::<ChatSystemLine>()
            .add_systems(Startup, spawn_chat.after(crate::combat_ui::load_font))
            .add_systems(PreUpdate, type_chat.after(InputSystems))
            .add_systems(
                Update,
                ((collect_lines, buttons, wheel_scroll), (render_log, render_input), (spawn_bubbles, place_bubbles))
                    .chain(),
            )
            .add_systems(Update, debug_chat.run_if(|| std::env::var_os("DUSK_CHAT").is_some()));
    }
}

/// Post a line to the chat log from any module (shown in the system colour).
#[derive(Message, Clone)]
pub struct ChatSystemLine(pub String);

/// Scrollback lines kept.
const MAX_LINES: usize = 200;
/// Lines shown at once (wrapped lines take more room; the top is clipped).
const VISIBLE: usize = 14;
/// Longest message the input accepts.
const MAX_INPUT: usize = 200;
/// Panel position: bottom-left, above the action bar's bottom edge.
const CHAT_BOTTOM: f32 = 70.0;
const CHAT_SIZE: Vec2 = Vec2::new(537.0, 237.0);

const SAY: Color = Color::srgb(0.93, 0.9, 0.82);
const NAME: Color = Color::srgb(0.95, 0.78, 0.4);
const SYSTEM: Color = Color::srgb(1.0, 0.85, 0.2);
const ERROR: Color = Color::srgb(1.0, 0.35, 0.3);
const HINT: Color = Color::srgba(0.8, 0.75, 0.65, 0.45);

/// One coloured run of a log line.
type Segment = (String, Color);

#[derive(Resource, Default)]
pub struct ChatLog {
    lines: VecDeque<Vec<Segment>>,
    /// Lines scrolled up from the bottom.
    scroll: usize,
    /// Text being typed (`Some` while the input is open).
    input: Option<String>,
    dirty: bool,
}

impl ChatLog {
    fn push(&mut self, line: Vec<Segment>) {
        self.lines.push_back(line);
        if self.lines.len() > MAX_LINES {
            self.lines.pop_front();
        }
        if self.scroll > 0 {
            // Keep the view where the reader left it.
            self.scroll = (self.scroll + 1).min(self.max_scroll());
        }
        self.dirty = true;
    }

    fn system(&mut self, text: impl Into<String>, color: Color) {
        self.push(vec![(text.into(), color)]);
    }

    fn max_scroll(&self) -> usize {
        self.lines.len().saturating_sub(VISIBLE)
    }

    fn scroll_by(&mut self, delta: isize) {
        self.scroll = (self.scroll as isize + delta).clamp(0, self.max_scroll() as isize) as usize;
        self.dirty = true;
    }
}

#[derive(Component)]
struct LogArea;
#[derive(Component)]
struct InputText;
#[derive(Component, Clone, Copy)]
enum ChatButton {
    Top,
    Up,
    Down,
    Bottom,
    Enter,
}

impl ChatButton {
    fn art(self) -> &'static str {
        match self {
            ChatButton::Top => "game_chat_fullup",
            ChatButton::Up => "game_chat_up",
            ChatButton::Down => "game_chat_down",
            ChatButton::Bottom => "game_chat_fulldown",
            ChatButton::Enter => "game_chat_enter",
        }
    }
}

/// Idle / hover / press textures of a button.
#[derive(Component)]
struct ButtonArt([Handle<Image>; 3]);

fn img(data: &GameData, assets: &AssetServer, name: &str) -> Handle<Image> {
    data.asset_path(name).map(|p| assets.load(p)).unwrap_or_default()
}

fn abs(pos: Vec2, size: Vec2) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(pos.x),
        top: Val::Px(pos.y),
        width: Val::Px(size.x),
        height: Val::Px(size.y),
        ..default()
    }
}

fn spawn_chat(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    mut log: ResMut<ChatLog>,
) {
    let f = TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                bottom: Val::Px(CHAT_BOTTOM),
                width: Val::Px(CHAT_SIZE.x),
                height: Val::Px(CHAT_SIZE.y),
                ..default()
            },
            ImageNode::new(img(&data, &assets, "game_chat_backdrop.png")),
            CapturesPointer,
        ))
        .with_children(|p| {
            let mut area = abs(Vec2::new(16.0, 10.0), Vec2::new(484.0, 184.0));
            area.overflow = Overflow::clip();
            area.flex_direction = FlexDirection::Column;
            area.justify_content = JustifyContent::FlexEnd;
            p.spawn((area, LogArea));

            let mut input = abs(Vec2::new(18.0, 201.0), Vec2::new(420.0, 30.0));
            input.align_items = AlignItems::Center;
            input.overflow = Overflow::clip();
            p.spawn((input, Text::new(""), f.clone(), TextColor(HINT), InputText));

            for (button, pos, size) in [
                (ChatButton::Top, Vec2::new(510.0, 10.0), Vec2::new(17.0, 18.0)),
                (ChatButton::Up, Vec2::new(510.0, 31.0), Vec2::new(17.0, 11.0)),
                (ChatButton::Down, Vec2::new(510.0, 162.0), Vec2::new(17.0, 11.0)),
                (ChatButton::Bottom, Vec2::new(510.0, 176.0), Vec2::new(17.0, 17.0)),
                (ChatButton::Enter, Vec2::new(447.0, 196.0), Vec2::new(82.0, 40.0)),
            ] {
                let art = ["idle", "hover", "press"].map(|s| img(&data, &assets, &format!("{}_{s}.png", button.art())));
                p.spawn((abs(pos, size), Button, ImageNode::new(art[0].clone()), ButtonArt(art), button));
            }
        });
    log.system("Welcome to Duskhollow! Press Enter to chat, /help for commands.", SYSTEM);
}

/// Text entry. Runs in `PreUpdate` so every `Update` system sees the final capture state.
fn type_chat(
    mut keys: MessageReader<KeyboardInput>,
    mut log: ResMut<ChatLog>,
    mut captured: ResMut<UiInputCaptured>,
    net: Res<Net>,
    data: Res<GameData>,
) {
    let was_typing = log.input.is_some();
    for k in keys.read() {
        if k.state != ButtonState::Pressed {
            continue;
        }
        let Some(input) = log.input.as_mut() else {
            if matches!(k.key_code, KeyCode::Enter | KeyCode::NumpadEnter) {
                log.input = Some(String::new());
                log.dirty = true;
            }
            continue;
        };
        match (&k.logical_key, k.key_code) {
            (_, KeyCode::Enter | KeyCode::NumpadEnter) => {
                let text = log.input.take().unwrap_or_default();
                submit(&text, &mut log, &net, &data);
            }
            (_, KeyCode::Escape) => log.input = None,
            (Key::Backspace, _) => {
                input.pop();
            }
            _ => {
                if let Some(t) = &k.text {
                    for c in t.chars().filter(|c| !c.is_control()) {
                        if input.chars().count() < MAX_INPUT {
                            input.push(c);
                        }
                    }
                }
            }
        }
        log.dirty = true;
    }
    let capture = was_typing || log.input.is_some();
    if captured.keyboard != capture {
        captured.keyboard = capture;
    }
}

/// Sends a typed line: plain text and `/say` go to the server, other slash commands are
/// answered locally.
fn submit(text: &str, log: &mut ChatLog, net: &Net, data: &GameData) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    log.scroll = 0;
    let Some(cmd) = text.strip_prefix('/') else {
        net.send(ClientMsg::Chat { text: text.to_string() });
        return;
    };
    let (name, rest) = cmd.split_once(' ').unwrap_or((cmd, ""));
    match name.to_lowercase().as_str() {
        "say" | "s" => {
            if !rest.trim().is_empty() {
                net.send(ClientMsg::Chat { text: rest.trim().to_string() });
            }
        }
        "help" | "?" => {
            let help = std::fs::read_to_string(data.root.join("scripts/text/help.txt")).unwrap_or_default();
            for line in help.lines().filter(|l| !l.trim().is_empty()) {
                log.system(line.trim(), SYSTEM);
            }
        }
        other => log.system(format!("/{other} is not available yet."), ERROR),
    }
}

/// Server chat, level ups, deaths and [`ChatSystemLine`]s into the log.
fn collect_lines(
    mut net_msgs: MessageReader<SpellNet>,
    mut system: MessageReader<ChatSystemLine>,
    state: Res<PlayerState>,
    mut last: Local<(u32, bool)>,
    mut log: ResMut<ChatLog>,
) {
    for SpellNet(msg) in net_msgs.read() {
        match msg {
            ServerMsg::Chat { from, text } => {
                log.push(vec![(format!("[{from}]: "), NAME), (text.clone(), SAY)]);
            }
            _ => {}
        }
    }
    for ChatSystemLine(text) in system.read() {
        log.system(text.clone(), SYSTEM);
    }
    if state.is_changed() {
        if last.0 > 0 && state.level > last.0 {
            log.system(format!("You have reached level {}!", state.level), SYSTEM);
        }
        if state.dead && !last.1 {
            log.system("You have died.", ERROR);
        }
        *last = (state.level, state.dead);
    }
}

#[allow(clippy::type_complexity)]
fn buttons(
    mut log: ResMut<ChatLog>,
    net: Res<Net>,
    data: Res<GameData>,
    mut buttons: Query<(&Interaction, &ChatButton, &ButtonArt, &mut ImageNode), Changed<Interaction>>,
) {
    for (interaction, button, art, mut image) in &mut buttons {
        image.image = art.0[match interaction {
            Interaction::None => 0,
            Interaction::Hovered => 1,
            Interaction::Pressed => 2,
        }]
        .clone();
        if *interaction != Interaction::Pressed {
            continue;
        }
        match button {
            ChatButton::Top => log.scroll_by(isize::MAX / 2),
            ChatButton::Up => log.scroll_by(1),
            ChatButton::Down => log.scroll_by(-1),
            ChatButton::Bottom => log.scroll_by(-(isize::MAX / 2)),
            ChatButton::Enter => match log.input.take() {
                Some(text) => submit(&text, &mut log, &net, &data),
                None => log.input = Some(String::new()),
            },
        }
        log.dirty = true;
    }
}

fn wheel_scroll(
    mut wheel: MessageReader<MouseWheel>,
    captured: Res<UiInputCaptured>,
    area: Query<&Interaction, With<CapturesPointer>>,
    panel: Query<&ChildOf, With<LogArea>>,
    mut log: ResMut<ChatLog>,
) {
    let over_chat = captured.pointer
        && panel.single().ok().and_then(|c| area.get(c.parent()).ok()).is_some_and(|i| *i != Interaction::None);
    for w in wheel.read() {
        if over_chat {
            let lines = match w.unit {
                MouseScrollUnit::Line => w.y.round() as isize,
                MouseScrollUnit::Pixel => (w.y / 16.0).round() as isize,
            };
            log.scroll_by(lines);
        }
    }
}

fn render_log(mut commands: Commands, log: Res<ChatLog>, font: Res<UiFont>, area: Query<Entity, With<LogArea>>) {
    if !log.dirty {
        return;
    }
    let Ok(area) = area.single() else { return };
    let f = TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() };
    let end = log.lines.len() - log.scroll.min(log.lines.len());
    let start = end.saturating_sub(VISIBLE);
    commands.entity(area).despawn_related::<Children>();
    commands.entity(area).with_children(|a| {
        for line in log.lines.range(start..end) {
            a.spawn((
                Node { width: Val::Percent(100.0), flex_shrink: 0.0, ..default() },
                Text::new(""),
                f.clone(),
                TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
            ))
            .with_children(|t| {
                for (text, color) in line {
                    t.spawn((TextSpan::new(text.clone()), f.clone(), TextColor(*color)));
                }
            });
        }
    });
    // `render_input` also reads `dirty`; it is cleared there.
}

fn render_input(
    mut log: ResMut<ChatLog>,
    time: Res<Time>,
    mut text: Query<(&mut Text, &mut TextColor), With<InputText>>,
) {
    let Ok((mut t, mut c)) = text.single_mut() else { return };
    let caret = log.input.is_some() && (time.elapsed_secs() * 2.0) as i32 % 2 == 0;
    let (s, color) = match &log.input {
        Some(input) => (format!("Say: {input}{}", if caret { "|" } else { "" }), SAY),
        None if log.scroll > 0 => (format!("(scrolled up {} lines)", log.scroll), HINT),
        None => ("Press Enter to chat".to_string(), HINT),
    };
    if t.0 != s {
        t.0 = s;
        c.0 = color;
    }
    log.dirty = false;
}

/// Debug aid (`DUSK_CHAT=text`): says `text` after 2 s, then opens the input with it typed.
fn debug_chat(time: Res<Time>, net: Res<Net>, mut log: ResMut<ChatLog>, mut step: Local<u8>) {
    let text = std::env::var("DUSK_CHAT").unwrap_or_default();
    let t = time.elapsed_secs();
    if *step == 0 && t > 2.0 {
        net.send(ClientMsg::Chat { text: text.clone() });
        *step = 1;
    } else if *step == 1 && t > 3.0 {
        log.input = Some(text);
        log.dirty = true;
        *step = 2;
    }
}

// ---------------------------------------------------------------- speech bubbles

/// Seconds a bubble stays up (plus a little per character).
const BUBBLE_SECS: f32 = 5.0;
const BUBBLE_MAX_W: f32 = 220.0;

#[derive(Component)]
struct Bubble {
    unit: Entity,
    until: f32,
}

fn spawn_bubbles(
    mut commands: Commands,
    mut net_msgs: MessageReader<SpellNet>,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    speakers: Query<(Entity, &Name), (With<Unit>, Without<Npc>)>,
    old: Query<(Entity, &Bubble)>,
) {
    let now = time.elapsed_secs();
    for (e, b) in &old {
        if b.until < now {
            commands.entity(e).despawn();
        }
    }
    for SpellNet(msg) in net_msgs.read() {
        let ServerMsg::Chat { from, text } = msg else { continue };
        let Some((unit, _)) = speakers.iter().find(|(_, n)| n.as_str() == from) else { continue };
        for (e, b) in &old {
            if b.unit == unit {
                commands.entity(e).despawn();
            }
        }
        let piece =
            |n: &str| ImageNode::new(img(&data, &assets, &format!("saybox_{n}.png"))).with_mode(NodeImageMode::Stretch);
        commands
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::Grid,
                    grid_template_columns: vec![GridTrack::px(4.0), GridTrack::auto(), GridTrack::px(4.0)],
                    grid_template_rows: vec![GridTrack::px(4.0), GridTrack::auto(), GridTrack::px(4.0)],
                    max_width: Val::Px(BUBBLE_MAX_W),
                    ..default()
                },
                Visibility::Hidden,
                GlobalZIndex(-1),
                Bubble { unit, until: now + BUBBLE_SECS + text.len() as f32 * 0.04 },
            ))
            .with_children(|g| {
                for name in ["topleft", "topacross", "topright", "leftup"] {
                    g.spawn((Node::default(), piece(name)));
                }
                g.spawn((Node { padding: UiRect::axes(Val::Px(4.0), Val::Px(2.0)), ..default() }, piece("center")))
                    .with_child((
                        Text::new(text.clone()),
                        TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() },
                        TextColor(SAY),
                        TextLayout::justify(Justify::Center),
                    ));
                for name in ["rightup", "bottomleft", "bottomacross", "bottomright"] {
                    g.spawn((Node::default(), piece(name)));
                }
            });
    }
}

/// Keeps bubbles above their speakers' heads.
fn place_bubbles(
    camera: Query<(&Camera, &GlobalTransform), With<crate::player::MainCamera>>,
    units: Query<(&Unit, &GlobalTransform)>,
    mut bubbles: Query<(&Bubble, &mut Node, &mut Visibility, &ComputedNode)>,
) {
    let Ok((cam, cam_tf)) = camera.single() else { return };
    for (b, mut node, mut vis, computed) in &mut bubbles {
        let Ok((u, tf)) = units.get(b.unit) else {
            *vis = Visibility::Hidden;
            continue;
        };
        let head = tf.translation() + Vec3::Y * (u.height * u.scale + 22.0);
        let Ok(vp) = cam.world_to_viewport(cam_tf, head) else { continue };
        let size = computed.size() * computed.inverse_scale_factor();
        node.left = Val::Px((vp.x - size.x / 2.0).round());
        node.top = Val::Px((vp.y - size.y).round());
        // First frame: size unknown until layout ran once.
        vis.set_if_neq(if size.x > 0.0 { Visibility::Inherited } else { Visibility::Hidden });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrolling_is_clamped() {
        let mut log = ChatLog::default();
        for i in 0..30 {
            log.system(format!("line {i}"), SYSTEM);
        }
        log.scroll_by(100);
        assert_eq!(log.scroll, 30 - VISIBLE);
        log.scroll_by(-100);
        assert_eq!(log.scroll, 0);
        // New lines don't move a scrolled-up view.
        log.scroll_by(2);
        log.system("new", SYSTEM);
        assert_eq!(log.scroll, 3);
    }
}
