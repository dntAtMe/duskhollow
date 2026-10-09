# Content: files, formats, and how to add things

Everything the game reads lives under one asset root, `assets/` (override with `DUSK_ASSETS`;
`dusk_formats::assets_root`). Client and server load it through `dusk_formats::content`, one
loader per kind of data. All of it is generated or hand-written in this repository: art by
`tools/artgen`, sounds by `tools/sfxgen`, data by hand.

```
assets/
  data/      game data, text: classes exp spells class_spells npc_templates items item_bases
             affixes loot maps spell_visuals particles sprite_sounds help (.txt)
  maps/      <name>.map + sidecars <name>.spawns .cover .markers
  scripts/   npc/<model>.txt  player/<model>.txt (sprite scripts)  animation/<name>.sa (flipbooks)
  content/   everything resolved by bare file name:
             sprites/ (unit + gear sheets)  env/ vale/ (map tiles and props + their metadata)
             ui/  icons/items/  icons/spells/  portraits/  spellfx/ (flipbook frames)
             fx/ (particle atlas, light textures)  sfx/  music/  fonts/
  env_manifest.json   written by tools/artgen/enviro.py, read by mapgen.py
  preview/   contact sheets and overviews (gitignored)
```

**Bare names.** Data files, maps and scripts name images, sounds and fonts by file name only
(`cv_cairn_0.png`, `spell_dash`). At start-up `FileIndex::scan` indexes every file under
`content/` by its lowercase name, so a file can sit in any subfolder, but **a name must be unique
under `content/`** (case-insensitive; a duplicate is a start-up error and fails
`cargo test -p dusk_formats`). Text files (`.txt`, `.md`) are not indexed: `sprite_fx.txt`,
`hotspots.txt` and `roofs.txt` exist once per art folder and are read by walking `content/`.

**Checking.** `cargo test -p dusk_formats` validates everything: every data file parses with no
unknown keys, and `tests/content.rs` resolves every reference (map textures, spawns, NPC and
item models, their sheets, portraits, icons, spell kits with their flipbooks, frames, particles
and sounds, sprite effects, sprite sounds, fonts, music, help text). Run it after any content
change; the failure message lists every broken reference.

## ID ranges

| What | Range | Where |
|---|---|---|
| Maps | 10000.. (10000 duskhollow, 10001 glade) | `data/maps.txt` |
| Classes | 1 Vanguard, 2 Emberwright, 3 Cutthroat, 4 Ashpriest | `data/classes.txt` |
| NPC templates (and their model ids) | 50000.. (50001-50004 vale enemies, 50010-50012 Lowshade folk, 50020-50023 glade) | `data/npc_templates.txt` |
| Player skills | 50001..50099 | `data/spells.txt` |
| Auto attacks | 50100 Attack, 50101 Shoot | `data/spells.txt` |
| Item spells | 50110.. | `data/spells.txt` |
| NPC spells | 51001.. | `data/spells.txt` |
| Items | 1-99 consumables / quest items, 10-19 starter gear, 100-199 junk, 1000-9999 named items | `data/items.txt` |
| Generated gear | `100000 + base·1000 + quality·100 + level` | `data/item_bases.txt` |
| Affixes | theme·10 + level band | `data/affixes.txt` |
| Loot tables | 1.. | `data/loot.txt` |

Ids only need to be unique within their file; the ranges keep them readable.

## The data format (`data/*.txt`)

One format for all data files (`dusk_formats::content::sections`):

```text
# comment (whole line)
[50001]               # a section: plain id ...
name=Glarewolf
junk=100,101,102      # lists are comma-separated
[spell 50002]         # ... or kind + id
impact=bleed_hit
spell=12              # some keys repeat (start_item=, spell=, stat=, item=); order is kept
```

Keys outside a section, a repeated `[kind id]` and unknown keys are errors. Every data file
starts with a comment header listing its keys; the tables below are the reference.

### `classes.txt` — `[class]`

