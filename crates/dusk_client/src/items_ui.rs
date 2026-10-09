//! Items UI: Inventory (`I`, `inventory.png`), Character / equipment (`C`, `equipment.png`),
//! corpse loot window (`loot_window.png`), item tooltips and a loot/error log.
//!
//! Clicks: bag item -> equip (gear) or use (potions); right click does the same; shift +
//! right click destroys; equipment slot -> unequip; left click on a corpse marked with a
//! pouch opens its loot window.

use crate::{
    chat::ChatSystemLine,
    combat_ui::UiFont,
    data::GameData,
    iso,
    minimap::overlay_layer,
    net::{Net, PlayerState},
    paper_doll::Appearances,
    player::{MainCamera, Player},
    spells_ui::{Spellbook, describe},
    ui_input::{CapturesPointer, UiInputCaptured},
    unit::{Dead, Unit},
    windows::{self, Hint, HoverTint, UiWindow, WindowCommand, WindowId, Windows},
};
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use dusk_formats::{
    db::GameDb,
    item::{self, Affix, BAG_SLOTS, EQUIP_SLOTS, ItemTemplate, equip, quality, slot, stat},
};
use dusk_protocol::{ClientMsg, CombatStats, EntityId, Item, ServerMsg};
use std::collections::{HashMap, HashSet};

pub struct ItemsUiPlugin;

impl Plugin for ItemsUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ItemsState>()
            .init_resource::<CharTab>()
            .add_message::<ItemNet>()
            .add_systems(Startup, (load_item_db, spawn_windows.after(crate::combat_ui::load_font)))
            .add_systems(
                Update,
                (
                    handle_item_net,
                    (char_buttons, slot_clicks, loot_clicks, world_loot_click, sync_loot_closed).chain(),
                    (refresh_slots, refresh_character, refresh_tabs, refresh_loot_window, update_tooltip, update_log),
                    (close_far_loot, loot_markers),
                )
                    .chain(),
            )
            .add_systems(Update, auto_loot.run_if(|| std::env::var_os("DUSK_AUTOPLAY").is_some()))
            .add_systems(Update, open_windows_once.run_if(|| std::env::var_os("DUSK_OPEN_INVENTORY").is_some()))
            .add_systems(Update, debug_char_tab.run_if(|| std::env::var_os("DUSK_CHAR_TAB").is_some()));
    }
}

/// Item-related server messages, forwarded by `net::receive`.
#[derive(Message, Clone)]
pub struct ItemNet(pub ServerMsg);

/// Item templates + affixes (the same tables the server uses for stats).
#[derive(Resource)]
pub struct ItemDb {
    pub items: HashMap<i64, ItemTemplate>,
    pub affixes: HashMap<i64, Affix>,
}

impl ItemDb {
    fn template(&self, it: &Item) -> Option<&ItemTemplate> {
        self.items.get(&(it.entry as i64))
    }

    fn affix(&self, it: &Item) -> Option<&Affix> {
        self.affixes.get(&(it.affix as i64))
    }

    pub fn name(&self, it: &Item) -> String {
        self.template(it).map(|t| item::display_name(t, self.affix(it))).unwrap_or_else(|| format!("#{}", it.entry))
    }
}

/// Mirror of the server-side inventory plus loot state.
#[derive(Resource)]
pub struct ItemsState {
    pub bag: Vec<Option<Item>>,
    pub equipment: Vec<Option<Item>>,
    pub gold: u32,
    pub combat: CombatStats,
    /// Open loot window: (corpse, gold, items).
    loot: Option<(EntityId, u32, Vec<Item>)>,
    /// Corpses we may loot.
    lootable: HashSet<EntityId>,
    /// (text, color, shown_at) lines of the loot log.
    log: Vec<(String, Color, f32)>,
}

impl ItemsState {
    /// The server marked this corpse lootable for us.
    pub fn is_lootable(&self, id: EntityId) -> bool {
        self.lootable.contains(&id)
    }
}

impl Default for ItemsState {
    fn default() -> Self {
        Self {
            bag: vec![None; BAG_SLOTS],
            equipment: vec![None; EQUIP_SLOTS],
            gold: 0,
            combat: default(),
            loot: None,
            lootable: default(),
            log: default(),
        }
    }
}

/// Mirrors the client-side check before asking the server (it validates again).
const LOOT_RANGE: f32 = 3.0;
const GOLD: Color = Color::srgb(0.95, 0.82, 0.45);
const GREY: Color = Color::srgb(0.75, 0.72, 0.66);
const RED: Color = Color::srgb(1.0, 0.3, 0.25);
const BONE: Color = Color::srgb(0.88, 0.83, 0.72);

/// Tooltip/name colour per `item_template.quality`.
pub fn quality_color(q: i64) -> Color {
    match q {
        quality::JUNK => Color::srgb(0.62, 0.62, 0.62),
        quality::GREEN => Color::srgb(0.12, 1.0, 0.0),
        quality::BLUE => Color::srgb(0.25, 0.55, 1.0),
        quality::GOLD => Color::srgb(1.0, 0.75, 0.1),
        quality::PURPLE => Color::srgb(0.72, 0.35, 1.0),
        _ => Color::WHITE,
    }
}

// ---------------------------------------------------------------- layout (measured from the art)

/// `inventory.png` (364x436): 7x7 wells, inner 37px at (28 + 45c, 76 + 45r).
const BAG_X0: f32 = 28.0;
const BAG_Y0: f32 = 76.0;
const BAG_STRIDE: f32 = 45.0;
const BAG_SIZE: f32 = 37.0;
const BAG_COLS: usize = 7;

/// `equipment.png` (603x571): two columns of 41px frames at x 46 / 288, tops below.
const EQUIP_TOPS: [f32; 6] = [179.0, 238.0, 296.0, 354.0, 412.0, 471.0];
const EQUIP_SIZE: f32 = 39.0;

