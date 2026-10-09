//! `maps/*.map` — isometric tile maps.
//!
//! ```text
//! u32 size                     // map is size x size cells
//! u32 texture_count
//! cstr textures[texture_count] // NUL-terminated filenames, resolved via FileIndex
//! u32 cell_count               // sparse: only non-empty cells stored
//! cell[cell_count]:
//!   u32 index                  // y * size + x
//!   u8  flags                  // FLAG_UNWALKABLE / FLAG_BLOCK (see consts)
//!   layer[3]:
//!     u8 present
//!     if present: u32 texture, u32 param  // param: unused (0)
//! u32 terrain_texture_count
//! cstr terrain_textures[..]
//! if terrain_texture_count > 0:
//!   u32 n; n * (u32 terrain_id, u32 terrain_texture)
//! u32 n; n * (u32 terrain_id, u32 zone_id)      // zone ids (parsed, unused)
//! u32 n; n * (u32 area_id,    u32 terrain_id)   // area ids (parsed, unused)
//! ```
//!
//! Terrain chunks are `TERRAIN_CHUNK` x `TERRAIN_CHUNK` cells on a grid of
//! `size / TERRAIN_CHUNK` (integer division) per side; `terrain_id = row * w + col`.
//! Each chunk draws one repeated texture centred (origin = texture centre) on the
//! render position of cell `(col * 13, row * 13)`.

use thiserror::Error;

pub const LAYERS: usize = 3;
/// Cells per terrain chunk side (a map has `size / 13` chunks per side).
pub const TERRAIN_CHUNK: u32 = 13;
/// The cell cannot be walked on.
pub const FLAG_UNWALKABLE: u8 = 0x20;
/// The cell blocks (set together with unwalkable on walls).
pub const FLAG_BLOCK: u8 = 0x40;