| Key | Meaning |
|---|---|
| `name`, `role` | display name, one-line tagline (menu) |
| `hp`, `mana`, `strength`, `agility`, `willpower`, `intelligence`, `courage` | formulas over `clvl` (the level), e.g. `hp=75*clvl*(59+clvl)/60` |
| `armor` | body armour families `cloth leather mail plate robe` or armour type numbers (basic cloth is always allowed) |
| `weapons` | `axe bow mace sword staff dagger wand shield` |
| `stats` | stat keys this class's random loot favours |
| `start_item` | `<item>,<count>`, repeated; gear is put on, the rest goes to the bags |

### `exp.txt` — `[level]`

`exp` (experience to advance from this level), `kill_exp` (base experience for killing an enemy
of this level; ±10 % per level of difference, nothing 5+ levels below), `title` (rank). The last
level is the cap.

### `class_spells.txt` — `[class]`

`spell=<entry>`, repeated, in action-bar order. Every class has 50100 and 50101.

### `spells.txt` — `[entry]`

| Key | Meaning |
|---|---|
| `name`, `icon` | display name, icon file (`skill_<name>.png`, `content/icons/spells/`) |
| `description`, `aura_description` | tooltip and aura tooltip; tokens `$E1min $E1max` (effect 1 value range), `$E2D3` (raw data3 of effect 2), `$DUR`, `$INVL` |
| `mana`, `mana_pct` | mana cost formula; or percent of max mana |
| `cast_time`, `cooldown`, `duration`, `interval` | milliseconds (`interval`: aura tick) |
| `duration_formula` | formula for the duration instead of `duration` |
| `range` | 64 per cell (130 = melee) |
| `speed` | projectile speed (16 ≈ 12 cells/s), 0 = instant |
| `school` | 1 physical, 2 frost, 3 fire, 4 shadow, 5 holy (default 1) |
| `attributes` | reserved bit flags (0) |
| `abilities_tab` | 1 Spells (default), 0 Actions |
| `effectN` (N = 1..3) | `school_damage weapon_damage apply_aura heal heal_pct restore_mana_pct melee_atk ranged_atk charge` (or a number, [combat.md](combat.md)) |
| `effectN_data` | `d1,d2,d3`: damage/heal: d2 = base `value`; weapon damage: d2 = weapon %; auras: d1 = aura type, d2 = mechanic / stat, d3 = value |
| `effectN_target` | `caster friendly area_src_friendly hostile area_src_hostile area_dst_hostile any` |
| `effectN_radius` | cells, for area targets |
| `effectN_positive` | 1 = a buff (friendly aura) |
| `effectN_formula` | value formula over `value clvl splvl STR AGI WIL INT CUR` |

Periodic auras: the formula is the total over `duration` unless the description says "every".
Implemented effects, auras and mechanics: [combat.md](combat.md).

### `spell_visuals.txt` — `[kit <name>]` and `[spell <entry>]`

Kits: `anim anim_x anim_y anim_color anim_blend`, `anim2*` (a second flipbook), `particles
particles_x particles_y`, `sound`, `unit_glow ground_glow`. Spells: `casting traveling impact go
aura` (kit names) and `go_anim cast_anim` (`swing cast shoot cast_alt block hit`). Every spell
of `spells.txt` needs a `[spell N]` section. Full reference and the shipped kits:
[visuals.md](visuals.md).

### `particles.txt` — `[name]`

Particle systems for map emitters and kits: `sprite blend emission lifetime life direction spread
relative speed gravity radial tangential size spin color_start color_end color_var alpha_var`.
Reference: [visuals.md](visuals.md) and the header of `dusk_formats::content::particles`.

### `npc_templates.txt` — `[entry]`

Each template brings its own model (`model id = entry`).

