//! Menu widgets on the "iron & oak" skin (`tools/artgen/menu_ui.py`): buttons, sliders,
//! check boxes, cyclers, text fields, class medallions. Each interactive widget carries a
//! [`Widget`] (what it controls) and a [`Focus`] index (keyboard order within its screen).

use super::{Action, CycleKey, FieldKey, SliderKey, ToggleKey};
use crate::data::GameData;
use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

// 9-slice borders, as drawn by menu_ui.py.
const SLICE_BUTTON: f32 = 9.0;
const SLICE_PANEL: f32 = 18.0;
const SLICE_FIELD: f32 = 7.0;
const SLICE_TRACK: f32 = 5.0;

pub const BONE: Color = Color::srgb(0.88, 0.83, 0.72);
pub const PARCH: Color = Color::srgb(0.80, 0.72, 0.56);
pub const GOLD: Color = Color::srgb(0.95, 0.78, 0.42);
pub const DIM: Color = Color::srgb(0.62, 0.56, 0.48);
pub const FAINT: Color = Color::srgb(0.45, 0.40, 0.34);
pub const BRONZE: Color = Color::srgb(0.55, 0.42, 0.24);
pub const EMBER: Color = Color::srgb(0.93, 0.45, 0.18);
pub const ERROR: Color = Color::srgb(0.92, 0.42, 0.30);

/// Images and fonts of the menu skin.
#[derive(Resource, Clone)]
pub struct Skin {
    pub font: Handle<Font>,
    pub bold: Handle<Font>,
    pub button: [Handle<Image>; 4],
    pub panel: Handle<Image>,
    pub panel_dark: Handle<Image>,
    pub field: [Handle<Image>; 2],
    pub track: Handle<Image>,
    pub fill: Handle<Image>,
    pub knob: [Handle<Image>; 2],
    pub check: [Handle<Image>; 2],
    pub class: [Handle<Image>; 4],
    pub class_ring: Handle<Image>,
    pub rule: Handle<Image>,
}

impl Skin {
    pub fn load(data: &GameData, assets: &AssetServer, font: Handle<Font>, bold: Handle<Font>) -> Self {
        let img = |n: &str| data.asset_path(&format!("{n}.png")).map(|p| assets.load(p)).unwrap_or_default();
        Self {
            font,
            bold,
            button: ["idle", "hover", "press", "disabled"].map(|s| img(&format!("menu_button_{s}"))),
            panel: img("menu_panel"),
            panel_dark: img("menu_panel_dark"),
            field: [img("menu_field"), img("menu_field_focus")],
            track: img("menu_slider_track"),
            fill: img("menu_slider_fill"),
            knob: [img("menu_slider_knob"), img("menu_slider_knob_hover")],
            check: [img("menu_check_off"), img("menu_check_on")],
            class: [1, 2, 3, 4].map(|i| img(&format!("menu_class_{i}"))),
            class_ring: img("menu_class_ring_sel"),
            rule: img("menu_rule"),
        }
    }

    pub fn text(&self, size: f32) -> TextFont {
        TextFont { font: self.font.clone().into(), font_size: size.into(), ..default() }
    }

    pub fn title(&self, size: f32) -> TextFont {
        TextFont { font: self.bold.clone().into(), font_size: size.into(), ..default() }
    }
}

pub fn shadow() -> TextShadow {
    TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.9) }
}

pub fn sliced(image: Handle<Image>, border: f32) -> ImageNode {
    ImageNode::new(image).with_mode(NodeImageMode::Sliced(TextureSlicer {
        border: BorderRect::all(border),
        center_scale_mode: SliceScaleMode::Stretch,
        sides_scale_mode: SliceScaleMode::Stretch,
        max_corner_scale: 1.0,
    }))
}

pub fn panel_image(skin: &Skin, dark: bool) -> ImageNode {
    // Tiled, not stretched: the dithered oak keeps its pixels at any panel size.
    let tile = SliceScaleMode::Tile { stretch_value: 1.0 };
    ImageNode::new(if dark { skin.panel_dark.clone() } else { skin.panel.clone() }).with_mode(NodeImageMode::Sliced(
        TextureSlicer {
            border: BorderRect::all(SLICE_PANEL),
            center_scale_mode: tile,
            sides_scale_mode: tile,
            max_corner_scale: 1.0,
        },
    ))
}

