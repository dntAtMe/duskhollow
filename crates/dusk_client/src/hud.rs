//! Unit frames on the original art (player top-left, target next to it), the XP bar,
//! circular portraits and the `config.ini` HUD options.
//!
//! Frame geometry comes from `UnitFrame::setFrameStyle` (`FUN_0052f460`): style 1 is the
//! player frame (`unit_frame.png`), style 2 the mirrored target frame (`unit_frame_reverse.png`).
//! See `docs/ui.md`.

use crate::{
    combat_ui::UiFont,
    data::GameData,
    net::{Net, PlayerState},
    ui_input::CapturesPointer,
    unit::{Health, Level, Npc},
};
use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use dusk_formats::db::faction;
use std::collections::HashMap;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Portraits>()
            .add_systems(PreStartup, load_config)
            .add_systems(Startup, spawn_hud.after(crate::combat_ui::load_font))
            .add_systems(Update, (update_player_frame, update_target_frame, update_xp_bar, bake_portraits).chain());
    }
}

// ---------------------------------------------------------------- config

/// HUD options from the original `config.ini` (`[System]` / `[UI]`), defaults as shipped.
/// Read from `<assets>/config.ini` when present.
#[derive(Resource, Debug, Clone)]
pub struct HudConfig {
    /// Health bars over hostile/neutral NPCs.
    pub enemy_nameplates: bool,
    /// Health bars over friendly NPCs and other players.
    pub friendly_nameplates: bool,
    /// Our own name over our head.
    pub your_name: bool,
    /// Our own health bar over our head.
    pub your_nameplate: bool,
    pub show_player_names: bool,
    pub show_npc_names: bool,
    /// Minimap zoom level index (0 = closest).
    pub minimap_zoom: usize,
}

impl Default for HudConfig {
    fn default() -> Self {
        Self {
            enemy_nameplates: true,
            friendly_nameplates: false,
            your_name: true,
            your_nameplate: false,
            show_player_names: true,
            show_npc_names: true,
            minimap_zoom: 1,
        }
    }
}

impl HudConfig {
    /// Applies `Key=Value` lines of an ini file over the defaults (sections are ignored).
    pub fn parse(text: &str) -> Self {
        let mut c = Self::default();
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().trim_matches('"');
            let flag = v != "0";
            match k.trim() {
                "EnemyNameplateTick" => c.enemy_nameplates = flag,
                "FriendlyNameplateTick" => c.friendly_nameplates = flag,
                "YourNameTick" => c.your_name = flag,
                "YourNameplateTick" => c.your_nameplate = flag,
                "ShowPlayerNameTick" => c.show_player_names = flag,
                "ShowNpcNameTick" => c.show_npc_names = flag,
                "MinimapZoom" => c.minimap_zoom = v.parse().unwrap_or(c.minimap_zoom),
                _ => {}
            }
        }
        c
    }
}

fn load_config(mut commands: Commands, data: Res<GameData>) {
    let config =
        std::fs::read_to_string(data.root.join("config.ini")).map(|t| HudConfig::parse(&t)).unwrap_or_default();
    commands.insert_resource(config);
}

// ---------------------------------------------------------------- portraits

/// Portrait diameter inside the frame circle.
const PORTRAIT: u32 = 78;

/// Circular portrait thumbnails baked on the CPU from the original portrait cards
/// (210x330, or 80x80 faction placeholders).
#[derive(Resource, Default)]
pub struct Portraits {
    entries: HashMap<String, PortraitEntry>,
    /// `scripts/sprite/portrait_offset.txt`: face height in player portraits.
    offsets: Option<HashMap<String, u32>>,
}

enum PortraitEntry {
    Loading { source: Handle<Image>, centre_y: Option<u32> },
    Ready(Handle<Image>),
    Failed,
}