| Key | Meaning |
|---|---|
| `name`, `subname` | name plate text |
| `model` | sprite script `scripts/npc/<model>.txt` (also names voices `npc_<model>_*` and the portrait `portrait_custom_<model>.png`) |
| `height` | visual height in px (name plate, floating text) |
| `model_scale` | percent (default 100) |
| `portrait` | `portrait_<portrait>.png` (target frame); empty = `portrait_custom_<model>.png`, else a faction placeholder |
| `level` | `min-max` or one level, rolled per spawn |
| `faction` | 1 friendly, 2 neutral (only fights back), 3 hostile (default) |
| `health`, `weapon_value` | -1 / unset = from the level (`20 + 30·lvl`, `3 + 2·lvl`; ×elite, ×boss) |
| `mana`, `armor`, `melee_speed_ms` (default 2000), `leash_range` (cells, 0 = 20) | |
| `ai_type` | 0 melee, 1 caster, 2 archer |
| `strength agility intellect willpower courage` | attributes |
| `resist` | `frost,fire,shadow,holy` ratings |
| `elite`, `boss` | 1 = elite / boss (frame ring, health and loot multipliers) |
| `spellN` (N = 1..4) | `spell,chance %,interval_ms,cooldown_ms,target_type` cast in combat |
| `loot` | loot table of `loot.txt` |
| `loot_chances` | `green,blue,gold,purple` percent (-1 = default) |
| `gold_ratio` | percent of the default gold (-1 = default) |
| `junk` | junk item entries (`items.txt` 100-199) |
| `npc_flags` | reserved |

### `items.txt` — `[entry]`

| Key | Meaning |
|---|---|
| `name`, `icon`, `description` | display; icon in `content/icons/items/` |
| `model` | paper-doll layer `scripts/player/<model>.txt` (equippable items) |
| `equip_type` | `head neck chest belt legs feet hands ring weapon shield ranged` |
| `weapon_type` / `armor_type` | `axe bow mace sword staff dagger wand` / 1 cloth, 2-4 leather, 5-8 mail, 9-11 plate, 12-15 robe (shields 4-8) |
| `material` | weapon tier: 1-7 metals, 8-14 woods |
| `quality` | `junk common green blue gold purple` (1..6) |
| `required_level`, `item_level` | equip level; junk level (matched to the killed NPC) |
| `stack` | max stack size |
| `spell` | on-use spell (repeatable) |
| `stat` | `<key>:<amount>` (repeatable), keys: `mana health armor strength agility willpower intelligence courage regeneration meditate weapon_value melee_speed ranged_weapon_value ranged_speed melee_crit ranged_crit spell_crit dodge block resist_frost resist_fire resist_shadow resist_holy` |
| `sell_price`, `flags` | gold; `no_save no_trade no_arena no_group_dungeon quest_item skillbook gold_value_scales` |

Weapon value, armour and block of equippable items come from formulas ([items.md](items.md)).

### `item_bases.txt` — `[base N]`

`name model icon` (one value, or five: one per quality 2..6), `equip_type`, `weapon_type` or
`armor_type`, `material`, `levels=lo-hi`. Every base becomes one item per quality and level of
its band. Random drops pick from these.

### `affixes.txt` — `[N]`

`name` ("Noun" or "Prefix Noun"), `noun=1` ("of the"), `level=lo-hi`, `stat=<key>:<factor>`.

### `loot.txt` — `[loot N]`

`item=<entry>,<chance %>,<min>-<max>`, repeated; every row rolls on its own.

### `maps.txt` — `[<map name>]`

`id`, `title`, `default=1` (exactly one map: where new characters start), `music` (tracks;
empty = the whole soundtrack), `ambience` (loop; empty = none), `darkness` (0 full light .. 1
black). The start point is the map's `arrival` marker.

### `sprite_sounds.txt`

`<sprite> <sound> <radius>` per line: map sprites whose file stem matches (`*` suffix = prefix)
play a loop, fading out at `radius` cells. Every `C` cell of a `.cover` also plays
`loop_cairn_fire`.

### `help.txt`

The chat `/help` text, one chat line per line.

## Maps

### `.map` (binary, little-endian)

