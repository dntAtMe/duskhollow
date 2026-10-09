//! Per-sprite data of map art: particle emitters, lights, pivots (hotspots) and roofs.

use crate::sprite_fx::{SpriteLight, SpritePsi, parse_sprite_fx};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct SpriteFx {
    /// Lowercase sprite file name -> particle emitters.
    pub psi: HashMap<String, Vec<SpritePsi>>,
    /// Lowercase sprite file name -> lights.
    pub lights: HashMap<String, Vec<SpriteLight>>,
    /// Lowercase sprite file name -> pivot in pixels.
    pub hotspots: HashMap<String, (i32, i32)>,
    /// (lowercase sprite name prefix, cells roofed beyond the back cell).
    pub roofs: Vec<(String, (i32, i32))>,
}

/// Every `sprite_fx.txt`, `hotspots.txt` and `roofs.txt` under `content/`.
pub fn load(root: &Path) -> anyhow::Result<SpriteFx> {
    let mut fx = SpriteFx::default();
    let (psi, lights) = parse_sprite_fx(&metadata(root, "sprite_fx.txt"));
    for (k, v) in psi {
        fx.psi.entry(k).or_default().push(v);
    }
    for (k, v) in lights {
        fx.lights.entry(k).or_default().push(v);
    }
    fx.hotspots.extend(parse_hotspots(&metadata(root, "hotspots.txt")));
    fx.roofs = parse_roofs(&metadata(root, "roofs.txt"));
    Ok(fx)
}

/// Every file called `file_name` under `content/` (metadata written next to the art by
/// tools/artgen), concatenated.
fn metadata(root: &Path, file_name: &str) -> String {
    let mut out = Vec::new();
    let mut stack = vec![root.join("content")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == file_name) {
                out.push(std::fs::read_to_string(&p).unwrap_or_default());
            }
        }
    }
    out.join("\n")
}

/// `name x y` lines.
fn parse_hotspots(text: &str) -> Vec<(String, (i32, i32))> {
    text.lines()
        .filter_map(|line| {
            let v: Vec<&str> = line.split_whitespace().collect();
            let [name, x, y] = v[..] else { return None };
            Some((name.to_lowercase(), (x.parse().ok()?, y.parse().ok()?)))
        })
        .collect()
}

/// `roof <sprite prefix> <dx> <dy>` lines; `#` comments.
fn parse_roofs(text: &str) -> Vec<(String, (i32, i32))> {
    text.lines()
        .filter_map(|l| match l.split_whitespace().collect::<Vec<_>>()[..] {
            ["roof", name, dx, dy] => Some((name.to_lowercase(), (dx.parse().ok()?, dy.parse().ok()?))),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotspots_and_roofs() {
        assert_eq!(parse_hotspots("A.png 3 -4\n# c\nbad\n"), [("a.png".to_string(), (3, -4))]);
        assert_eq!(parse_roofs("# c\nroof House_ 2 1\n"), [("house_".to_string(), (2, 1))]);
    }

    /// Every shipped emitter names a particle system of `data/particles.txt`.
    #[test]
    fn shipped_emitters_resolve() {
        let root = crate::assets_root();
        let fx = load(&root).unwrap();
        let particles = super::super::particles::load(&root).unwrap();
        assert!(fx.psi.contains_key("green_firefly.psi"), "glade fireflies");
        for (sprite, e) in fx.psi.iter().flat_map(|(k, v)| v.iter().map(move |e| (k, e))) {
            assert!(particles.contains_key(&e.psi), "{sprite}: particles {}", e.psi);
        }
        assert!(!fx.lights.is_empty());
    }
}
