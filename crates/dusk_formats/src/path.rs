//! Grid pathfinding over a [`WalkGrid`]: 8-directional A* (no corner cutting)
//! followed by line-of-walk smoothing. Coordinates are cell-space floats.

use crate::map::WalkGrid;
use std::cmp::Ordering;
use std::collections::{BinaryHeap, HashMap};

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f32,
    cell: (i32, i32),
}

impl Eq for Node {}

impl Ord for Node {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.total_cmp(&self.f) // min-heap
    }
}

impl PartialOrd for Node {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl WalkGrid {
    fn cell_ok(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && self.is_walkable(x as f32 + 0.5, y as f32 + 0.5)
    }

    /// True if a straight walk from `a` to `b` never enters an unwalkable cell.
    pub fn walk_line_clear(&self, a: (f32, f32), b: (f32, f32)) -> bool {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let steps = ((dx.abs().max(dy.abs())) * 4.0).ceil().max(1.0) as i32;
        (0..=steps).all(|i| {
            let t = i as f32 / steps as f32;
            // Sample slightly to both sides so diagonal moves can't squeeze between two walls.
            let (x, y) = (a.0 + dx * t, a.1 + dy * t);
            self.is_walkable(x, y)
                && self.is_walkable(x + 0.2, y)
                && self.is_walkable(x, y + 0.2)
                && self.is_walkable(x - 0.2, y)
                && self.is_walkable(x, y - 0.2)
        })
    }

    /// Waypoints from `from` (exclusive) to `to` (inclusive), or `None` if unreachable
    /// within `max_nodes` expansions. `to` may be unwalkable (e.g. a unit standing in a
    /// doorway); the path then ends at the closest reachable cell.
    pub fn find_path(&self, from: (f32, f32), to: (f32, f32), max_nodes: usize) -> Option<Vec<(f32, f32)>> {
        if self.walk_line_clear(from, to) {
            return Some(vec![to]);
        }
        let start = (from.0.floor() as i32, from.1.floor() as i32);
        let goal = (to.0.floor() as i32, to.1.floor() as i32);
        let h = |c: (i32, i32)| {
            let (dx, dy) = ((c.0 - goal.0).abs() as f32, (c.1 - goal.1).abs() as f32);
            dx.max(dy) + (std::f32::consts::SQRT_2 - 1.0) * dx.min(dy)
        };

        let mut open = BinaryHeap::new();
        let mut g: HashMap<(i32, i32), f32> = HashMap::new();
        let mut came: HashMap<(i32, i32), (i32, i32)> = HashMap::new();
        open.push(Node { f: h(start), cell: start });
        g.insert(start, 0.0);
        let mut best = (start, h(start));
        let mut expanded = 0;

        while let Some(Node { cell, .. }) = open.pop() {
            if cell == goal {
                best = (goal, 0.0);
                break;
            }
            expanded += 1;
            if expanded > max_nodes {
                break;
            }
            let gc = g[&cell];
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                let n = (cell.0 + dx, cell.1 + dy);
                let goal_cell = n == goal;
                if !goal_cell && !self.cell_ok(n.0, n.1) {
                    continue;
                }
                if dx != 0 && dy != 0 && !(self.cell_ok(cell.0 + dx, cell.1) && self.cell_ok(cell.0, cell.1 + dy)) {
                    continue; // no corner cutting
                }
                let cost = gc + if dx != 0 && dy != 0 { std::f32::consts::SQRT_2 } else { 1.0 };
                if g.get(&n).is_none_or(|&old| cost < old) {
                    g.insert(n, cost);
                    came.insert(n, cell);
                    let hn = h(n);
                    if hn < best.1 {
                        best = (n, hn);
                    }
                    open.push(Node { f: cost + hn, cell: n });
                }
            }
        }

        let (end, _) = best;
        if end == start {
            return None;
        }
        let mut cells = vec![end];
        let mut c = end;
        while let Some(&p) = came.get(&c) {
            if p == start {
                break;
            }
            cells.push(p);
            c = p;
        }
        cells.reverse();
        let mut points: Vec<(f32, f32)> = cells.iter().map(|c| (c.0 as f32 + 0.5, c.1 as f32 + 0.5)).collect();
        if end == goal {
            *points.last_mut().unwrap() = to;
        }

        // String pulling: skip waypoints while the straight walk stays clear.
        let mut smooth = Vec::new();
        let mut anchor = from;
        let mut i = 0;
        while i < points.len() {
            let mut j = points.len() - 1;
            while j > i && !self.walk_line_clear(anchor, points[j]) {
                j -= 1;
            }
            smooth.push(points[j]);
            anchor = points[j];
            i = j + 1;
        }
        Some(smooth)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::FLAG_UNWALKABLE;

    /// 10x10 open grid with a wall at x = 5 for y in 0..8.
    fn grid() -> WalkGrid {
        let size = 10;
        let mut flags = vec![0u8; 100];
        for y in 0..8 {
            flags[y * size + 5] = FLAG_UNWALKABLE;
        }
        WalkGrid { size: size as u32, flags, floor: vec![true; 100] }
    }

    #[test]
    fn straight_when_clear() {
        assert_eq!(grid().find_path((1.5, 9.5), (8.5, 9.5), 1000), Some(vec![(8.5, 9.5)]));
    }

    #[test]
    fn goes_around_wall() {
        let g = grid();
        let path = g.find_path((2.5, 2.5), (8.5, 2.5), 1000).unwrap();
        assert_eq!(*path.last().unwrap(), (8.5, 2.5));
        // Every leg must be walkable and the path must dip below the wall (y >= 8).
        let mut a = (2.5, 2.5);
        for &p in &path {
            assert!(g.walk_line_clear(a, p), "leg {a:?} -> {p:?} crosses a wall");
            a = p;
        }
        assert!(path.iter().any(|p| p.1 >= 8.0));
    }

    #[test]
    fn unreachable_returns_closest() {
        let mut g = grid();
        for y in 0..10 {
            g.flags[y * 10 + 5] = FLAG_UNWALKABLE; // full wall
        }
        let path = g.find_path((2.5, 2.5), (8.5, 2.5), 1000).unwrap();
        assert!(path.last().unwrap().0 < 5.0);
    }
}
