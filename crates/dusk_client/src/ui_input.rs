//! Shared "the UI owns this input" flags, so gameplay systems don't react to keys typed
//! into the chat box or clicks that land on a HUD panel.
//!
//! Gameplay input systems check [`UiInputCaptured`] before acting on keyboard/mouse input.
//! UI modules set `keyboard` (text entry) themselves; `pointer` is computed every frame from
//! nodes marked [`CapturesPointer`].

use bevy::prelude::*;
use bevy::ui::UiSystems;

pub struct UiInputPlugin;

impl Plugin for UiInputPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UiInputCaptured>().add_systems(PreUpdate, update_pointer.after(UiSystems::Focus));
    }
}

#[derive(Resource, Default, Debug)]
pub struct UiInputCaptured {
    /// A text field has keyboard focus (also true for the frame in which it lost it, so the
    /// `Enter`/`Escape` that closed it doesn't leak into gameplay).
    pub keyboard: bool,
    /// The cursor is over a HUD panel: world clicks should be ignored.
    pub pointer: bool,
}

/// Marks a UI node whose area swallows world clicks (needs an [`Interaction`], added automatically).
#[derive(Component, Default)]
#[require(Interaction)]
pub struct CapturesPointer;

fn update_pointer(
    mut captured: ResMut<UiInputCaptured>,
    nodes: Query<(&Interaction, &InheritedVisibility), With<CapturesPointer>>,
) {
    let over = nodes.iter().any(|(i, v)| v.get() && *i != Interaction::None);
    if captured.pointer != over {
        captured.pointer = over;
    }
}
