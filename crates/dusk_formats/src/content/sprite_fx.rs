//! Per-sprite data of map art: particle emitters, lights, pivots (hotspots) and roofs.

use crate::sprite_fx::{SpriteLight, SpritePsi, parse_custom_fx};
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
    /// Zone id -> darkness (0..1). Stream 0 only: Stream B replaces it with `MapInfo.darkness`.
    pub zone_night: HashMap<u32, f32>,
}

pub fn load(root: &Path) -> anyhow::Result<SpriteFx> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "sprite_psi, sprite_light, sprite_hotspot, zone_template.night_pct");
    let mut psi = db.sprite_psi()?;
    let mut lights = db.sprite_lights()?;
    let mut hotspots = db.sprite_hotspots()?;
    let (own_psi, own_lights) = parse_custom_fx(&metadata(root, "sprite_fx.txt"));
    for (k, v) in own_psi {
        psi.entry(k).or_default().push(v);
    }
    for (k, v) in own_lights {
        lights.entry(k).or_default().push(v);
    }
    hotspots.extend(parse_hotspots(&metadata(root, "hotspots.txt")));
    Ok(SpriteFx {
        psi,
        lights,
        hotspots,
        roofs: parse_roofs(&metadata(root, "roofs.txt")),
        zone_night: db.zone_night_pct()?,
    })
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
}