```
u32 size                       // size x size cells
u32 texture_count
cstr textures[texture_count]   // NUL-terminated bare file names (content/)
u32 cell_count                 // sparse: only listed cells exist
cell[cell_count]:
  u32 index                    // y * size + x
  u8  flags                    // 0x20 not walkable, 0x40 blocks (walls)
  layer[3]:                    // 0 ground tile, 1 flat decal, 2 upright (depth sorted)
    u8 present (0/1)
    if present: u32 texture_index, u32 param (0)
u32 terrain_texture_count      // optional repeated ground under all cells (our maps: 0)
cstr terrain_textures[..]
if terrain_texture_count > 0:
  u32 n; n * (u32 chunk_id, u32 terrain_texture_index)   // chunks of 13x13 cells, id = row * (size/13) + col
u32 n; n * (u32, u32)          // zone pairs: parsed, unused (write 0)
u32 n; n * (u32, u32)          // area pairs: parsed, unused (write 0)
```

Trailing sections may be missing (read as empty). Parser: `dusk_formats::map`; writers:
`write_map` in `tools/artgen/mapgen.py` and `valemap.py`.

- Projection: cell (x, y) renders at `((x - y) * 32, (x + y) * 16)` px; ground tiles are 64x32
  diamonds. Unit and spawn positions are fractional cells; orientation is radians of
  `atan2(dy, dx)` in cell space.