impl Portraits {
    /// Baked portrait for a bare file name like `portrait_goblin.png`; `None` while loading.
    pub fn get(&mut self, data: &GameData, assets: &AssetServer, name: &str) -> Option<Handle<Image>> {
        let offsets = self.offsets.get_or_insert_with(|| {
            std::fs::read_to_string(data.root.join("scripts/sprite/portrait_offset.txt"))
                .unwrap_or_default()
                .lines()
                .filter_map(|l| l.split_once('='))
                .filter_map(|(k, v)| Some((k.trim().to_lowercase(), v.trim().parse().ok()?)))
                .collect()
        });
        let key = name.to_lowercase();
        let centre_y = offsets.get(&key).copied();
        let entry = self.entries.entry(key).or_insert_with(|| match data.asset_path(name) {
            Some(path) => PortraitEntry::Loading { source: assets.load(path), centre_y },
            None => PortraitEntry::Failed,
        });
        match entry {
            PortraitEntry::Ready(h) => Some(h.clone()),
            _ => None,
        }
    }

    /// Portrait for an NPC: `npc_template.portrait`, else one named after its model, else the
    /// faction placeholder.
    pub fn npc(&mut self, data: &GameData, assets: &AssetServer, entry: i64) -> Option<Handle<Image>> {
        let tpl = data.npc_templates.get(&entry)?;
        let model = data.npc_models.get(&tpl.model_id).map(|m| m.name.as_str()).unwrap_or("");
        // `--art custom`: our generated close-up (tools/artgen/portraits.py) when one exists.
        let custom = data.custom_art.then(|| format!("portrait_custom_{model}.png"));
        let candidates =
            custom.into_iter().chain([format!("portrait_{}.png", tpl.portrait), format!("portrait_{model}.png")]);
        let named = candidates.into_iter().find(|n| n != "portrait_.png" && data.asset_path(n).is_some());
        let name = named.unwrap_or_else(|| {
            match tpl.faction {
                faction::FRIENDLY => "portrait_friendly.png",
                faction::HOSTILE => "portrait_hostile.png",
                _ => "portrait_grey.png",
            }
            .to_string()
        });
        self.get(data, assets, &name)
    }
}

/// The local player's portrait: the default male card, or our adventurer with `--art custom`.
fn player_portrait(data: &GameData) -> &'static str {
    if data.custom_art { "portrait_custom_adventurer.png" } else { "portrait_male (90).png" }
}

fn bake_portraits(mut portraits: ResMut<Portraits>, mut images: ResMut<Assets<Image>>) {
    for entry in portraits.entries.values_mut() {
        let PortraitEntry::Loading { source, centre_y } = entry else { continue };
        let Some(src) = images.get(&*source) else { continue };
        *entry = match bake(src, *centre_y) {
            Some(img) => PortraitEntry::Ready(images.add(img)),
            None => PortraitEntry::Failed,
        };
    }
}

