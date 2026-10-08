//! Isometric projection. Cells are 64x32 diamonds; world coordinates are in
//! cell units (fractional), with (x + 0.5, y + 0.5) being a cell's centre.

use bevy::math::Vec2;

pub const TILE_W: f32 = 64.0;
pub const TILE_H: f32 = 32.0;

/// Cell-space -> Bevy world space (y up).
pub fn to_screen(cell: Vec2) -> Vec2 {
    Vec2::new((cell.x - cell.y) * TILE_W / 2.0, -(cell.x + cell.y) * TILE_H / 2.0)
}

/// Bevy world space -> cell-space.
pub fn to_cell(screen: Vec2) -> Vec2 {
    let sx = screen.x / (TILE_W / 2.0);
    let sy = -screen.y / (TILE_H / 2.0);
    Vec2::new((sx + sy) / 2.0, (sy - sx) / 2.0)
}

/// Draw order for upright things (walls, units): further down-screen = in front.
pub fn depth(cell: Vec2) -> f32 {
    1.0 + (cell.x + cell.y) * 0.5
}

/// Sprite-sheet direction for a unit orientation, as in `ClientUnit::computeDirection`
/// (0x5548f0). `orientation` is radians of `atan2(dy, dx)` in *cell* space (same as
/// `npc.orientation` in game.db). Sheet directions are 0=W 1=NW 2=N 3=NE 4=E 5=SE 6=S 7=SW.
pub fn direction_from_orientation(orientation: f32) -> u8 {
    use std::f32::consts::{FRAC_PI_4, FRAC_PI_8, TAU};
    const DIRS: [u8; 8] = [5, 6, 7, 0, 1, 2, 3, 4];
    let a = (orientation + FRAC_PI_8).rem_euclid(TAU);
    DIRS[((a / FRAC_PI_4) as usize).min(7)]
}

/// Orientation (cell-space radians, [0, 2pi)) of a movement vector in cell space.
pub fn orientation_of(cell_delta: Vec2) -> f32 {
    cell_delta.y.atan2(cell_delta.x).rem_euclid(std::f32::consts::TAU)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let c = Vec2::new(12.25, 7.5);
        assert!((to_cell(to_screen(c)) - c).length() < 1e-4);
    }

    #[test]
    fn directions() {
        // cell +x is screen right-down (SE), +y is left-down (SW)
        assert_eq!(direction_from_orientation(orientation_of(Vec2::X)), 5);
        assert_eq!(direction_from_orientation(orientation_of(Vec2::Y)), 7);
        assert_eq!(direction_from_orientation(orientation_of(-Vec2::X)), 1);
        assert_eq!(direction_from_orientation(orientation_of(-Vec2::Y)), 3);
        // screen up (N) is cell (-1,-1)
        assert_eq!(direction_from_orientation(orientation_of(Vec2::new(-1.0, -1.0))), 2);
        assert_eq!(direction_from_orientation(orientation_of(Vec2::new(1.0, 1.0))), 6);
    }
}