fn equip_slot_pos(s: usize) -> Vec2 {
    let (col, row) = if s < 6 { (46.0, s) } else { (288.0, s - 6) };
    Vec2::new(col + 1.0, EQUIP_TOPS[row] + 1.0)
}

// ---------------------------------------------------------------- markers

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum SlotRef {
    Bag(usize),
    Equip(usize),
    Loot(usize),
}

#[derive(Component)]
struct ItemSlot(SlotRef);
#[derive(Component)]
struct SlotIcon(SlotRef);
#[derive(Component)]
struct SlotCount(SlotRef);
#[derive(Component)]
struct LootList;
#[derive(Component)]
struct LootTakeAll;
#[derive(Component)]
struct CharInvButton;
#[derive(Component)]
struct GoldText;
#[derive(Component)]
struct ProgressText;
#[derive(Component)]
struct ItemTooltip;
#[derive(Component)]
struct LogPanel;
/// Pouch floating over a lootable corpse.
#[derive(Component)]
struct LootMarker(Entity);

/// Character window tabs (baked into `equipment.png`).
#[derive(Resource, Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum CharTab {
    #[default]
    General,
    Combat,
    Skills,
}

/// (tab, label centre x) on `equipment.png`'s tab strip (y 67..108).
const CHAR_TABS: [(CharTab, f32); 3] = [(CharTab::General, 157.0), (CharTab::Combat, 296.0), (CharTab::Skills, 431.0)];

#[derive(Component)]
struct CharTabButton(CharTab);
/// Darkens an inactive tab's label.
#[derive(Component)]
struct CharTabShade(CharTab);
/// Right-hand column contents of a tab.
#[derive(Component)]
struct CharPage(CharTab);
/// A row of the Skills page (opens the Abilities window).
#[derive(Component)]
struct SkillRow;

fn img(data: &GameData, assets: &AssetServer, name: &str) -> Handle<Image> {
    data.asset_path(name).map(|p| assets.load(p)).unwrap_or_default()
}

fn absolute(left: f32, top: f32, w: f32, h: f32) -> Node {
    Node {
        position_type: PositionType::Absolute,
        left: Val::Px(left),
        top: Val::Px(top),
        width: Val::Px(w),
        height: Val::Px(h),
        ..default()
    }
}

fn load_item_db(mut commands: Commands, data: Res<GameData>) {
    let db = GameDb::open(data.root.join("game.db")).expect("game.db");
    commands.insert_resource(ItemDb {
        items: db.items().expect("item_template"),
        affixes: db.affixes().expect("affix_template"),
    });
}

fn spawn_slot(p: &mut ChildSpawnerCommands, r: SlotRef, pos: Vec2, size: f32, font: &TextFont) {
    p.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(pos.x),
            top: Val::Px(pos.y),
            width: Val::Px(size),
            height: Val::Px(size),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(Color::NONE),
        Button,
        CapturesPointer,
        ItemSlot(r),
    ))
    .with_children(|s| {
        s.spawn((absolute(0.0, 0.0, size - 2.0, size - 2.0), ImageNode::default(), Visibility::Hidden, SlotIcon(r)));
        s.spawn((
            Node { position_type: PositionType::Absolute, right: Val::Px(2.0), bottom: Val::Px(0.0), ..default() },
            Text::new(""),
            font.clone(),
            TextColor(Color::WHITE),
            TextShadow::default(),
            SlotCount(r),
        ));
    });
}

