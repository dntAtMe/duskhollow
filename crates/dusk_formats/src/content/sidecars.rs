//! Map sidecars: `maps/<name>.spawns`, `.markers` and `.cover` next to `maps/<name>.map`.

use super::types::NpcSpawn;

/// Parses a `maps/<name>.spawns` sidecar: `entry x y orientation wander_distance` per line,
/// `#` comments. Spawns get synthetic guids from `first_guid` and wander if the distance is > 0.
pub fn parse_spawns(text: &str, map: i64, first_guid: i64) -> Vec<NpcSpawn> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .enumerate()
        .filter_map(|(i, l)| {
            let v: Vec<f32> = l.split_whitespace().map(|s| s.parse().ok()).collect::<Option<_>>()?;
            (v.len() >= 3).then(|| NpcSpawn {
                guid: first_guid + i as i64,
                entry: v[0] as i64,
                map,
                x: v[1],
                y: v[2],
                orientation: v.get(3).copied().unwrap_or(0.0),
                respawn_time: 60,
                movement_type: if v.get(4).copied().unwrap_or(0.0) > 0.0 { 1 } else { 0 },
                wander_distance: v.get(4).copied().unwrap_or(0.0) as i64,
            })
        })
        .collect()
}

/// A named point of a map (`maps/<name>.markers`: `name x y [radius]` per line, cells).
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
}

pub fn parse_markers(text: &str) -> Vec<Marker> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?.to_string();
            let v: Vec<f32> = it.map(|s| s.parse().ok()).collect::<Option<_>>()?;
            (v.len() >= 2).then(|| Marker { name, x: v[0], y: v[1], radius: v.get(2).copied().unwrap_or(3.0) })
        })
        .collect()
}

/// Cover of one cell (`maps/<name>.cover`, docs/demo-plan.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    #[default]
    Open,
    Shade,
    Shelter,
    Cairn,
}

/// Per-cell cover grid: first line `W H`, then H rows of W chars (`.` open, `s` shade,
/// `S` deep shelter, `C` rest cairn). Cells outside the grid are open sky.
#[derive(Debug, Clone, Default)]
pub struct CoverGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Cover>,
}

impl CoverGrid {
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines().map(str::trim_end).filter(|l| !l.starts_with('#'));
        let mut dims = lines.next()?.split_whitespace().map(|s| s.parse::<usize>());
        let (width, height) = (dims.next()?.ok()?, dims.next()?.ok()?);
        let mut cells = vec![Cover::Open; width * height];
        for (y, row) in lines.take(height).enumerate() {
            for (x, c) in row.chars().take(width).enumerate() {
                cells[y * width + x] = match c {
                    's' => Cover::Shade,
                    'S' => Cover::Shelter,
                    'C' => Cover::Cairn,
                    _ => Cover::Open,
                };
            }
        }
        Some(Self { width, height, cells })
    }

    pub fn get(&self, x: i32, y: i32) -> Cover {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return Cover::Open;
        }
        self.cells[y as usize * self.width + x as usize]
    }

    /// Cell centres of every `C`.
    pub fn cairns(&self) -> Vec<(f32, f32)> {
        (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cover::Cairn)
            .map(|i| ((i % self.width) as f32 + 0.5, (i / self.width) as f32 + 0.5))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_sidecar() {
        let s = parse_spawns("# comment\n2 10.5 12 1.5 4\n\n13 3 4\nbad line\n", 10000, 1);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].entry, s[0].x, s[0].wander_distance, s[0].movement_type), (2, 10.5, 4, 1));
        assert_eq!((s[1].entry, s[1].movement_type, s[1].guid), (13, 0, 2));
    }

    #[test]
    fn cover_and_markers() {
        let g = CoverGrid::parse("3 2\n.sS\nC..\n").unwrap();
        assert_eq!(
            (g.get(1, 0), g.get(2, 0), g.get(0, 1), g.get(5, 5)),
            (Cover::Shade, Cover::Shelter, Cover::Cairn, Cover::Open)
        );
        assert_eq!(g.cairns(), vec![(0.5, 1.5)]);
        let m = parse_markers("# x\nglare_gate 30 8 6\nspawn 31.5 60\n");
        assert_eq!(m[0], Marker { name: "glare_gate".into(), x: 30.0, y: 8.0, radius: 6.0 });
        assert_eq!(m[1].radius, 3.0);
    }
}