/// Crops a square around the face, scales it to [`PORTRAIT`] and cuts a soft-edged circle.
fn bake(src: &Image, centre_y: Option<u32>) -> Option<Image> {
    let (w, h) = (src.width() as f32, src.height() as f32);
    // Small images (faction placeholders) are used whole; cards are cropped around the face.
    let (side, cx, cy) = if w <= 100.0 {
        (w.min(h), w / 2.0, h / 2.0)
    } else {
        let side = 130.0f32.min(w);
        let cy = centre_y.map(|c| c as f32 + 10.0).unwrap_or(115.0).clamp(side / 2.0, h - side / 2.0);
        (side, w / 2.0, cy)
    };
    let mut out = Image::new_fill(
        Extent3d { width: PORTRAIT, height: PORTRAIT, depth_or_array_layers: 1 },
        TextureDimension::D2,
        &[0, 0, 0, 0],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let n = PORTRAIT as f32;
    let step = side / n;
    for y in 0..PORTRAIT {
        for x in 0..PORTRAIT {
            let d = Vec2::new(x as f32 + 0.5 - n / 2.0, y as f32 + 0.5 - n / 2.0).length();
            let alpha = (n / 2.0 - d).clamp(0.0, 1.0);
            if alpha <= 0.0 {
                continue;
            }
            // 2x2 supersampling of the (larger) source.
            let mut acc = Vec4::ZERO;
            for (ox, oy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
                let sx = (cx - side / 2.0 + (x as f32 + ox) * step).clamp(0.0, w - 1.0) as u32;
                let sy = (cy - side / 2.0 + (y as f32 + oy) * step).clamp(0.0, h - 1.0) as u32;
                acc += src.get_color_at(sx, sy).ok()?.to_linear().to_vec4();
            }
            let c = acc / 4.0;
            let color = LinearRgba::new(c.x, c.y, c.z, c.w * alpha);
            out.set_color_at(x, y, color.into()).ok()?;
        }
    }
    Some(out)
}

// ---------------------------------------------------------------- frames

/// Screen position (top-left) of the player frame.
pub const PLAYER_FRAME: Vec2 = Vec2::new(0.0, 0.0);
/// Screen position (top-left) of the target frame.
pub const TARGET_FRAME: Vec2 = Vec2::new(372.0, 0.0);
const FRAME_SIZE: Vec2 = Vec2::new(372.0, 116.0);
const HP_SIZE: Vec2 = Vec2::new(296.0, 28.0);
const MP_SIZE: Vec2 = Vec2::new(265.0, 22.0);

/// Node for an aura icon row under a unit frame (`UnitFrame` +0x90: 95,108 / 265,108).
pub fn aura_row_node(player: bool) -> Node {
    let mut node = Node {
        position_type: PositionType::Absolute,
        top: Val::Px(PLAYER_FRAME.y + 108.0),
        column_gap: Val::Px(3.0),
        ..default()
    };
    if player {
        node.left = Val::Px(PLAYER_FRAME.x + 95.0);
    } else {
        // Grows leftwards from the portrait side, like the mirrored frame.
        node.left = Val::Px(TARGET_FRAME.x + 20.0);
        node.width = Val::Px(245.0);
        node.flex_direction = FlexDirection::RowReverse;
    }
    node
}

/// Geometry of one frame style.
struct Style {
    frame: &'static str,
    hp: (&'static str, Vec2),
    mp: (&'static str, Vec2),
    /// Portrait circle centre.
    portrait: Vec2,
    /// Level badge centre.
    level: Vec2,
    /// Bars drain towards the portrait side (right) instead of the left.
    reverse: bool,
}

const PLAYER_STYLE: Style = Style {
    frame: "unit_frame.png",
    hp: ("unit_frame_hp.png", Vec2::new(74.0, 39.0)),
    mp: ("unit_frame_mp.png", Vec2::new(84.0, 70.0)),
    portrait: Vec2::new(43.0, 73.0),
    level: Vec2::new(71.0, 105.0),
    reverse: false,
};

const TARGET_STYLE: Style = Style {
    frame: "unit_frame_reverse.png",
    hp: ("unit_frame_hp_reverse.png", Vec2::new(3.0, 39.0)),
    mp: ("unit_frame_mp_reverse.png", Vec2::new(23.0, 70.0)),
    portrait: Vec2::new(328.0, 73.0),
    level: Vec2::new(330.0, 108.0),
    reverse: true,
};

/// Which frame a HUD node belongs to.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Which {
    Player,
    Target,
}

#[derive(Component)]
struct FrameRoot(Which);
#[derive(Component)]
struct BarClip {
    which: Which,
    mana: bool,
}
#[derive(Component)]
struct BarText {
    which: Which,
    mana: bool,
}
#[derive(Component)]
struct NameText(Which);
#[derive(Component)]
struct LevelText(Which);
#[derive(Component)]
struct PortraitImage(Which);
/// Elite/boss dragon ring around the target portrait.
#[derive(Component)]
struct RankRing;
#[derive(Component)]
struct XpClip;
/// Hover box for the XP numbers.
#[derive(Component)]
struct XpText;
#[derive(Component)]
struct XpLabel;
/// The XP bar's hover area.
#[derive(Component)]
struct XpBar;

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

/// Box that centres its (text) child on `centre`.
fn centred_box(centre: Vec2, size: Vec2) -> Node {
    Node { justify_content: JustifyContent::Center, align_items: AlignItems::Center, ..abs(centre - size / 2.0, size) }
}

fn spawn_frame(
    commands: &mut Commands,
    data: &GameData,
    assets: &AssetServer,
    font: &UiFont,
    which: Which,
    style: &Style,
    origin: Vec2,
) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let shadow = TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.9) };
    let mut root = commands.spawn((abs(origin, FRAME_SIZE), FrameRoot(which), CapturesPointer));
    if which == Which::Target {
        root.insert(Visibility::Hidden);
    }
    root.with_children(|p| {
        // Frame art first; the portrait goes on top (the frame circle is opaque black).
        p.spawn((abs(Vec2::ZERO, FRAME_SIZE), ImageNode::new(img(data, assets, style.frame))));
        for (mana, (name, pos), size) in [(false, style.hp, HP_SIZE), (true, style.mp, MP_SIZE)] {
            // Clip container shrinks with the value; the bar image stays put inside it.
            let mut clip = abs(pos, size);
            clip.overflow = Overflow::clip();
            p.spawn((clip, BarClip { which, mana })).with_children(|c| {
                let mut bar = abs(Vec2::ZERO, size);
                if style.reverse {
                    bar.left = Val::Auto;
                    bar.right = Val::Px(0.0);
                }
                c.spawn((bar, ImageNode::new(img(data, assets, name))));
            });
            let text_centre = pos + size / 2.0 + Vec2::new(if style.reverse { -10.0 } else { 10.0 }, 0.0);
            p.spawn(centred_box(text_centre, Vec2::new(200.0, size.y))).with_child((
                Text::new(""),
                f(if mana { 12.0 } else { 14.0 }),
                TextColor(Color::srgb(0.95, 0.92, 0.85)),
                shadow,
                BarText { which, mana },
            ));
        }
        let r = PORTRAIT as f32 / 2.0;
        p.spawn((
            abs(style.portrait - Vec2::splat(r), Vec2::splat(PORTRAIT as f32)),
            ImageNode::default(),
            Visibility::Hidden,
            PortraitImage(which),
        ));
        if which == Which::Target {
            // Ring centre in unit_frame_elite/boss.png is (65.5, 75.5).
            p.spawn((
                abs(style.portrait - Vec2::new(65.5, 75.5), Vec2::new(136.0, 140.0)),
                ImageNode::default(),
                Visibility::Hidden,
                RankRing,
            ));
        }
        p.spawn((
            abs(style.level - Vec2::splat(16.5), Vec2::splat(33.0)),
            ImageNode::new(img(data, assets, "unit_frame_level_bg.png")),
        ))
        .with_children(|b| {
            b.spawn(centred_box(Vec2::splat(16.5), Vec2::splat(33.0))).with_child((
                Text::new(""),
                f(14.0),
                TextColor(Color::srgb(1.0, 0.85, 0.45)),
                shadow,
                LevelText(which),
            ));
        });
        // Name above the health bar, aligned to the side away from the portrait.
        let mut name = abs(Vec2::new(style.hp.1.x + 16.0, 14.0), Vec2::new(HP_SIZE.x - 32.0, 22.0));
        name.justify_content = if style.reverse { JustifyContent::FlexEnd } else { JustifyContent::FlexStart };
        p.spawn(name).with_child((
            Text::new(""),
            f(15.0),
            TextColor(Color::srgb(0.95, 0.85, 0.6)),
            shadow,
            NameText(which),
        ));
    });
}