fn spawn_windows(mut commands: Commands, data: Res<GameData>, assets: Res<AssetServer>, font: Res<UiFont>) {
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };

    // Inventory (placed by the window manager; right side by default).
    commands
        .spawn((
            absolute(0.0, 0.0, 364.0, 436.0),
            ImageNode::new(img(&data, &assets, "inventory.png")),
            UiWindow(WindowId::Inventory),
        ))
        .with_children(|w| {
            // The art is translucent; a second copy underneath keeps text readable over the world/chat.
            w.spawn((absolute(0.0, 0.0, 364.0, 436.0), ImageNode::new(img(&data, &assets, "inventory.png"))));
            windows::spawn_drag_handle(w, WindowId::Inventory, 14.0, 14.0, 336.0, 44.0);
            windows::spawn_close_button(w, &data, &assets, WindowId::Inventory, 316.0, 23.0, false);
            for i in 0..BAG_SLOTS {
                let (c, r) = ((i % BAG_COLS) as f32, (i / BAG_COLS) as f32);
                let pos = Vec2::new(BAG_X0 + BAG_STRIDE * c, BAG_Y0 + BAG_STRIDE * r);
                spawn_slot(w, SlotRef::Bag(i), pos, BAG_SIZE, &f(12.0));
            }
            w.spawn((
                Node { position_type: PositionType::Absolute, left: Val::Px(52.0), top: Val::Px(400.0), ..default() },
                Text::new("0 Gold Pieces"),
                f(14.0),
                TextColor(GOLD),
                GoldText,
            ));
            w.spawn((
                Node { position_type: PositionType::Absolute, right: Val::Px(22.0), top: Val::Px(403.0), ..default() },
                Text::new("Click: equip / use   Shift+Right: destroy"),
                f(10.0),
                TextColor(GREY.with_alpha(0.7)),
            ));
        });

    // Character window (equipment + stats), left side by default.
    commands
        .spawn((
            absolute(0.0, 0.0, 603.0, 571.0),
            ImageNode::new(img(&data, &assets, "equipment.png")),
            UiWindow(WindowId::Character),
        ))
        .with_children(|w| {
            // The art is translucent; a second copy underneath keeps text readable over the world/chat.
            w.spawn((absolute(0.0, 0.0, 603.0, 571.0), ImageNode::new(img(&data, &assets, "equipment.png"))));
            windows::spawn_drag_handle(w, WindowId::Character, 14.0, 16.0, 530.0, 50.0);
            // Over the baked-in close box (552, 30).
            windows::spawn_close_button(w, &data, &assets, WindowId::Character, 552.0, 30.0, false);
            for (tab, cx) in CHAR_TABS {
                w.spawn((absolute(cx - 62.0, 69.0, 124.0, 37.0), BackgroundColor(Color::NONE), CharTabShade(tab)));
                let (title, body) = match tab {
                    CharTab::General => ("General", "Equipment, health and attributes"),
                    CharTab::Combat => ("Combat", "Weapon, defence and resistances"),
                    CharTab::Skills => ("Skills", "Your spells and actions"),
                };
                w.spawn((
                    absolute(cx - 62.0, 69.0, 124.0, 37.0),
                    HoverTint,
                    CapturesPointer,
                    CharTabButton(tab),
                    Hint::new(title).with_body(body),
                ));
            }
            for s in 0..EQUIP_SLOTS {
                spawn_slot(w, SlotRef::Equip(s), equip_slot_pos(s), EQUIP_SIZE, &f(12.0));
            }
            w.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(110.0),
                    top: Val::Px(250.0),
                    width: Val::Px(160.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    ..default()
                },
                Text::new(""),
                f(14.0),
                TextColor(BONE),
                TextLayout::justify(Justify::Center),
                ProgressText,
            ));
            for (tab, _) in CHAR_TABS {
                w.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(378.0),
                        top: Val::Px(122.0),
                        width: Val::Px(206.0),
                        height: Val::Px(432.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Visibility::Hidden,
                    CharPage(tab),
                ));
            }
            w.spawn((
                absolute(22.0, 532.0, 120.0, 30.0),
                HoverTint,
                CapturesPointer,
                CharInvButton,
                Hint::new("Inventory (I)"),
            ));
        });

    // Loot window, centre-left by default.
    commands
        .spawn((
            absolute(0.0, 0.0, 214.0, 263.0),
            ImageNode::new(img(&data, &assets, "loot_window.png")),
            UiWindow(WindowId::Loot),
        ))
        .with_children(|w| {
            // The art is translucent; a second copy underneath keeps text readable over the world/chat.
            w.spawn((absolute(0.0, 0.0, 214.0, 263.0), ImageNode::new(img(&data, &assets, "loot_window.png"))));
            windows::spawn_drag_handle(w, WindowId::Loot, 8.0, 6.0, 170.0, 26.0);
            w.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(14.0),
                    top: Val::Px(44.0),
                    width: Val::Px(186.0),
                    height: Val::Px(168.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    overflow: Overflow::clip(),
                    ..default()
                },
                LootList,
            ));
            w.spawn((
                absolute(50.0, 222.0, 116.0, 30.0),
                HoverTint,
                CapturesPointer,
                LootTakeAll,
                Hint::new("Take All").with_body("Or click an item to take just that"),
            ));
            windows::spawn_close_button(w, &data, &assets, WindowId::Loot, 181.0, 8.0, true);
        });

    // Item tooltip.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(8.0)),
            width: Val::Px(260.0),
            border: UiRect::all(Val::Px(1.0)),
            ..default()
        },
        BackgroundColor(Color::srgba(0.06, 0.05, 0.04, 0.95)),
        BorderColor::all(Color::srgb(0.45, 0.38, 0.25)),
        GlobalZIndex(120),
        Visibility::Hidden,
        ItemTooltip,
    ));

    // Loot / error log above the micro-menu, right.
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            right: Val::Px(20.0),
            bottom: Val::Px(90.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            ..default()
        },
        LogPanel,
    ));
}

// ---------------------------------------------------------------- server events

