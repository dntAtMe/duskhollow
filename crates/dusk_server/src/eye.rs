//! Scripted control of the Eye. The director writes these; the gaze system (which owns the
//! Eye's state per map) reads them with a `MessageReader<EyeCommand>`.

use bevy::prelude::*;

#[derive(Message, Debug, Clone, Copy, PartialEq)]
pub enum EyeCommand {
    /// Force the Eye on `map` wide open for (at most) `secs`.
    Open { map: i64, secs: f32 },
    /// Let it settle back to half-lidded now.
    Settle { map: i64 },
}