fn spawn_hud(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>, font: Res<UiFont>) {
    spawn_frame(&mut commands, &data, &assets, &font, Which::Player, &PLAYER_STYLE, PLAYER_FRAME);
    spawn_frame(&mut commands, &data, &assets, &font, Which::Target, &TARGET_STYLE, TARGET_FRAME);

    // XP bar (xp_bar.png 633x12) just above the action bar.
    let xp = data.asset_path("xp_bar.png").map(|p| assets.load(p)).unwrap_or_default();
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: Val::Px(62.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-316.0)),
                width: Val::Px(633.0),
                height: Val::Px(12.0),
                ..default()
            },
            ImageNode::new(xp.clone()).with_color(Color::srgb(0.22, 0.17, 0.22)),
            Interaction::default(),
            XpBar,
        ))
        .with_children(|b| {
            let mut clip = abs(Vec2::ZERO, Vec2::new(0.0, 12.0));
            clip.overflow = Overflow::clip();
            b.spawn((clip, XpClip)).with_children(|c| {
                c.spawn((abs(Vec2::ZERO, Vec2::new(633.0, 12.0)), ImageNode::new(xp)));
            });
            b.spawn((centred_box(Vec2::new(316.5, 6.0), Vec2::new(633.0, 12.0)), Visibility::Hidden, XpText))
                .with_child((
                    Text::new(""),
                    TextFont { font: font.0.clone().into(), font_size: 11.0.into(), ..default() },
                    TextColor(Color::srgb(0.95, 0.9, 0.95)),
                    TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK },
                    XpLabel,
                ));
        });
}