- Upright pivot: `hotspots.txt` next to the art (`<file> <x> <y>`, px from the image's top-left),
  else `(w / 2, h / 1.25)`.
- A texture named `<anything>.psi` is an invisible sprite that only carries `sprite_fx.txt`
  effects (fireflies over a pond).
- Walking only checks the `0x20` flag: a cell without ground (or not listed at all) is still
  walkable unless flagged, so flag every void cell (rock, chasms, everything outside a dungeon).

### Sidecars

| File | Format | Used for |
|---|---|---|
| `<name>.spawns` | `entry x y orientation wander_distance` per line (cells, radians); `#` comments | server NPC spawns (respawn 60 s; wander > 0 = roams that far) |
| `<name>.markers` | `name x y [radius]` per line (cells; radius default 3) | `arrival` (start / respawn point, required for the default map); script landmarks (`glare_gate`, `lowshade` ...) |
| `<name>.cover` | first line `W H`, then H rows of W chars: `.` open sky, `s` shade, `S` deep shelter, `C` rest cairn | the Eye's gaze (strain, shade tint, ambience). **No `.cover` = no gaze on that map** (indoors, underground) |

### Art metadata (next to the art, in `content/env/`, `content/vale/` ...)

| File | Line format |
|---|---|
| `hotspots.txt` | `<sprite.png> <x> <y>`: pivot of an upright sprite |
| `roofs.txt` | `roof <sprite prefix> <dx> <dy>`: the sprite (placed on its back cell) roofs cells up to +dx,+dy; drawn above units there and faded while the player stands beneath |
| `sprite_fx.txt` | `particles <sprite> <system> <x> <y>` (offset from the image's top-left) and `light <sprite> <rrggbbaa> <x> <y> <ground 0/1> <top 0/1> <scale>` (offset from the cell), see [visuals.md](visuals.md) |

## Sprite scripts (`scripts/npc/*.txt`, `scripts/player/*.txt`)

```
image=custom_npc_glarewolf.png     # sheet in content/sprites/
[stance]                           # animation name
frames=4
duration=1000ms                    # whole animation; also "1s"
type=looped                        # looped | play_once | back_forth
hit=350                            # optional: ms into the animation where the blow lands
frame=F,D,x,y,w,h,px,py            # frame F, direction D, rect in the sheet, pivot (feet) in the rect
```

Directions: 0=W 1=NW 2=N 3=NE 4=E 5=SE 6=S 7=SW (screen). Animations the client plays:
`stance`, `run`, `swing` / `swing2` / `swing3` (attacks, picked at random), `cast`, `cast_alt`,
`shoot`, `hit`, `block`, `die`; a missing one falls back to `stance`. `<model>_smear.txt` is an
optional weapon-smear layer drawn over attacks. Players are a paper doll: the body
`custom_body.txt` plus one layer per equipped item model, all on the same frame layout.
Written by `tools/artgen/sheet.py` (used by every character generator).

## Flipbooks (`scripts/animation/*.sa`)

```
ratio=4                  # area ratio of the frames to world pixels (drawn at 1/sqrt(ratio))
size=192                 # canvas size; frames are trimmed
filename=sfx_cast_001    # frames are content/spellfx/<filename>_<n>.png
loopstart=0
loopend=0
delay=50                 # ms per frame
1,39,43                  # frame n, offset of the trimmed frame in the canvas (one line per frame)
```

Written by `tools/artgen/spellfx.py` from `spellfx_layout.json` (per-flipbook canvas, ratio,
delay, frame boxes). Frames are drawn on black and luma-keyed (an additive look).

## Sounds and music

- Effects: `content/sfx/<name>.wav` (16-bit PCM mono 44.1 kHz) from `tools/sfxgen`; named by bare
  name, `.wav` implied (`spell_dash`). Event sounds are the constants of
  `dusk_formats::sound::builtin` (hits, misses, UI, level up) plus its `CUES` list.
- NPC voices: `npc_<model>_<event>.wav` or `_1`..`_4` variants (events `aggro attack hit death
  greet`); a model without files uses its name prefix (`glarewolf_alpha` → `glarewolf`).
- Music: `content/music/*.mp3`, chosen by `maps.txt`.
- Details: [audio.md](audio.md).

## Fonts and UI

`content/fonts/DejaVuSerif.ttf` and `DejaVuSerif-Bold.ttf` (`dusk_formats::content::UI_FONT`,
`UI_FONT_BOLD`; licence `LICENSE_DEJAVU`). Interface art in `content/ui/` at fixed names and
sizes ([ui.md](ui.md)); icons are 40x40 PNGs.

---

# How to add ...

Each recipe ends with the same check: `cargo test -p dusk_formats` (references) and a look in the
game (`cargo run -p dusk_client -- <map>`; `DUSK_AUDIO_LOG=1` warns about missing sounds).

## ... a map (e.g. a dungeon)

1. **Art**: tiles (64x32 ground diamonds) and props as PNGs in a new folder, e.g.
   `content/barrow/`, with unique file names (prefix them: `cb_*`). Write them from a generator in
   `tools/artgen/` (copy the structure of `vale.py`: `paths.CONTENT / "barrow"`), plus
   `hotspots.txt` for every upright, `roofs.txt` for roofs/overhangs, `sprite_fx.txt` for torches
   (particles + lights).
2. **Map**: a generator like `mapgen.py` / `valemap.py` builds the cell grid and calls a
   `write_map` (copy it): ground on layer 0, props on layer 2, flags `0x20 | 0x40` on walls.
   Flag every cell that is not floor `0x20` too (walking ignores missing ground).
   Write `maps/<name>.map`, `<name>.spawns` and `<name>.markers` (at least `arrival`; add named
   points for scripted spots). Leave out `.cover` underground (no gaze), or write one with
   `covergen.py` / your generator.
3. **List it** in `data/maps.txt`: `[<name>]` with a new `id` (10002..), `title`, `music`,
   `ambience`, `darkness` (a dungeon wants 0.6-0.85, lit by its `sprite_fx.txt` lights).
4. **NPCs**: spawn lines in `<name>.spawns` (templates must exist, see below).
5. **Sounds**: torches/braziers can loop via `data/sprite_sounds.txt`.
6. **Getting there**: the server starts players on the `default=1` map; for testing use
   `cargo run -p dusk_client -- <name>` (offline) or `cargo run -p dusk_server -- --start-map <name>`.
   Travel between maps is not implemented yet.
7. Check: `cargo test -p dusk_formats` (textures, spawns, `.psi` sprites, sprite sounds) and walk it.

## ... an NPC

1. **Model**: a builder in `tools/artgen/valefolk.py` (humanoids, quadrupeds) or `creatures.py`
   (bugs, goblins); run it to write `scripts/npc/<model>.txt` + `content/sprites/custom_npc_<model>.png`
   (+ `<model>_smear` for weapon swings). Reusing an existing model is fine.
2. **Portrait**: add the model to `tools/artgen/portraits.py` →
   `content/portraits/portrait_custom_<model>.png` (required for hostile and neutral NPCs).
3. **Template**: `[500xx]` in `data/npc_templates.txt` (keys above): `name`, `model`, `height`,
   `level`, `faction`, stats or `-1` for level-based ones, `spellN=` for casters, `loot` / `junk`
   / `loot_chances` / `gold_ratio`.
4. **Voices** (hostile/neutral need `aggro attack hit death`; friendly may have `greet`): add
   generators named `npc_<model>_<event>` to `tools/sfxgen/sounds_game.py` (or reuse a prefix:
   a model `barrow_wolf` falls back to `npc_barrow_*`), run `python -I tools/sfxgen/sfxgen.py npc_<model>`.
5. **Spawn** it in a map's `.spawns`.
6. **Talking / quests** are code: entries, dialogue and quest stages live in
   `crates/dusk_server/src/director/script.rs` (data) and `director/mod.rs` (flow); the client
   shows them generically (`dialogue.rs`, `journal.rs`).
7. Check: `cargo test -p dusk_formats` (model, sheet, portrait, voices, spells, loot).

## ... a spell

1. `data/spells.txt`: a new `[entry]` (player skills 500xx, NPC spells 510xx) with `name`,
   `icon=skill_<slug>.png`, `description`, costs, timings, `range`, effects.
2. `data/spell_visuals.txt`: `[spell <entry>]` naming kits (reuse or add one, see "an effect").
3. Players: add `spell=<entry>` to a class in `data/class_spells.txt`. NPCs: `spellN=` on the
   template.
4. Icon: `python -I tools/artgen/icons.py` draws every missing icon named in the data
   (`--force --only <name>` to redraw).
5. New mechanics need server code (`crates/dusk_server/src/spells.rs`); the formulas and the
   tooltip tokens work for any spell. `dusk_server::spells::skill_tests` shows how to test a
   skill end to end.

## ... an item

- **Hand-made** (rewards, uniques, potions, junk): `[entry]` in `data/items.txt` in its range;
  `icon=` (generated if missing by `icons.py`), `model=` for gear (an existing
  `scripts/player/<model>.txt`, or a new layer in `tools/artgen/gear.py`), `stat=` lines for
  bonuses, `spell=` for consumables. Hand it out via a loot table (`data/loot.txt` +
  `loot=` on an NPC), `start_item=` in `classes.txt`, or a quest reward (director script).
- **Random gear**: a `[base N]` in `data/item_bases.txt` (five names/icons for the qualities);
  it drops automatically at the levels of its band. **Affixes**: `data/affixes.txt`.

## ... a sound

1. Design it in `tools/sfxgen/sounds_game.py` (combat, UI, kits, creatures) or `sounds.py`
   (ambience, story cues, voices): one generator function per file name.
2. `python -I tools/sfxgen/sfxgen.py <name>` writes `content/sfx/<name>.wav` and prints loudness
   checks (the folder has a 16 MB budget).
3. Reference it by bare name: a kit's `sound=`, `sprite_sounds.txt`, `maps.txt` `ambience=`, a
   voice file name, or in code via `PlaySfx` (add new code-played names to
   `dusk_formats::sound::builtin::CUES` so the tests check them).
4. Music: drop an `.mp3` into `content/music/` and name it in `maps.txt` `music=` (or leave
   `music=` empty to rotate the whole soundtrack).

## ... an effect

- **Spell kit**: `[kit <name>]` in `data/spell_visuals.txt` combining a flipbook, particles, a
  sound and glows; place it with `anim_x/anim_y` and test with `DUSK_FX_TEST=<spell id>`
  ([visuals.md](visuals.md)).
- **Flipbook**: add an entry to `tools/artgen/spellfx_layout.json` (canvas, ratio, delay, frame
  boxes) and to the effect table in `spellfx.py`, then `python -I tools/artgen/spellfx.py <name>.sa`
  (writes `scripts/animation/<name>.sa` + `content/spellfx/sfx_<name>_<n>.png`).
- **Particle system**: a `[name]` in `data/particles.txt` (sprite = atlas cell of
  `fx_particles.png`); use it from a kit (`particles=`) or a map sprite (`sprite_fx.txt`).
- **Map light / fire**: `light` and `particles` lines in the art folder's `sprite_fx.txt`;
  darkness per map in `maps.txt`.
