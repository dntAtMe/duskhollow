//! Per-sprite effects of map art (`sprite_fx.txt`, written next to the art by tools/artgen):
//! particle emitters and lights, keyed by the sprite's texture file name. A map texture entry may
//! itself be a `.psi` name: an invisible sprite that only carries effects (fireflies).
//!
//! - Emitter at `sprite position - hotspot + (x, y)`, i.e. relative to the sprite image's
//!   top-left. Sprites without a texture (`*.psi` entries) use hotspot (1, 1).
//! - Light at the cell's render position + offset; `color` is packed `0xRRGGBBAA`.

#[derive(Debug, Clone, PartialEq)]
pub struct SpritePsi {
    /// Particle system name (`content::particles::key`).
    pub psi: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpriteLight {
    /// Packed `0xRRGGBBAA`, alpha included.
    pub color: u32,
    pub x: i32,
    pub y: i32,
    /// Unused (always 100).
    pub intensity: i32,
    /// Additive `fx_light_glow.png` glow drawn over everything (after the upright layer).
    pub apply_ground: bool,
    /// Additive glow drawn in the sprite's own depth-sorted slot.
    pub apply_top: bool,
    /// Scale of both the glow sprite and the darkness cut-out.
    pub scale: f32,
}

/// Parses a `sprite_fx.txt`:
/// `particles <sprite> <name> <x> <y>` (alias `psi`; a `.psi` suffix is dropped) and
/// `light <sprite> <rrggbbaa hex> <x> <y> <ground 0/1> <top 0/1> <scale>`; `#` comments.
/// Returns (lowercase sprite name, effect) pairs.
pub fn parse_sprite_fx(text: &str) -> (Vec<(String, SpritePsi)>, Vec<(String, SpriteLight)>) {
    let (mut psi, mut lights) = (Vec::new(), Vec::new());
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let v: Vec<&str> = line.split_whitespace().collect();
        match v[..] {
            ["particles" | "psi", sprite, name, x, y] => {
                if let (Ok(x), Ok(y)) = (x.parse(), y.parse()) {
                    psi.push((sprite.to_lowercase(), SpritePsi { psi: crate::content::particles::key(name), x, y }));
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
mod tests {
    use super::*;

    #[test]
    fn parses_sprite_fx() {
        let (p, l) = parse_sprite_fx(
            "# c\nparticles A.png campfire 20 8\npsi f.psi Fireflies.psi 0 -32\nlight A.png e25822c8 0 16 1 0 1.0\nbogus\n",
        );
        assert_eq!(p[0], ("a.png".into(), SpritePsi { psi: "campfire".into(), x: 20, y: 8 }));
        assert_eq!(p[1], ("f.psi".into(), SpritePsi { psi: "fireflies".into(), x: 0, y: -32 }));
        assert_eq!((l[0].1.color, l[0].1.y, l[0].1.apply_ground, l[0].1.apply_top), (0xe25822c8, 16, true, false));
    }
}