#[derive(Debug, Error)]
pub enum MapError {
    #[error("unexpected end of file at offset {0}")]
    Eof(usize),
    #[error("texture index {index} out of range ({count}) in cell {cell}")]
    BadTexture { cell: u32, index: u32, count: usize },
    #[error("bad layer marker {0} at offset {1}")]
    BadLayerMarker(u8, usize),
    #[error("terrain texture index {0} out of range")]
    BadTerrainTexture(u32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TileLayer {
    pub texture: u32,
    /// Per-layer value, unused (our generators write 0).
    pub param: u32,
}

#[derive(Debug, Clone)]
pub struct Cell {
    pub x: u32,
    pub y: u32,
    pub flags: u8,
    pub layers: [Option<TileLayer>; LAYERS],
}

#[derive(Debug, Clone)]
pub struct MapFile {
    pub size: u32,
    pub textures: Vec<String>,
    pub cells: Vec<Cell>,
    pub terrain_textures: Vec<String>,
    /// (terrain_id, index into `terrain_textures`)
    pub terrain: Vec<(u32, u32)>,
    /// (terrain_id, zone id)
    pub zones: Vec<(u32, u32)>,
    /// (area id, terrain_id)
    pub areas: Vec<(u32, u32)>,
}

/// Per-cell flags for collision queries (shared by client prediction and server).
#[derive(Debug, Clone, Default)]
pub struct WalkGrid {
    pub size: u32,
    /// Row-major `size * size`; 0 for cells absent from the file.
    pub flags: Vec<u8>,
    /// Row-major: cell has a ground tile or a terrain chunk under it.
    pub floor: Vec<bool>,
}

impl WalkGrid {
    pub fn flags_at(&self, x: f32, y: f32) -> Option<u8> {
        if x < 0.0 || y < 0.0 || x >= self.size as f32 || y >= self.size as f32 {
            return None;
        }
        Some(self.flags[(y as u32 * self.size + x as u32) as usize])
    }

    /// Inside the map and not flagged unwalkable.
    pub fn is_walkable(&self, x: f32, y: f32) -> bool {
        self.flags_at(x, y).is_some_and(|f| f & FLAG_UNWALKABLE == 0)
    }

    /// Centre of the walkable floor cell nearest to `near`.
    pub fn nearest_floor(&self, near: (f32, f32)) -> Option<(f32, f32)> {
        (0..self.size * self.size)
            .filter(|&i| self.floor[i as usize] && self.flags[i as usize] & FLAG_UNWALKABLE == 0)
            .map(|i| ((i % self.size) as f32 + 0.5, (i / self.size) as f32 + 0.5))
            .min_by(|a, b| {
                let d = |p: &(f32, f32)| (p.0 - near.0).powi(2) + (p.1 - near.1).powi(2);
                d(a).total_cmp(&d(b))
            })
    }
}

impl MapFile {
    pub fn walk_grid(&self) -> WalkGrid {
        let n = (self.size * self.size) as usize;
        let (mut flags, mut floor) = (vec![0; n], vec![false; n]);
        let tw = self.terrain_width();
        for &(id, _) in &self.terrain {
            let (col, row) = (id % tw.max(1), id / tw.max(1));
            for y in row * TERRAIN_CHUNK..((row + 1) * TERRAIN_CHUNK).min(self.size) {
                for x in col * TERRAIN_CHUNK..((col + 1) * TERRAIN_CHUNK).min(self.size) {
                    floor[(y * self.size + x) as usize] = true;
                }
            }
        }
        for c in &self.cells {
            let i = (c.y * self.size + c.x) as usize;
            flags[i] = c.flags;
            floor[i] |= c.layers[0].is_some();
        }
        WalkGrid { size: self.size, flags, floor }
    }

    /// Terrain chunks per side.
    pub fn terrain_width(&self) -> u32 {
        self.size / TERRAIN_CHUNK
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], MapError> {
        let s = self.data.get(self.pos..self.pos + n).ok_or(MapError::Eof(self.pos))?;
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, MapError> {
        Ok(self.take(1)?[0])
    }
    fn u32(&mut self) -> Result<u32, MapError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn cstr(&mut self) -> Result<String, MapError> {
        let rest = &self.data[self.pos..];
        let len = rest.iter().position(|&b| b == 0).ok_or(MapError::Eof(self.pos))?;
        // Filenames are ASCII in practice; latin1 keeps any odd byte lossless.
        let s = rest[..len].iter().map(|&b| b as char).collect();
        self.pos += len + 1;
        Ok(s)
    }
}

impl MapFile {
    pub fn parse(data: &[u8]) -> Result<Self, MapError> {
        let mut r = Reader { data, pos: 0 };
        let size = r.u32()?;
        let tex_count = r.u32()? as usize;
        let textures = (0..tex_count).map(|_| r.cstr()).collect::<Result<Vec<_>, _>>()?;

        let cell_count = r.u32()? as usize;
        let mut cells = Vec::with_capacity(cell_count);
        for _ in 0..cell_count {
            let index = r.u32()?;
            let flags = r.u8()?;
            let mut layers = [None; LAYERS];
            for layer in &mut layers {
                match r.u8()? {
                    0 => {}
                    1 => {
                        let texture = r.u32()?;
                        if texture as usize >= tex_count {
                            return Err(MapError::BadTexture { cell: index, index: texture, count: tex_count });
                        }
                        *layer = Some(TileLayer { texture, param: r.u32()? });
                    }
                    m => return Err(MapError::BadLayerMarker(m, r.pos - 1)),
                }
            }
            cells.push(Cell { x: index % size, y: index / size, flags, layers });
        }

        let terrain_tex_count = r.u32()? as usize;
        let terrain_textures = (0..terrain_tex_count).map(|_| r.cstr()).collect::<Result<Vec<_>, _>>()?;
        // Maps may stop early; missing sections read as empty.
        let pairs = |r: &mut Reader| -> Result<Vec<(u32, u32)>, MapError> {
            if r.pos == r.data.len() {
                return Ok(Vec::new());
            }
            let n = r.u32()?;
            (0..n).map(|_| Ok((r.u32()?, r.u32()?))).collect()
        };
        let terrain = if terrain_tex_count > 0 { pairs(&mut r)? } else { Vec::new() };
        if let Some(&(_, t)) = terrain.iter().find(|(_, t)| *t as usize >= terrain_tex_count) {
            return Err(MapError::BadTerrainTexture(t));
        }
        let zones = pairs(&mut r)?;
        let areas = pairs(&mut r)?;

        Ok(Self { size, textures, cells, terrain_textures, terrain, zones, areas })
    }

    pub fn load(path: impl AsRef<std::path::Path>) -> anyhow::Result<Self> {
        Ok(Self::parse(&std::fs::read(path)?)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_map() {
        let mut d = Vec::new();
        d.extend(4u32.to_le_bytes());
        d.extend(1u32.to_le_bytes());
        d.extend(b"a.png\0");
        d.extend(1u32.to_le_bytes());
        d.extend(6u32.to_le_bytes()); // x=2,y=1
        d.push(0x60);
        d.push(1);
        d.extend(0u32.to_le_bytes());
        d.extend(7u32.to_le_bytes());
        d.push(0);
        d.push(0);
        d.extend(1u32.to_le_bytes()); // terrain textures
        d.extend(b"g.png\0");
        for pairs in [[(0u32, 0u32)], [(0, 51)], [(3, 0)]] {
            d.extend((pairs.len() as u32).to_le_bytes());
            for (a, b) in pairs {
                d.extend(a.to_le_bytes());
                d.extend(b.to_le_bytes());
            }
        }
        let m = MapFile::parse(&d).unwrap();
        assert_eq!(m.textures, ["a.png"]);
        assert_eq!((m.cells[0].x, m.cells[0].y, m.cells[0].flags), (2, 1, 0x60));
        assert_eq!(m.cells[0].layers[0], Some(TileLayer { texture: 0, param: 7 }));
        assert_eq!(m.terrain_textures, ["g.png"]);
        assert_eq!(
            (m.terrain.as_slice(), m.zones.as_slice(), m.areas.as_slice()),
            (&[(0, 0)][..], &[(0, 51)][..], &[(3, 0)][..])
        );
    }
}
