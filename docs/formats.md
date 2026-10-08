# Legacy data formats

Original client: 32-bit C++ / SFML 2 (legacy client), online-only, TCP to
port 16383. Server is not shipped.

Status legend: ✅ parsed & verified on all files · 🟡 partially understood · ❌ unknown

## Install layout

| Path | Contents | Status |
|---|---|---|
| `game.db` | SQLite 3, ~60 tables: items, spells, npcs, spawns, loot, quests, gossip, class stats | ✅ (schema self-describing) |
| `content/**/*.zip` | Plain (unencrypted) zips of PNG / OGG / TTF | ✅ |
| `maps/*.map` | Binary isometric tile maps | ✅ (a few flag bits 🟡) |
| `scripts/npc/*.txt`, `scripts/player/{male,female}/*.txt` | Sprite-sheet animation scripts | ✅ |
| `scripts/animation/*.sa` | Spell/effect flipbooks | 🟡 (`ratio` meaning) |
| `scripts/buttons/*.bscript` | UI button definitions | ❌ |
| `scripts/particles/*.psi` | Particle systems (HGE `hgeParticleSystemInfo`) | ✅ |
| `scripts/shaders/` | GLSL for SFML | 🟡 (not used for lighting) |
| `config.ini` | Window/net/keybind settings (SFML key codes) | ✅ |

Textures are resolved by **bare, case-insensitive filename** across all zips;
`dusk_extract` writes `assets/file_index.txt` to reproduce that.

## `.map`

All little-endian. Source of truth: `GameMap_loadFromDisk` (0x55b700) and
`ClientMap_saveToDisk` (0x4b4c20) in the legacy client (names applied in the Ghidra project).

```
u32 size                       // size x size cells
u32 texture_count
cstr textures[texture_count]   // NUL-terminated
u32 cell_count                 // sparse list
cell:
  u32 index                    // y * size + x
  u8  flags
  layer[3]:
    u8 present (0/1)
    if present: u32 texture_index, u32 param
u32 terrain_texture_count
cstr terrain_textures[..]
if terrain_texture_count > 0:
  u32 n; n * (u32 terrain_id, u32 terrain_texture_index)
u32 n; n * (u32 terrain_id, u32 zone_id)     // zone_template.id
u32 n; n * (u32 area_id, u32 terrain_id)     // area_template.id
```

Trailing sections may be absent in older maps; the loader treats them as empty. ✅ all 28 maps.

- Layer 0: ground tiles, layer 1: flat decals, layer 2: upright sprites (depth sorted)
- `param`: 0 in most maps, looks like f32 0.6–1.0 in fanadin 🟡
- Flags (from the map editor overlay code in `ClientMap_buildDrawList`):
  - `0x20` → editor shows `mapeditor_unwalkable_tile.png` → **blocks movement** ✅
  - `0x40` → editor shows `mapeditor_block_tile.png` → probably blocks LOS/missiles 🟡
  - fanadin also uses `0x10`, `0x08`, `0x04` ❌
- **Terrain**: `getTerrainWidth() = size / 13`; chunk `id = row * w + col` covers 13x13 cells.
  Each chunk draws one texture (set repeated, origin = texture centre) at
  `((col - row) * 416, (col + row) * 208)` + map origin, i.e. the render position of cell
  `(13 col, 13 row)`. Drawn beneath all cell layers.

Sprite pivot: `sprite_hotspot` table (filename → x,y), else
`(w/2, h-16)` (centre of the base diamond). Projection:
`screen = ((x - y) * 32, (x + y) * 16)`, cell centre at `(x+0.5, y+0.5)`.
NPC spawn coords in `game.db` use the same cell units.

## Sprite scripts (`.txt`)

```
image=npc_goblin.png
[stance]                 // stance shoot hit die critdie cast swing run block spawn cast_alt
frames=4
duration=800ms           // also "1s"
type=back_forth          // looped | play_once | back_forth
frame=F,D,x,y,w,h,px,py  // frame, direction 0..7, rect in sheet, pivot (may be negative)
```

