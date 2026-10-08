//! The Eye's gaze as seen by the client: strain, cover and eye state of the local player
//! (docs/demo-plan.md). Written from server messages, read by the HUD, visuals and audio.

use bevy::prelude::*;

pub struct GazePlugin;

impl Plugin for GazePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GazeView>();
    }
}

/// The Eye's lid, broadcast by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EyeState {
    #[default]
    Lidded,
    Opening,
    Open,
    Closing,
}

/// Cover under the local player (`.cover` sidecar of the map).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoverKind {
    #[default]
    Open,
    Shade,
    Shelter,
    Cairn,
}

#[derive(Resource, Debug, Clone, Default)]
pub struct GazeView {
    /// False on maps without a `.cover` sidecar: no strain, no crimson grade.
    pub active: bool,
    pub eye: EyeState,
    /// 0 = half-lidded, 1 = wide open (smoothed for visuals).
    pub openness: f32,
    /// 0..=100.
    pub strain: f32,
    /// 0..=100.
    pub corruption: f32,
    pub cover: CoverKind,
    pub in_combat: bool,
    /// Inside the wandering gaze spot.
    pub in_spot: bool,
}
