//! Per-sprite effects from `game.db`: particle emitters (`sprite_psi`), lights
//! (`sprite_light`) and the zone darkness (`zone_template.night_pct`).
//!
//! Both sprite tables are keyed by the sprite's texture filename; a map texture
//! entry may itself be a `.psi` name (an invisible sprite that only carries effects).
//! Semantics (client, see `docs/formats.md`):
//! - `sprite_psi` (`Sprite::renderScript`, 0x50e8c0): emitter at
//!   `sprite position - hotspot + (x_offset, y_offset)`, i.e. relative to the sprite
//!   image's top-left. Sprites without a texture (`*.psi` entries) use hotspot (1, 1).
//! - `sprite_light` (`ClientMap_buildDrawList`, 0x4b0850, and 0x4b8640): light at the
//!   cell's render position + offset; `color` is packed `0xRRGGBBAA`.

use crate::db::{GameDb, int, real, text};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub struct SpritePsi {
    pub psi: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpriteLight {
    /// Packed `0xRRGGBBAA` (`sf::Color(Uint32)`), alpha included.
    pub color: u32,
    pub x: i32,
    pub y: i32,
    /// Stored as `intensity / 100` but never read by the drawing code.
    pub intensity: i32,
    /// Additive `light_source.png` glow drawn over everything (after the upright layer).
    pub apply_ground: bool,
    /// Additive glow drawn in the sprite's own depth-sorted slot.
    pub apply_top: bool,
    /// Scale of both the glow sprite and the darkness cut-out.
    pub scale: f32,
}

impl GameDb {
    /// `sprite_psi`, keyed by lowercase sprite filename (several emitters per sprite possible).
    pub fn sprite_psi(&self) -> rusqlite::Result<HashMap<String, Vec<SpritePsi>>> {
        let mut out: HashMap<String, Vec<SpritePsi>> = HashMap::new();
        let mut stmt = self.conn().prepare("SELECT filename, psi, x_offset, y_offset FROM sprite_psi")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                text(r, "filename").to_lowercase(),
                SpritePsi { psi: text(r, "psi"), x: int(r, "x_offset") as i32, y: int(r, "y_offset") as i32 },
            ))
        })?;
        for row in rows {
            let (k, v) = row?;
            out.entry(k).or_default().push(v);
        }
        Ok(out)
    }

    /// `sprite_light`, keyed by lowercase sprite filename.
    pub fn sprite_lights(&self) -> rusqlite::Result<HashMap<String, Vec<SpriteLight>>> {
        let mut out: HashMap<String, Vec<SpriteLight>> = HashMap::new();
        let mut stmt = self.conn().prepare("SELECT * FROM sprite_light")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                text(r, "filename").to_lowercase(),
                SpriteLight {
                    color: int(r, "color") as u32,
                    x: int(r, "x_offset") as i32,
                    y: int(r, "y_offset") as i32,
                    intensity: int(r, "intensity") as i32,
                    apply_ground: int(r, "bool_applyground") != 0,
                    apply_top: int(r, "bool_applytop") != 0,
                    scale: real(r, "scale"),
                },
            ))
        })?;
        for row in rows {
            let (k, v) = row?;
            out.entry(k).or_default().push(v);
        }
        Ok(out)
    }

    /// `zone_template.night_pct` by zone id (missing/NULL = 0). The client sets the map's
    /// target brightness to `1 - night_pct` when the player enters the zone (0x556be0).
    pub fn zone_night_pct(&self) -> rusqlite::Result<HashMap<u32, f32>> {
        let mut stmt = self.conn().prepare("SELECT id, night_pct FROM zone_template")?;
        let rows = stmt.query_map([], |r| Ok((int(r, "id") as u32, real(r, "night_pct"))))?;
        rows.collect()
    }
}

/// Parses a `sprite_fx.txt` shipped with our own art (tools/artgen), the custom-art equivalent
/// of the `sprite_psi` / `sprite_light` tables:
/// `psi <sprite> <file.psi> <x> <y>` and
/// `light <sprite> <rrggbbaa hex> <x> <y> <ground 0/1> <top 0/1> <scale>`; `#` comments.
/// Returns (lowercase sprite name, effect) pairs.
pub fn parse_custom_fx(text: &str) -> (Vec<(String, SpritePsi)>, Vec<(String, SpriteLight)>) {
    let (mut psi, mut lights) = (Vec::new(), Vec::new());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let v: Vec<&str> = line.split_whitespace().collect();
        match v[..] {
            ["psi", sprite, file, x, y] => {
                if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                    psi.push((sprite.to_lowercase(), SpritePsi { psi: file.to_string(), x, y }));
                }
            }
            ["light", sprite, color, x, y, ground, top, scale] => {
                let parsed = (u32::from_str_radix(color, 16), x.parse(), y.parse(), scale.parse());
                if let (Ok(color), Ok(x), Ok(y), Ok(scale)) = parsed {
                    lights.push((
                        sprite.to_lowercase(),
                        SpriteLight {
                            color,
                            x,
                            y,
                            intensity: 100,
                            apply_ground: ground == "1",
                            apply_top: top == "1",
                            scale,
                        },
                    ));
                }
            }
            _ => {}
        }
    }
    (psi, lights)
}

#[cfg(test)]
mod custom_tests {
    use super::*;

    #[test]
    fn parses_custom_fx() {
        let (p, l) = parse_custom_fx("# c\npsi A.png campfire.psi 20 8\nlight A.png e25822c8 0 16 1 0 1.0\nbogus\n");
        assert_eq!(p, vec![("a.png".into(), SpritePsi { psi: "campfire.psi".into(), x: 20, y: 8 })]);
        assert_eq!((l[0].1.color, l[0].1.y, l[0].1.apply_ground, l[0].1.apply_top), (0xe25822c8, 16, true, false));
    }
}