fn handle_item_net(
    mut events: MessageReader<ItemNet>,
    mut state: ResMut<ItemsState>,
    mut appearances: ResMut<Appearances>,
    db: Res<ItemDb>,
    time: Res<Time>,
    mut chat: MessageWriter<ChatSystemLine>,
) {
    let now = time.elapsed_secs();
    for ItemNet(msg) in events.read() {
        match msg {
            ServerMsg::Inventory { bag, equipment, gold } => {
                state.bag = bag.clone();
                state.equipment = equipment.clone();
                state.gold = *gold;
            }
            ServerMsg::Appearance { id, gear } => {
                appearances.0.insert(*id, gear.clone());
            }
            ServerMsg::CombatStats(c) => state.combat = *c,
            ServerMsg::Lootable { id, lootable } => {
                if *lootable {
                    state.lootable.insert(*id);
                } else {
                    state.lootable.remove(id);
                    if state.loot.as_ref().is_some_and(|l| l.0 == *id) {
                        state.loot = None;
                    }
                }
            }
            ServerMsg::LootWindow { corpse, gold, items } => {
                state.loot = (*gold > 0 || !items.is_empty()).then(|| (*corpse, *gold, items.clone()));
            }
            ServerMsg::ItemError { reason } => state.log.push((reason.clone(), RED, now)),
            ServerMsg::Received { item, gold } => {
                if *gold > 0 {
                    state.log.push((format!("You receive {gold} Gold"), GOLD, now));
                    chat.write(ChatSystemLine(format!("You receive {gold} Gold.")));
                }
                if let Some(it) = item {
                    let color = db.template(it).map_or(Color::WHITE, |t| quality_color(t.quality));
                    let count = if it.count > 1 { format!(" x{}", it.count) } else { String::new() };
                    state.log.push((format!("You receive loot: {}{count}", db.name(it)), color, now));
                    chat.write(ChatSystemLine(format!("You receive loot: [{}]{count}.", db.name(it))));
                }
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------- input

/// Character window tabs, its Inventory button and the Skills page rows.
fn char_buttons(
    mut tab: ResMut<CharTab>,
    tabs: Query<(&Interaction, &CharTabButton), Changed<Interaction>>,
    inv: Query<&Interaction, (Changed<Interaction>, With<CharInvButton>)>,
    skill_rows: Query<&Interaction, (Changed<Interaction>, With<SkillRow>)>,
    mut out: MessageWriter<WindowCommand>,
) {
    for (i, t) in &tabs {
        if *i == Interaction::Pressed && *tab != t.0 {
            *tab = t.0;
        }
    }
    if skill_rows.iter().any(|i| *i == Interaction::Pressed) {
        out.write(WindowCommand::Open(WindowId::Abilities));
    }
    if inv.iter().any(|i| *i == Interaction::Pressed) {
        out.write(WindowCommand::Toggle(WindowId::Inventory));
    }
}

fn slot_clicks(
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
    net: Res<Net>,
    state: Res<ItemsState>,
    db: Res<ItemDb>,
    slots: Query<(&Interaction, &ItemSlot)>,
) {
    let left = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    let Some(r) = slots.iter().find(|(i, _)| **i != Interaction::None).map(|(_, s)| s.0) else { return };
    match r {
        SlotRef::Bag(i) => {
            let Some(it) = state.bag[i] else { return };
            let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
            if right && shift {
                net.send(ClientMsg::DestroyItem { bag_slot: i as u8 });
            } else if let Some(t) = db.template(&it) {
                if t.is_equippable() {
                    net.send(ClientMsg::EquipItem { bag_slot: i as u8 });
                } else if !t.spells.is_empty() {
                    net.send(ClientMsg::UseItem { bag_slot: i as u8 });
                }
            }
        }
        SlotRef::Equip(s) => {
            if state.equipment[s].is_some() {
                net.send(ClientMsg::UnequipItem { slot: s as u8 });
            }
        }
        SlotRef::Loot(i) => {
            if let Some((corpse, ..)) = state.loot {
                net.send(ClientMsg::TakeLoot { corpse, index: Some(i as u8) });
            }
        }
    }
}

fn loot_clicks(
    net: Res<Net>,
    state: Res<ItemsState>,
    buttons: Query<&Interaction, (Changed<Interaction>, Or<(With<LootTakeAll>, With<LootGoldRow>)>)>,
) {
    let Some((corpse, ..)) = state.loot else { return };
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        net.send(ClientMsg::TakeLoot { corpse, index: None });
    }
}

/// The loot window was closed by the window manager (close box, `Esc`): forget the loot.
fn sync_loot_closed(windows: Res<Windows>, mut state: ResMut<ItemsState>, mut was_open: Local<bool>) {
    if windows.is_open(WindowId::Loot) {
        *was_open = true;
    } else if std::mem::take(&mut *was_open) && state.loot.is_some() {
        state.loot = None;
    }
}

/// Unit under the cursor (rough body box, front-most), as in `combat_ui::click_target`.
fn unit_under_cursor<'a>(
    world: Vec2,
    units: impl Iterator<Item = (Entity, &'a Unit, &'a Transform)>,
) -> Option<(Entity, &'a Unit)> {
    units
        .filter(|(_, u, _)| {
            let feet = iso::to_screen(u.pos);
            let half_w = 26.0 * u.scale;
            (world.x - feet.x).abs() <= half_w && world.y >= feet.y - 16.0 && world.y <= feet.y + 40.0 * u.scale
        })
        .max_by(|a, b| a.2.translation.z.total_cmp(&b.2.translation.z))
        .map(|(e, u, _)| (e, u))
}

#[allow(clippy::too_many_arguments)]
fn world_loot_click(
    mouse: Res<ButtonInput<MouseButton>>,
    window: Query<&Window, With<PrimaryWindow>>,
    camera: Query<(&Camera, &GlobalTransform), With<MainCamera>>,
    net: Res<Net>,
    mut state: ResMut<ItemsState>,
    time: Res<Time>,
    captured: Res<UiInputCaptured>,
    player: Query<&Unit, With<Player>>,
    corpses: Query<(Entity, &Unit, &Transform), With<Dead>>,
) {
    if !mouse.just_pressed(MouseButton::Left) || captured.pointer {
        return;
    }
    let (Ok(window), Ok((cam, cam_tf)), Ok(me)) = (window.single(), camera.single(), player.single()) else { return };
    let Some(world) = window.cursor_position().and_then(|c| cam.viewport_to_world_2d(cam_tf, c).ok()) else { return };
    let lootable = corpses.iter().filter(|(e, ..)| net.entity_id(*e).is_some_and(|id| state.lootable.contains(&id)));
    let Some((e, u)) = unit_under_cursor(world, lootable) else { return };
    let Some(id) = net.entity_id(e) else { return };
    if u.pos.distance(me.pos) > LOOT_RANGE {
        state.log.push(("Too far away".into(), RED, time.elapsed_secs()));
    } else {
        net.send(ClientMsg::OpenLoot { corpse: id });
    }
}

/// Debug aid (`DUSK_AUTOPLAY=1`): take everything from lootable corpses in range.
fn auto_loot(
    net: Res<Net>,
    state: Res<ItemsState>,
    mut asked: Local<HashSet<EntityId>>,
    player: Query<&Unit, With<Player>>,
    units: Query<&Unit>,
) {
    let Ok(me) = player.single() else { return };
    for id in &state.lootable {
        let near = net.entities.get(id).and_then(|e| units.get(*e).ok()).is_some_and(|u| u.pos.distance(me.pos) < 2.5);
        if near && asked.insert(*id) {
            net.send(ClientMsg::OpenLoot { corpse: *id });
            // `DUSK_LOOT_WINDOW=1`: leave the window open instead (for screenshots).
            if std::env::var_os("DUSK_LOOT_WINDOW").is_none() {
                net.send(ClientMsg::TakeLoot { corpse: *id, index: None });
            }
        }
    }
}

fn open_windows_once(mut done: Local<bool>, mut out: MessageWriter<WindowCommand>) {
    if !*done {
        *done = true;
        out.write(WindowCommand::Open(WindowId::Character));
        out.write(WindowCommand::Open(WindowId::Inventory));
    }
}

/// Debug aid: `DUSK_CHAR_TAB=general|combat|skills` picks the Character window tab.
fn debug_char_tab(mut tab: ResMut<CharTab>, mut done: Local<bool>) {
    if !std::mem::replace(&mut *done, true) {
        *tab = match std::env::var("DUSK_CHAR_TAB").unwrap_or_default().as_str() {
            "combat" => CharTab::Combat,
            "skills" => CharTab::Skills,
            _ => CharTab::General,
        };
    }
}

/// Closes the loot window when walking away or the corpse vanishes.
fn close_far_loot(
    mut state: ResMut<ItemsState>,
    net: Res<Net>,
    player: Query<&Unit, With<Player>>,
    units: Query<&Unit>,
) {
    let Some((corpse, ..)) = state.loot else { return };
    let Ok(me) = player.single() else { return };
    if std::env::var_os("DUSK_LOOT_WINDOW").is_some() {
        return; // debug: keep it up while autoplay walks on
    }
    let corpse_pos = net.entities.get(&corpse).and_then(|e| units.get(*e).ok()).map(|u| u.pos);
    if corpse_pos.is_none_or(|p| p.distance(me.pos) > LOOT_RANGE + 1.0) {
        state.loot = None;
    }
}

// ---------------------------------------------------------------- refresh

fn slot_item(state: &ItemsState, r: SlotRef) -> Option<Item> {
    match r {
        SlotRef::Bag(i) => state.bag.get(i).copied().flatten(),
        SlotRef::Equip(i) => state.equipment.get(i).copied().flatten(),
        SlotRef::Loot(i) => state.loot.as_ref().and_then(|l| l.2.get(i).copied()),
    }
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_slots(
    state: Res<ItemsState>,
    db: Res<ItemDb>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut icons: Query<(&SlotIcon, &mut ImageNode, &mut Visibility)>,
    mut counts: Query<(&SlotCount, &mut Text)>,
    mut frames: Query<(&ItemSlot, &Interaction, &mut BorderColor)>,
    mut gold: Query<&mut Text, (With<GoldText>, Without<SlotCount>)>,
) {
    for (s, interaction, mut border) in &mut frames {
        let q = slot_item(&state, s.0).and_then(|it| db.template(&it)).map(|t| t.quality);
        *border = BorderColor::all(match (interaction, q) {
            (Interaction::None, Some(q)) if q >= quality::GREEN => quality_color(q).with_alpha(0.7),
            (Interaction::None, _) => Color::NONE,
            _ => GOLD,
        });
    }
    if !state.is_changed() {
        return;
    }
    for (s, mut image, mut vis) in &mut icons {
        match slot_item(&state, s.0).and_then(|it| db.template(&it)) {
            Some(t) => {
                image.image = img(&data, &assets, &t.icon);
                *vis = Visibility::Inherited;
            }
            None => *vis = Visibility::Hidden,
        }
    }
    for (s, mut text) in &mut counts {
        text.0 = slot_item(&state, s.0).filter(|it| it.count > 1).map(|it| it.count.to_string()).unwrap_or_default();
    }
    if let Ok(mut t) = gold.single_mut() {
        t.0 = format!("{} Gold Pieces", state.gold);
    }
}

fn refresh_character(player: Res<PlayerState>, mut progress: Query<&mut Text, With<ProgressText>>) {
    if !player.is_changed() {
        return;
    }
    if let Ok(mut t) = progress.single_mut() {
        t.0 = format!("Level {}\n\n\n\n{} / {}", player.level, player.xp, player.xp_next);
    }
}

/// One `label ... value` line of a Character page.
fn stat_row(p: &mut ChildSpawnerCommands, f: &dyn Fn(f32) -> TextFont, label: &str, value: String, hint: &str) {
    let mut row = p.spawn((
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(19.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(Val::Px(4.0)),
            ..default()
        },
        Interaction::default(),
    ));
    if !hint.is_empty() {
        row.insert(Hint::new(label).with_body(hint));
    }
    row.with_children(|r| {
        r.spawn((Text::new(label), f(13.0), TextColor(GREY)));
        r.spawn((Text::new(value), f(13.0), TextColor(BONE)));
    });
}

fn page_header(p: &mut ChildSpawnerCommands, f: &dyn Fn(f32) -> TextFont, text: &str, first: bool) {
    p.spawn((
        Node {
            margin: UiRect::new(Val::Px(0.0), Val::Px(0.0), Val::Px(if first { 0.0 } else { 9.0 }), Val::Px(3.0)),
            padding: UiRect::new(Val::Px(4.0), Val::Px(4.0), Val::Px(0.0), Val::Px(2.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BorderColor::all(Color::srgb(0.45, 0.34, 0.18)),
    ))
    .with_child((Text::new(text), f(14.0), TextColor(GOLD)));
}

/// Tab highlight + the right-hand page of the Character window.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn refresh_tabs(
    mut commands: Commands,
    tab: Res<CharTab>,
    state: Res<ItemsState>,
    player: Res<PlayerState>,
    book: Res<Spellbook>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    mut shades: Query<(&CharTabShade, &mut BackgroundColor, &mut Node, &mut BorderColor)>,
    mut pages: Query<(Entity, &CharPage, &mut Visibility)>,
) {
    if !(tab.is_changed() || state.is_changed() || player.is_changed() || book.is_changed()) {
        return;
    }
    for (s, mut bg, mut node, mut border) in &mut shades {
        let active = s.0 == *tab;
        bg.0 = if active { Color::srgba(0.55, 0.12, 0.08, 0.22) } else { Color::srgba(0.0, 0.0, 0.0, 0.45) };
        node.border = UiRect::bottom(Val::Px(2.0));
        *border = BorderColor::all(if active { GOLD.with_alpha(0.85) } else { Color::NONE });
    }
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let a = &player.attributes;
    let c = &state.combat;
    for (e, page, mut vis) in &mut pages {
        vis.set_if_neq(if page.0 == *tab { Visibility::Inherited } else { Visibility::Hidden });
        if page.0 != *tab {
            continue;
        }
        commands.entity(e).despawn_related::<Children>();
        commands.entity(e).with_children(|p| match page.0 {
            CharTab::General => {
                page_header(p, &f, &player.name, true);
                stat_row(p, &f, "Health", format!("{} / {}", player.hp.max(0), player.max_hp), "");
                stat_row(p, &f, "Mana", format!("{} / {}", player.mana, player.max_mana), "");
                stat_row(p, &f, "Gold", state.gold.to_string(), "");
                page_header(p, &f, "Attributes", false);
                stat_row(p, &f, "Strength", a.strength.to_string(), "Adds half its value to Weapon Value.");
                stat_row(p, &f, "Agility", a.agility.to_string(), "");
                stat_row(
                    p,
                    &f,
                    "Willpower",
                    a.willpower.to_string(),
                    "Resists the Eye's gaze: strain builds 0.5% slower per point (up to 40%).",
                );
                stat_row(p, &f, "Intelligence", a.intelligence.to_string(), "");
                stat_row(p, &f, "Courage", a.courage.to_string(), "");
            }
            CharTab::Combat => {
                page_header(p, &f, "Offence", true);
                stat_row(
                    p,
                    &f,
                    "Weapon Value",
                    c.weapon_value.to_string(),
                    "Melee damage per swing, rolled from 80% to 120%.",
                );
                stat_row(
                    p,
                    &f,
                    "Melee Speed",
                    format!("{:.2} s", c.melee_speed_ms as f32 / 1000.0),
                    "Time between swings.",
                );
                stat_row(
                    p,
                    &f,
                    "Melee Critical",
                    c.melee_crit.to_string(),
                    "Crit chance: rating / target level %, up to 50%. Crits hit 1.5x.",
                );
                stat_row(p, &f, "Spell Critical", c.spell_crit.to_string(), "");
                page_header(p, &f, "Defence", false);
                stat_row(
                    p,
                    &f,
                    "Armor Value",
                    c.armor.to_string(),
                    "Reduces melee damage taken, up to 50% at 300 armour.",
                );
                stat_row(
                    p,
                    &f,
                    "Dodge Rating",
                    c.dodge.to_string(),
                    "Dodge chance: rating / attacker level %, up to 30%.",
                );
                stat_row(
                    p,
                    &f,
                    "Parry Rating",
                    c.parry.to_string(),
                    "Parry chance: rating / attacker level %, up to 25%. Parries halve the hit.",
                );
                stat_row(
                    p,
                    &f,
                    "Block Rating",
                    c.block.to_string(),
                    "Needs a shield. Block chance: rating / attacker level %, up to 50%.",
                );
                page_header(p, &f, "Resistances", false);
                for (name, v) in ["Frost", "Fire", "Shadow", "Holy"].iter().zip(c.resist) {
                    stat_row(p, &f, name, v.to_string(), "");
                }
            }
            CharTab::Skills => {
                let mut spells: Vec<_> = book
                    .known
                    .iter()
                    .filter(|s| !crate::spells_ui::AUTO_SPELLS.contains(s))
                    .filter_map(|s| data.spells.get(&(*s as i64)))
                    .collect();
                spells.sort_by_key(|t| (t.abilities_tab != 1, t.entry));
                let mut header: Option<bool> = None;
                for t in spells {
                    let is_spell = t.abilities_tab == 1;
                    if header != Some(is_spell) {
                        page_header(p, &f, if is_spell { "Spells" } else { "Actions" }, header.is_none());
                        header = Some(is_spell);
                    }
                    p.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(28.0),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            column_gap: Val::Px(7.0),
                            padding: UiRect::horizontal(Val::Px(3.0)),
                            ..default()
                        },
                        HoverTint,
                        CapturesPointer,
                        SkillRow,
                        Hint::new(t.name.clone()).with_body(describe(t, &player)),
                    ))
                    .with_children(|r| {
                        r.spawn((
                            Node { width: Val::Px(24.0), height: Val::Px(24.0), flex_shrink: 0.0, ..default() },
                            ImageNode::new(img(&data, &assets, &t.icon)),
                        ));
                        r.spawn((Text::new(t.name.clone()), f(13.0), TextColor(BONE)));
                    });
                }
                p.spawn((
                    Node { margin: UiRect::top(Val::Px(10.0)), padding: UiRect::horizontal(Val::Px(4.0)), ..default() },
                    Text::new("Click a skill to open Abilities (P) and place it on the action bar."),
                    f(11.0),
                    TextColor(GREY.with_alpha(0.75)),
                ));
            }
        });
    }
}

#[derive(Component)]
struct LootGoldRow;

#[allow(clippy::too_many_arguments)]
fn refresh_loot_window(
    mut commands: Commands,
    state: Res<ItemsState>,
    db: Res<ItemDb>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    font: Res<UiFont>,
    windows: Res<Windows>,
    mut out: MessageWriter<WindowCommand>,
    list: Query<Entity, With<LootList>>,
    mut shown: Local<Option<(EntityId, u32, Vec<Item>)>>,
) {
    if !state.is_changed() || *shown == state.loot {
        return;
    }
    shown.clone_from(&state.loot);
    let Ok(list) = list.single() else { return };
    let Some((_, gold, items)) = &state.loot else {
        if windows.is_open(WindowId::Loot) {
            out.write(WindowCommand::Close(WindowId::Loot));
        }
        return;
    };
    out.write(WindowCommand::Open(WindowId::Loot));
    commands.entity(list).despawn_related::<Children>();
    let f = TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() };
    let row = Node {
        width: Val::Percent(100.0),
        height: Val::Px(36.0),
        align_items: AlignItems::Center,
        column_gap: Val::Px(6.0),
        ..default()
    };
    commands.entity(list).with_children(|l| {
        if *gold > 0 {
            l.spawn((row.clone(), Button, CapturesPointer, LootGoldRow)).with_children(|r| {
                r.spawn((
                    Node { width: Val::Px(32.0), height: Val::Px(32.0), ..default() },
                    ImageNode::new(img(&data, &assets, "gossip_gold_pouch.png")),
                ));
                r.spawn((Text::new(format!("{gold} Gold Pieces")), f.clone(), TextColor(GOLD)));
            });
        }
        for (i, it) in items.iter().enumerate() {
            let Some(t) = db.template(it) else { continue };
            l.spawn((row.clone(), Button, CapturesPointer, ItemSlot(SlotRef::Loot(i)), BorderColor::all(Color::NONE)))
                .with_children(|r| {
                    r.spawn((
                        Node { width: Val::Px(32.0), height: Val::Px(32.0), ..default() },
                        ImageNode::new(img(&data, &assets, &t.icon)),
                    ));
                    let count = if it.count > 1 { format!(" x{}", it.count) } else { String::new() };
                    r.spawn((
                        Node { max_width: Val::Px(140.0), ..default() },
                        Text::new(format!("{}{count}", db.name(it))),
                        f.clone(),
                        TextColor(quality_color(t.quality)),
                    ));
                });
        }
    });
}

fn update_log(
    mut commands: Commands,
    time: Res<Time>,
    mut state: ResMut<ItemsState>,
    font: Res<UiFont>,
    panel: Query<Entity, With<LogPanel>>,
    mut shown: Local<usize>,
) {
    const LOG_SECS: f32 = 6.0;
    let now = time.elapsed_secs();
    if state.log.is_empty() && *shown == 0 {
        return;
    }
    state.log.retain(|l| now - l.2 < LOG_SECS);
    let excess = state.log.len().saturating_sub(8);
    state.log.drain(..excess);
    let Ok(panel) = panel.single() else { return };
    // Rebuilt every frame while lines are fading; a handful of text nodes at most.
    *shown = state.log.len();
    commands.entity(panel).despawn_related::<Children>();
    let f = TextFont { font: font.0.clone().into(), font_size: 14.0.into(), ..default() };
    commands.entity(panel).with_children(|p| {
        for (text, color, at) in &state.log {
            let alpha = (1.0 - ((now - at) / LOG_SECS).powi(3)).clamp(0.0, 1.0);
            p.spawn((Text::new(text.clone()), f.clone(), TextColor(color.with_alpha(alpha)), TextShadow::default()));
        }
    });
}

/// Pouch icons bobbing over lootable corpses.
fn loot_markers(
    mut commands: Commands,
    state: Res<ItemsState>,
    net: Res<Net>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    time: Res<Time>,
    units: Query<&Unit>,
    mut markers: Query<(Entity, &LootMarker, &mut Transform)>,
) {
    let want: HashSet<Entity> = state.lootable.iter().filter_map(|id| net.entities.get(id).copied()).collect();
    let mut have = HashSet::new();
    for (m, LootMarker(e), mut tf) in &mut markers {
        match units.get(*e).ok().filter(|_| want.contains(e)) {
            Some(u) => {
                let s = iso::to_screen(u.pos);
                let bob = (time.elapsed_secs() * 3.0).sin() * 3.0;
                tf.translation = Vec3::new(s.x, s.y + 30.0 + bob, 940.0);
                have.insert(*e);
            }
            None => commands.entity(m).despawn(),
        }
    }
    for e in want.difference(&have) {
        commands.spawn((
            LootMarker(*e),
            Sprite { image: img(&data, &assets, "gossip_gold_pouch.png"), ..default() },
            overlay_layer(),
            Transform::from_xyz(0.0, 0.0, 940.0).with_scale(Vec3::splat(0.75)),
        ));
    }
}

// ---------------------------------------------------------------- tooltip

fn weapon_type_name(w: i64) -> &'static str {
    match w {
        1 => "Axe",
        2 => "Bow",
        3 => "Mace",
        4 => "Sword",
        5 => "Staff",
        6 => "Dagger",
        7 => "Wand",
        _ => "",
    }
}