/// What a focusable widget controls.
#[derive(Component, Clone, Debug, PartialEq)]
pub enum Widget {
    Button(Action),
    Slider(SliderKey),
    Toggle(ToggleKey),
    Cycle(CycleKey),
    Field(FieldKey),
    /// The class picker row (left/right picks).
    Classes,
}

/// Keyboard order within the current screen (0 = first).
#[derive(Component, Clone, Copy)]
pub struct Focus(pub usize);

/// A button that cannot be used right now (drawn with the disabled plate).
#[derive(Component)]
pub struct Disabled;

/// Label text of a button / value text of a slider or cycler.
#[derive(Component)]
pub struct ValueText;

/// The number next to a slider.
#[derive(Component)]
pub struct SliderValue(pub SliderKey);

/// Slider fill (width follows the value) and knob.
#[derive(Component)]
pub struct SliderFill;
#[derive(Component)]
pub struct SliderKnob;

/// The text inside a text field.
#[derive(Component)]
pub struct FieldText;

/// One class medallion (1..=4) of the picker.
#[derive(Component, Clone, Copy)]
pub struct ClassCard(pub u8);
#[derive(Component)]
pub struct ClassRing;

pub const SLIDER_W: f32 = 230.0;
pub const ROW_LABEL_W: f32 = 170.0;

/// A plate button with a centred label.
pub fn button(
    p: &mut ChildSpawnerCommands,
    skin: &Skin,
    label: &str,
    action: Action,
    focus: usize,
    w: f32,
    h: f32,
) -> Entity {
    p.spawn((
        Node {
            width: Val::Px(w),
            height: Val::Px(h),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            ..default()
        },
        Button,
        sliced(skin.button[0].clone(), SLICE_BUTTON),
        Widget::Button(action),
        Focus(focus),
    ))
    .with_child((
        Text::new(label),
        skin.text(if h >= 36.0 { 18.0 } else { 15.0 }),
        TextColor(PARCH),
        shadow(),
        ValueText,
    ))
    .id()
}

/// A labelled row: label on the left, the control on the right.
pub fn row(p: &mut ChildSpawnerCommands, skin: &Skin, label: &str, control: impl FnOnce(&mut ChildSpawnerCommands)) {
    p.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(14.0),
        min_height: Val::Px(30.0),
        ..default()
    })
    .with_children(|r| {
        r.spawn(Node {
            width: Val::Px(ROW_LABEL_W),
            height: Val::Px(30.0),
            align_items: AlignItems::Center,
            ..default()
        })
        .with_child((Text::new(label), skin.text(15.0), TextColor(DIM), shadow()));
        control(r);
    });
}

pub fn slider(p: &mut ChildSpawnerCommands, skin: &Skin, key: SliderKey, focus: usize) {
    p.spawn((Node { align_items: AlignItems::Center, column_gap: Val::Px(12.0), ..default() },)).with_children(|r| {
        r.spawn((
            Node { width: Val::Px(SLIDER_W), height: Val::Px(24.0), align_items: AlignItems::Center, ..default() },
            Button,
            RelativeCursorPosition::default(),
            Widget::Slider(key),
            Focus(focus),
        ))
        .with_children(|t| {
            t.spawn((
                Node { width: Val::Percent(100.0), height: Val::Px(12.0), ..default() },
                sliced(skin.track.clone(), SLICE_TRACK),
            ))
            .with_child((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(3.0),
                    top: Val::Px(3.0),
                    width: Val::Px(0.0),
                    height: Val::Px(6.0),
                    ..default()
                },
                ImageNode::new(skin.fill.clone()).with_mode(NodeImageMode::Stretch),
                SliderFill,
            ));
            t.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: Val::Px(0.0),
                    width: Val::Px(14.0),
                    height: Val::Px(24.0),
                    ..default()
                },
                ImageNode::new(skin.knob[0].clone()),
                SliderKnob,
            ));
        });
        r.spawn((
            Node { width: Val::Px(44.0), ..default() },
            Text::new(""),
            skin.text(15.0),
            TextColor(BONE),
            shadow(),
            SliderValue(key),
        ));
    });
}