fn pct(a: i32, b: i32) -> f32 {
    (a as f32 / b.max(1) as f32).clamp(0.0, 1.0)
}

/// Resizes a bar's clip container: drains to the left, or to the right for `reverse`.
fn set_bar(node: &mut Node, style: &Style, mana: bool, ratio: f32) {
    let (pos, size) = if mana { (style.mp.1, MP_SIZE) } else { (style.hp.1, HP_SIZE) };
    let w = (size.x * ratio).round();
    node.width = Val::Px(w);
    if style.reverse {
        node.left = Val::Px(pos.x + size.x - w);
    }
}

#[allow(clippy::type_complexity)]
fn update_player_frame(
    data: Res<GameData>,
    assets: Res<AssetServer>,
    state: Res<PlayerState>,
    mut portraits: ResMut<Portraits>,
    mut bars: Query<(&mut Node, &BarClip)>,
    mut texts: ParamSet<(Query<(&mut Text, &BarText)>, Query<(&mut Text, &NameText)>, Query<(&mut Text, &LevelText)>)>,
    mut portrait: Query<(&mut ImageNode, &mut Visibility, &PortraitImage)>,
) {
    // The portrait may finish baking after the last state change.
    for (mut node, mut vis, p) in &mut portrait {
        if p.0 == Which::Player && *vis == Visibility::Hidden {
            if let Some(h) = portraits.get(&data, &assets, player_portrait(&data)) {
                node.image = h;
                *vis = Visibility::Inherited;
            }
        }
    }
    if !state.is_changed() {
        return;
    }
    for (mut node, bar) in &mut bars {
        if bar.which == Which::Player {
            let ratio = if bar.mana { pct(state.mana, state.max_mana) } else { pct(state.hp.max(0), state.max_hp) };
            set_bar(&mut node, &PLAYER_STYLE, bar.mana, ratio);
        }
    }
    for (mut t, b) in &mut texts.p0() {
        if b.which == Which::Player {
            t.0 = if b.mana {
                format!("{} / {}", state.mana, state.max_mana)
            } else {
                format!("{} / {}", state.hp.max(0), state.max_hp)
            };
        }
    }
    for (mut t, n) in &mut texts.p1() {
        if n.0 == Which::Player {
            t.0.clone_from(&state.name);
        }
    }
    for (mut t, l) in &mut texts.p2() {
        if l.0 == Which::Player {
            t.0 = state.level.to_string();
        }
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn update_target_frame(
    data: Res<GameData>,
    assets: Res<AssetServer>,
    state: Res<PlayerState>,
    net: Res<Net>,
    mut portraits: ResMut<Portraits>,
    targets: Query<(Option<&Npc>, &Health, Option<&Level>, Option<&Name>)>,
    mut frame: Query<(&mut Visibility, &FrameRoot)>,
    mut bars: Query<(&mut Node, &BarClip)>,
    mut texts: ParamSet<(
        Query<(&mut Text, &BarText)>,
        Query<(&mut Text, &mut TextColor, &NameText)>,
        Query<(&mut Text, &LevelText)>,
    )>,
    mut images: ParamSet<(
        Query<(&mut ImageNode, &mut Visibility, &PortraitImage), Without<FrameRoot>>,
        Query<(&mut ImageNode, &mut Visibility), (With<RankRing>, Without<FrameRoot>)>,
    )>,
) {
    let target = state.target.and_then(|id| net.entities.get(&id)).and_then(|e| targets.get(*e).ok());
    for (mut vis, root) in &mut frame {
        if root.0 == Which::Target {
            vis.set_if_neq(if target.is_some() { Visibility::Inherited } else { Visibility::Hidden });
        }
    }
    let Some((npc, health, level, name)) = target else { return };
    let tpl = npc.and_then(|n| data.npc_templates.get(&n.entry));

    let has_mana = tpl.is_some_and(|t| t.mana > 0 || t.ai_type == 1);
    for (mut node, bar) in &mut bars {
        if bar.which == Which::Target {
            let ratio = if bar.mana { if has_mana { 1.0 } else { 0.0 } } else { pct(health.hp.max(0), health.max) };
            set_bar(&mut node, &TARGET_STYLE, bar.mana, ratio);
        }
    }
    for (mut t, b) in &mut texts.p0() {
        if b.which == Which::Target {
            let s = if b.mana { String::new() } else { format!("{} / {}", health.hp.max(0), health.max) };
            if t.0 != s {
                t.0 = s;
            }
        }
    }
    let (label, color) = match tpl {
        Some(t) => (t.name.clone(), faction_color(t.faction)),
        None => (name.map(|n| n.to_string()).unwrap_or_default(), FRIENDLY_GREEN),
    };
    for (mut t, mut c, n) in &mut texts.p1() {
        if n.0 == Which::Target && t.0 != label {
            t.0.clone_from(&label);
            c.0 = color;
        }
    }
    let lvl = level.map(|l| l.0.to_string()).unwrap_or_default();
    for (mut t, l) in &mut texts.p2() {
        if l.0 == Which::Target && t.0 != lvl {
            t.0.clone_from(&lvl);
        }
    }

    let portrait = npc.and_then(|n| portraits.npc(&data, &assets, n.entry));
    for (mut node, mut vis, p) in &mut images.p0() {
        if p.0 == Which::Target {
            match &portrait {
                Some(h) => {
                    if node.image != *h {
                        node.image = h.clone();
                    }
                    vis.set_if_neq(Visibility::Inherited);
                }
                None => {
                    vis.set_if_neq(Visibility::Hidden);
                }
            }
        }
    }
    let ring = tpl.and_then(|t| {
        if t.boss {
            Some("unit_frame_boss.png")
        } else if t.elite {
            Some("unit_frame_elite.png")
        } else {
            None
        }
    });
    if let Ok((mut node, mut vis)) = images.p1().single_mut() {
        match ring {
            Some(r) => {
                let h = img(&data, &assets, r);
                if node.image != h {
                    node.image = h;
                }
                vis.set_if_neq(Visibility::Inherited);
            }
            None => {
                vis.set_if_neq(Visibility::Hidden);
            }
        }
    }
}

pub const HOSTILE_RED: Color = Color::srgb(0.95, 0.25, 0.2);
pub const NEUTRAL_YELLOW: Color = Color::srgb(0.95, 0.85, 0.25);
pub const FRIENDLY_GREEN: Color = Color::srgb(0.35, 0.9, 0.35);

/// Name colour for an `npc_template.faction`.
pub fn faction_color(f: i64) -> Color {
    match f {
        faction::HOSTILE => HOSTILE_RED,
        faction::NEUTRAL => NEUTRAL_YELLOW,
        _ => FRIENDLY_GREEN,
    }
}

fn update_xp_bar(
    state: Res<PlayerState>,
    bar: Query<&Interaction, (Changed<Interaction>, With<XpBar>)>,
    mut clip: Query<&mut Node, With<XpClip>>,
    mut hover: Query<&mut Visibility, With<XpText>>,
    mut text: Query<&mut Text, With<XpLabel>>,
) {
    if let Ok(mut vis) = hover.single_mut() {
        if let Some(i) = bar.iter().next() {
            *vis = if *i == Interaction::None { Visibility::Hidden } else { Visibility::Inherited };
        }
    }
    if !state.is_changed() {
        return;
    }
    if let Ok(mut n) = clip.single_mut() {
        n.width = Val::Px((633.0 * pct(state.xp as i32, state.xp_next as i32)).round());
    }
    if let Ok(mut t) = text.single_mut() {
        t.0 = format!("Experience {} / {}", state.xp, state.xp_next);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_overrides_defaults() {
        let c = HudConfig::parse("[System]\nEnemyNameplateTick=0\nShowNpcNameTick=1\n[UI]\nMinimapZoom=3\n");
        assert!(!c.enemy_nameplates);
        assert!(c.show_npc_names);
        assert!(!c.friendly_nameplates);
        assert_eq!(c.minimap_zoom, 3);
    }
}