fn armor_type_name(t: &ItemTemplate) -> &'static str {
    match (t.equip_type, t.armor_type) {
        (equip::SHIELD, _) => "Shield",
        (_, 2..=4) => "Leather",
        (_, 5..=8) => "Mail",
        (_, 9..=11) => "Plate",
        (_, 1 | 12..=15) => "Cloth",
        _ => "",
    }
}

/// Tooltip lines (text, size, colour) for an item.
pub fn item_lines(db: &ItemDb, data: &GameData, it: &Item, level: u32) -> Vec<(String, f32, Color)> {
    let Some(t) = db.template(it) else { return vec![] };
    let affix = db.affix(it);
    let white = Color::WHITE;
    let mut lines = vec![(item::display_name(t, affix), 16.0, quality_color(t.quality))];
    if t.is_equippable() {
        let slot_name = match t.equip_type {
            equip::WEAPON => "Main Hand",
            equip::RANGED => "Ranged",
            equip::SHIELD => "Off Hand",
            e => slot::NAMES[item::slots_for(e)[0]],
        };
        let kind = if matches!(t.equip_type, equip::WEAPON | equip::RANGED) {
            weapon_type_name(t.weapon_type)
        } else {
            armor_type_name(t)
        };
        lines.push((format!("{slot_name}    {kind}"), 13.0, white));
    }
    let s = item::item_stats(t, affix);
    if s.weapon_value > 0 {
        let label = if t.equip_type == equip::RANGED { "Ranged Value" } else { "Weapon Value" };
        lines.push((format!("{} {label}    Speed {:.2}", s.weapon_value, s.speed_ms as f32 / 1000.0), 13.0, white));
    }
    if s.armor > 0 {
        lines.push((format!("{} Armor Value", s.armor), 13.0, white));
    }
    if s.block > 0 {
        lines.push((format!("{} Block Rating", s.block), 13.0, white));
    }
    for &(st, v) in &s.bonuses {
        lines.push((stat::equip_line(st, v), 13.0, quality_color(quality::GREEN)));
    }
    for sp in &t.spells {
        if let Some(spell) = data.spells.get(sp) {
            lines.push((format!("Use: {}", spell.aura_description.trim()), 13.0, quality_color(quality::GREEN)));
        }
    }
    if t.durability > 0 {
        lines.push((format!("Durability {0}/{0}", t.durability), 13.0, white));
    }
    if t.required_level > 1 {
        let color = if t.required_level > level as i64 { RED } else { white };
        lines.push((format!("Requires level {}", t.required_level), 13.0, color));
    }
    if t.flags & item::flags::QUEST_ITEM != 0 {
        lines.push(("Quest Item".into(), 13.0, white));
    }
    if t.sell_price > 0 {
        lines.push((format!("Sell Price: {} Gold Pieces", t.sell_price.max(1) * it.count.max(1) as i64), 12.0, GREY));
    }
    lines
}