Direction order: 0=W 1=NW 2=N 3=NE 4=E 5=SE 6=S 7=SW (screen). ✅
`ClientUnit::computeDirection` (0x5548f0): orientation θ = `atan2(dy, dx)` in **cell**
space, wrapped to [0, 2π) (same unit as `npc.orientation`);
`dir = [5,6,7,0,1,2,3,4][floor(((θ + π/8) mod 2π) / (π/4))]`.
Players are paper-dolls: one script per gear piece (`default_*`, `head_*`,
armour, weapons), all sharing the same frame layout.

## `.sa`

```
ratio=4
size=192
filename=cast_001        // frames are <filename>_<n>.png
loopstart=0
loopend=0
delay=50                 // ms/frame
1,39,43                  // frame n, x, y offset of trimmed frame in size x size canvas
```

## `.psi` particle systems

128 bytes, little-endian: exactly HGE's `hgeParticleSystemInfo` as written by the HGE
particle editor. The client does **not** use HGE; `ParticleSystem.cpp` re-implements it on
an SFML vertex array. Parser + simulation: `dusk_formats::psi` (✅ all 43 files).
Ghidra names: `ParticleSystemInfo_loadFromFile` 0x4e0b90, `ParticleSystem_ctor` 0x4f51a0,
`ParticleSystem_update` 0x4f5880, `ParticleSystem_spawnParticle` 0x4f5ee0,
`ParticleSystem_draw` 0x4f5e70, `ParticleSystem_setPosition` 0x4f5700,
`ParticleSystem_move` 0x4f54d0, `randomFloat` 0x4f5000 (mt19937 uniform),
`ContentMgr_loadPsi` 0x43e170, `ContentMgr_spawnParticleSystem` 0x440610.

```
off  type   field
  0  u32    sprite: bits 0-1 column, bits 2-15 row of a 32x32 cell in particles.png (128x128);
            bits 16+ HGE blend: 4 = additive, 6 = alpha. Client: additive = (v & 0xffff0000) != 0x60000
  4  i32    emission (particles/s)
  8  f32    lifetime (system; <= 0 = forever, all files use -1)
 12  f32x2  particle life min, max (s)
 20  f32    direction (rad, 0 = screen up, clockwise)   24 f32 spread (rad, full cone)
 28  u32    relative (add the emitter's movement angle)
 32  f32x2  speed min/max (px/s)       40 f32x2 gravity min/max (px/s^2, +y = down)
 48  f32x2  radial accel min/max       56 f32x2 tangential accel min/max
 64  f32x3  size start/end/var (x 32 px)   76 f32x3 spin start/end/var (never rendered)
 88  f32x4  colour start RGBA   104 f32x4 colour end   120 f32 colour var   124 f32 alpha var
```

Rules (✅ verified from the decompiled update/spawn code):
- `update(dt)`: `age += dt`; `n = floor(emission*dt + residue)`, residue keeps the fraction;
  unless stopped, spawn `n` particles while fewer than **500** are alive. Then for every
  particle (new ones included): `age += dt`, die at `terminal_age`; `r` = unit vector
  emitter→particle (0 at the emitter); `vel += (r*radial + perp(r)*tangential)*dt` with
  `perp(r) = (-r.y, r.x)`; `vel.y += gravity*dt`; `pos += vel*dt`; size, spin, colour += delta*dt.
- spawn: `terminal_age = rnd(life_min, life_max)`; `pos = emitter + rnd(-2,2)` per axis (no
  prev/current interpolation, unlike HGE); `angle = direction - pi/2 + rnd(0, spread) - spread/2`
  (+ `atan2(prev - cur) + pi/2` when relative); `vel = (cos, sin)(angle) * rnd(speed)`;
  gravity/radial/tangential = `rnd(min, max)`; `size = rnd(start, start + (end-start)*size_var)`,
  `size_delta = (end - size)/terminal_age`; same for spin; RGB = `rnd(start, start+(end-start)*color_var)`,
  A with `alpha_var`; colour delta towards the end colour over the particle's life.