pub fn toggle(p: &mut ChildSpawnerCommands, skin: &Skin, key: ToggleKey, focus: usize, note: Option<&str>) {
    p.spawn(Node { align_items: AlignItems::Center, column_gap: Val::Px(10.0), ..default() }).with_children(|r| {
        r.spawn((
            Node { width: Val::Px(22.0), height: Val::Px(22.0), ..default() },
            Button,
            ImageNode::new(skin.check[0].clone()),
            Widget::Toggle(key),
            Focus(focus),
        ));
        if let Some(note) = note {
            r.spawn((Text::new(note), skin.text(12.0), TextColor(FAINT), shadow()));
        }
    });
}

/// `<  value  >` with clickable arrows.
pub fn cycler(p: &mut ChildSpawnerCommands, skin: &Skin, key: CycleKey, focus: usize, w: f32) {
    p.spawn((
        Node {
            width: Val::Px(w),
            height: Val::Px(30.0),
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            ..default()
        },
        Button,
        RelativeCursorPosition::default(),
        sliced(skin.field[0].clone(), SLICE_FIELD),
        Widget::Cycle(key),
        Focus(focus),
    ))
    .with_children(|c| {
        c.spawn((
            Text::new("<"),
            skin.text(16.0),
            TextColor(BRONZE),
            shadow(),
            Node { margin: UiRect::left(Val::Px(10.0)), ..default() },
        ));
        c.spawn((Text::new(""), skin.text(15.0), TextColor(BONE), shadow(), ValueText));
        c.spawn((
            Text::new(">"),
            skin.text(16.0),
            TextColor(BRONZE),
            shadow(),
            Node { margin: UiRect::right(Val::Px(10.0)), ..default() },
        ));
    });
}

pub fn field(p: &mut ChildSpawnerCommands, skin: &Skin, key: FieldKey, focus: usize, w: f32) {
    p.spawn((
        Node {
            width: Val::Px(w),
            height: Val::Px(30.0),
            align_items: AlignItems::Center,
            overflow: Overflow::clip(),
            ..default()
        },
        Button,
        sliced(skin.field[0].clone(), SLICE_FIELD),
        Widget::Field(key),
        Focus(focus),
    ))
    .with_child((
        Text::new(""),
        skin.text(16.0),
        TextColor(BONE),
        shadow(),
        FieldText,
        Node { margin: UiRect::left(Val::Px(10.0)), ..default() },
    ));
}

/// The four class medallions in a row (one focus stop; arrows pick).
pub fn class_row(p: &mut ChildSpawnerCommands, skin: &Skin, names: &[String; 4], focus: usize) {
    p.spawn((
        Node { column_gap: Val::Px(18.0), justify_content: JustifyContent::Center, ..default() },
        Widget::Classes,
        Focus(focus),
        Interaction::None,
    ))
    .with_children(|r| {
        for (i, name) in names.iter().enumerate() {
            let class = i as u8 + 1;
            r.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    width: Val::Px(110.0),
                    row_gap: Val::Px(4.0),
                    ..default()
                },
                Button,
                ClassCard(class),
            ))
            .with_children(|c| {
                c.spawn((Node {
                    width: Val::Px(84.0),
                    height: Val::Px(84.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },))
                    .with_children(|m| {
                        m.spawn((
                            Node { width: Val::Px(76.0), height: Val::Px(76.0), ..default() },
                            ImageNode::new(skin.class[i].clone()),
                        ));
                        m.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                width: Val::Px(84.0),
                                height: Val::Px(84.0),
                                ..default()
                            },
                            ImageNode::new(skin.class_ring.clone()),
                            Visibility::Hidden,
                            ClassRing,
                        ));
                    });
                c.spawn((Text::new(name.clone()), skin.text(15.0), TextColor(DIM), shadow(), ValueText));
            });
        }
    });
}

/// Visual state of the plate under the cursor / keyboard focus.
pub fn button_art(skin: &Skin, focused: bool, pressed: bool, disabled: bool) -> Handle<Image> {
    let i = if disabled {
        3
    } else if pressed {
        2
    } else if focused {
        1
    } else {
        0
    };
    skin.button[i].clone()
}

pub fn field_art(skin: &Skin, focused: bool) -> Handle<Image> {
    skin.field[focused as usize].clone()
}