#[allow(clippy::too_many_arguments)]
fn update_tooltip(
    mut commands: Commands,
    state: Res<ItemsState>,
    player: Res<PlayerState>,
    db: Res<ItemDb>,
    data: Res<GameData>,
    font: Res<UiFont>,
    window: Query<&Window, With<PrimaryWindow>>,
    slots: Query<(&Interaction, &ItemSlot)>,
    mut tooltip: Query<(Entity, &mut Node, &mut Visibility), With<ItemTooltip>>,
    mut shown: Local<Option<(SlotRef, Option<Item>)>>,
) {
    let Ok((entity, mut node, mut vis)) = tooltip.single_mut() else { return };
    // Debug aid: `DUSK_TOOLTIP_ITEM=<n>` shows bag slot n's tooltip (`e<n>` = equipment slot n).
    let forced = std::env::var("DUSK_TOOLTIP_ITEM").ok().and_then(|s| match s.strip_prefix('e') {
        Some(n) => n.parse().ok().map(SlotRef::Equip),
        None => s.parse().ok().map(SlotRef::Bag),
    });
    let hovered = slots
        .iter()
        .find(|(i, _)| **i == Interaction::Hovered)
        .map(|(_, s)| s.0)
        .or(forced)
        // Empty equipment slots still say what goes there.
        .and_then(|r| match (r, slot_item(&state, r)) {
            (_, Some(it)) => Some((r, Some(it))),
            (SlotRef::Equip(_), None) => Some((r, None)),
            _ => None,
        });
    let Some((r, it)) = hovered else {
        *vis = Visibility::Hidden;
        *shown = None;
        return;
    };
    let cursor = window
        .single()
        .ok()
        .and_then(|w| w.cursor_position())
        .filter(|_| forced.is_none())
        .unwrap_or(Vec2::new(900.0, 200.0));
    *vis = Visibility::Visible;
    let width = window.single().map(|w| w.width()).unwrap_or(1280.0);
    let height = window.single().map(|w| w.height()).unwrap_or(720.0);
    // Prefer left of the cursor (the inventory sits on the right edge).
    node.left = Val::Px(if cursor.x > width / 2.0 { (cursor.x - 280.0).max(4.0) } else { cursor.x + 20.0 });
    node.top = Val::Px((cursor.y - 40.0).clamp(4.0, height - 260.0));
    if *shown == Some((r, it)) && !player.is_changed() {
        return;
    }
    *shown = Some((r, it));
    commands.entity(entity).despawn_related::<Children>();
    let f = |size: f32| TextFont { font: font.0.clone().into(), font_size: size.into(), ..default() };
    let lines = match (r, it) {
        (SlotRef::Equip(s), None) => vec![(slot::NAMES[s].to_string(), 14.0, GOLD), ("Empty".into(), 12.0, GREY)],
        (_, None) => vec![],
        (r, Some(it)) => {
            let mut lines = item_lines(&db, &data, &it, player.level);
            let usable = db.template(&it).is_some_and(|t| t.is_equippable() || !t.spells.is_empty());
            let action = match r {
                SlotRef::Equip(_) => "Click to unequip",
                SlotRef::Bag(_) if usable => "Click to equip / use",
                SlotRef::Loot(_) => "Click to take",
                SlotRef::Bag(_) => "",
            };
            if !action.is_empty() {
                lines.push((action.into(), 11.0, GREY.with_alpha(0.8)));
            }
            lines
        }
    };
    commands.entity(entity).with_children(|p| {
        for (text, size, color) in lines {
            p.spawn((
                Text::new(text),
                f(size),
                TextColor(color),
                Node { margin: UiRect::bottom(Val::Px(2.0)), ..default() },
            ));
        }
    });
}