- render: one quad per particle, side `size*32`, centred on it, no rotation; vertex colour
  `(u8)(int)(c*255)`; texture `particles.png`; `sf::BlendAdd` when additive, else default alpha.
  Positions are absolute (the drawable's transform is not applied).
- `setPosition(moveParticles, x, y)`: with `moveParticles` live particles are translated
  with the emitter (attached effect), otherwise they stay (trail).

Who spawns them:
- **Map sprites** (`Sprite::renderScript` 0x50e8c0): `sprite_psi` rows of the sprite's
  texture name; emitter at `sprite pos - hotspot + (x_offset, y_offset)` (image top-left
  relative), updated and drawn right after the sprite (`setPosition(true)`), only while the
  sprite is drawn. Map texture entries named `*.psi` are invisible sprites (hotspot (1, 1))
  that only carry `sprite_psi`/`sprite_light` rows. Note: `renderScript` computes a missing
  hotspot as `(w/2, h/1.25)`, not `(w/2, h-16)` 🟡 (equal for 80 px tall images).
- **Spell visual kits** (`WorldSpellAnimation` 0x54a7a0, kit ctor 0x4fdf10): emitter at
  the animation position + (`psystem_x`, `psystem_y`), expressions may use `height`.
  Casting kit: `setPosition(false)`, stopped at cast end (0x54c750). Traveling kit:
  `setPosition(true)` on the projectile. Impact/go kit: created stopped (0x54bdb0), never
  emits (no impact kit has a psystem anyway). A kit is finished when its system is stopped,
  empty and `int(age) > 1` (0x500d00). Aura kits (`aura_kit_ontop`) use psystems too
  (aura code not traced; we attach them to the unit while the aura is up 🟡).

## Lights and darkness

`sprite_light(filename, color 0xRRGGBBAA, x_offset, y_offset, intensity, bool_applyground,
bool_applytop, scale)` (✅ `ClientMap_buildDrawList` 0x4b0850, light collection 0x4b8640):
- light position = cell render position (sprite position - (0, 16)) + offset; `intensity`
  is stored /100 but unused by the drawing code.
- `bool_applytop`: `light_source.png` (422x193, centred, `setColor(color)`, scale, `BlendAdd`)
  drawn in the cell's depth-sorted slot just before the sprite.
- `bool_applyground`: same glow at +(16, 8), drawn after the whole upright layer.
- Darkness: map brightness `b` eases to its target (`b += dt*(target-b)`); the target is
  `1 - zone_template.night_pct` of the player's zone (0x556be0). While `b < 1`, a
  window-sized render texture is cleared to `(0,0,0,(1-b)*255)`, each light draws
  `shader_light.png` (1024x512, alpha ~0.04 centre .. 1 edge) at +(16, 8), scaled by the
  light's scale, colour (25,25,25), `BlendMultiply`, and the texture is drawn over the map.
  Units also add cut-outs (scale 0.3, via a virtual on world objects; not traced, not done).
- No day/night cycle: darkness is per zone. `clouds.png` (3600x3200, repeated) is drawn
  after the decal layer when a map flag is set (not done). `brightcontrast.frag` is a
  brightness/contrast/saturation filter (options), `unitbright`/`unitrecolor` tint units.
- Spell kits: `unit_glow_color` tints the unit, alpha ramping up over the first half of the
  kit's duration and down over the second (0x500860); `ground_glow_color` not traced. Neither done.

Client implementation (`dusk_client::particles`, `lights`, `spell_particles`): CPU simulation
with the rules above, one mesh per emitter with a custom `Material2d` (SFML blend modes),
emitters off-screen are not simulated (like the original); darkness is a camera quad whose
shader multiplies the `shader_light.png` cut-outs of up to 64 lights. Blending happens in
linear space (SFML: sRGB bytes), so additive effects come out somewhat brighter.
