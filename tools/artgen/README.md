# artgen — our own oldschool sprites

Legacy art can't be redistributed, so this folder generates replacement art
procedurally, in a Diablo / Ultima Online style "pre-rendered" look: models are built in code
from signed-distance primitives on a skeleton (`vox.py`), voxelized, posed per animation frame,
and rendered through the game's exact 2:1 isometric projection with a limited palette,
ordered dithering and a dark outline.

```bash
python -I tools/artgen/character.py            # player sprite (+ previews)
python -I tools/artgen/gear.py                 # paper-doll body + ~60 gear layers + weapon smears, 6 processes
python -I tools/artgen/gear.py dagger --anims swing,swing2   # re-render only some layers / animations
python -I tools/artgen/creatures.py            # monsters (scripts/npc, the only NPC art)
python -I tools/artgen/valefolk.py             # Duskhollow creatures + people (glarewolf, stooped, hollowed_warden, ...)
python -I tools/artgen/portraits.py            # unit-frame portraits of the above (optionally: model names)
python -I tools/artgen/mapgen.py               # environment tiles/props (enviro.py) + the glade map
python -I tools/artgen/valemap.py              # crimson vale art (vale.py) + the duskhollow demo map (~9 min)
python -I tools/artgen/ui.py                   # our UI skin (override/ui); --preview: contact sheet
python -I tools/artgen/menu_ui.py              # main menu / options / game menu pieces (custom/ui); --preview: contact sheet
python -I tools/artgen/windows_ui.py           # window chrome: close boxes, quest journal, Track buttons, micro-menu
python -I tools/artgen/icons.py                # icons named in data/{spells,items,item_bases}.txt that are missing -> content/icons
python -I tools/artgen/icons.py --force --only sword   # redraw our generated icons whose name contains "sword"
python -I tools/artgen/spellfx.py              # all 104 spell flipbooks (.sa) + the particle atlas
python -I tools/artgen/lightfx.py              # light glow + darkness mask (fx_light_glow.png, fx_light_mask.png)
cargo run -p dusk_client -- glade   # play our map
python -I tools/artgen/character.py --preview  # previews only
cargo run -p dusk_client                       # main menu
```

Output follows the original sprite-script format (`scripts/player/...txt` + sheet PNG), so the
engine renders it exactly like original sprites. The client reads `custom_assets/` directly (its
default asset source) and indexes every file under `custom_assets/content` by bare name.

| File | Content |
|---|---|
| `vox.py` | SDF primitives, bones, voxelizer, projection, palette/dither/outline renderer |
| `character.py` | base adventurer model + stance/run/swing/hit/block/cast/shoot/die animations, sheet packer. `run`: feet planted at the client's run speed (2-bone IK on a stance/swing foot path, no sliding). Attacks `swing` (horizontal slash), `swing2` (overhead chop), `swing3` (thrust), `cast`, `shoot` (bow draw) are keyframed (`keyframes.py`): held anticipation, strike snapped into one frame, held follow-through, eased recovery; `HITS` = hit frame, written as `hit=<ms>` so the client lines damage, flinch, sparks and sound up with the blow |
| `keyframes.py` | keyed poses (bone -> euler degrees, inherited key to key, per-key easing) and `retime` (holds and snaps for an existing smooth animation) |
| `smear.py` | weapon smear layers `<model>_smear`: the blade's swept arc between the previous and the strike / follow-through frame, depth-tested against the body, 3 dithered oxblood-to-bone bands; exported for the adventurer, every swung gear weapon, goblins, the Stooped and Corvin |
| `gear.py` | naked `custom_body` + one layer per item model (`item_template.model`: cloth/leather/chain/plate/mage sets, swords, daggers, axes, maces, staves, bows, buckler/shield). Each layer is depth-tested against the body so arms stay in front of armour and weapons held behind the back are hidden; equipped items render with these layers (the only player art) |
| `sheet.py` | renders animations in 8 directions, packs sheets, writes sprite scripts + previews |
| `creatures.py` | goblin / goblin_charger (humanoid rig, reuses the adventurer's animations with a hunch), spider (8 legs) and Antling (`antlion_small`, 6 legs, mandibles) on a generic arthropod rig with gait/bite/hit/death-flip; written to `scripts/npc/<model>.txt` |
| `valefolk.py` | Duskhollow's creatures and people (docs/world.md) for the custom NPC templates in `custom_assets/data/npc_templates.txt`: `glarewolf` (quadruped rig: trot, snapping bite `swing` and pounce `swing2`, rolls onto its side), `stooped` (hollowed lightworker bent double, head craned at the sky, rusty sickle), `hollowed_warden` (Corvin, 1.3x: tarnished pitted plate, torn tabard and cape, halberd; `swing` = sweep, `swing2` / `cast` = overhead slam, both with a held telegraph), `cairnkeeper` (Ysolde: shawls, hood, ember-tipped poker, ember pouch), `lightworker` (straw hat + face veil, sack, hoe), `lowshade_guard` (hooded mantle over brigandine, spear, lantern, oilcloth canopy on a back pole). Ramps come from `lit()`: dark albedo under a crimson key light falling into bruised violet shadow; only fire glows |
| `portraits.py` | 80x80 close-ups on a dithered vignette (`portrait_custom_<model>.png`), used by the HUD |
| `enviro.py` | ground tiles (grass/dirt/cobble/water: 4x4-cell periodic textures sliced into 16 seamless tiles), trees, pines, rocks, bushes, stone wall blocks, a 3x3-cell log hut, campfire, lamp post, crates, barrels; `hotspots.txt` (pivots) and `sprite_fx.txt` (particle emitters and lights, docs/visuals.md) |
| `ui.py` | "iron & oak" interface skin: dark oak panels in riveted blackened-iron frames with brass corner plates, oxblood leather title bars, recessed iron-rimmed wells, hand-made 5x7 pixel caps for baked labels (no commercial fonts). Writes ~59 PNGs with the ORIGINAL names and sizes to `custom_assets/content/ui/` (toolbar, cast bar, unit frames + bars + level badge + elite/boss rings, XP bar, nameplates, abilities window + row, inventory / character / loot windows, chat panel + buttons, speech bubble 9-slice, minimap frame / fade decal / zoom button / dots, gold pouch, faction portrait placeholders). Wells, bar tracks, holes and buttons sit at the pixel positions the client measured from the originals (numbers in each generator's docstring) |
| `menu_ui.py` | Front-end pieces in the same skin, all drawn for 9-slicing (`custom_assets/content/ui/menu_*`): leather plate buttons (idle / hover with brass rim / press / disabled), oak and near-black panels in iron frames with brass corners, text-field wells (+ brass focus rim), slider track / ember fill / brass knob, check boxes, the four class medallions (Vanguard, Emberwright, Cutthroat, Ashpriest) + selection ring, and the bronze rule under titles |
| `windows_ui.py` | window chrome in the same skin, own names in `custom_assets/content/ui/` (always available): `ui_close*` close boxes (26 / 22 px, idle / hover / press), the quest journal panel `ui_journal.png` (list + detail wells), `ui_btn_{track,untrack}_*` buttons, the micro-menu bar, plates and brass icons (`ui_micro_*`). Geometry in the docstring and `windows.rs` / `journal.rs` |
| `icons.py` (+ `iconlib.py`, `icon_items.py`, `icon_spells.py`) | reads `icon=` from our `data/spells.txt`, `items.txt`, `item_bases.txt`; every icon not yet under `custom_assets/content/` is drawn (40x40) into `content/icons/{items,spells}/` (`--force` redraws those). Older icons in `content/icons/` keep their names. Items: voxel models picked by `model`, `equip_type` / `weapon_type` / `armor_type`, armour slot words and name keywords, on a dithered card + frame tinted by quality, accents (trim metal, gem, cloth) by quality; `scroll_<spell>` = parchment with that spell's icon inset. Spells: 2D motifs + small voxel props chosen by keyword rules on name / icon name / description (effect types as fallback), palette by school. `iconlib.py`: 2D SDF shapes, 3D primitives (lathe, extrusion, convex/gem cuts, rough rocks), orthographic voxel renderer, framed canvas. Contact sheets: `custom_assets/preview/icons_*.png` |
| `spellfx.py` | spell visual effects, drawn from scratch as float intensity fields (sigils, noise-flame bursts, rings, light columns, domes, wire bubbles, sparkles, snowflakes, sky beam, lightning, slash arcs, swirls, spark rays, spirit, angel, wisps, whirlwinds, comets, portals) mapped through 7-step per-school ramps (fire/frost/holy/shadow/nature/pink/arcane/cyan/blood/earth, `white` for kits that recolour with `sprcolor`) with Bayer dithering, on opaque black (the client luma-keys that = additive look); quest markers, arrows and the obelisk use real alpha. Each `scripts/animation/<name>.sa` takes its ratio, canvas, delay, loop range, frame count and per-frame boxes from `spellfx_layout.json`, `filename=sfx_<name>` (frames in `content/spellfx/`), drawn at world resolution (canvas/ratio) scaled up by `ratio`; kits place them with `anim_x/anim_y` (`data/spell_visuals.txt`). Also `content/fx/fx_particles.png`: the 16-cell particle atlas of `data/particles.txt` |
| `lightfx.py` | light textures: `fx_light_glow.png` (422x193 elliptical additive glow, tinted per light) and `fx_light_mask.png` (1024x512 darkness cut-out, alpha ~0.04 in the centre to 1 at the edge), dithered |
| `mapgen.py` | `glade`: meadow ringed by forest, winding path, ruined plaza, a pond with fireflies, a woodcutter's camp; bakes blended tiles where ground types meet; writes the `.map` + a `.spawns` sidecar |
| `vale.py` | crimson-palette environment set (docs/world.md art rules: crimson key light from above, violet shade, only fire glows): 9 ground kinds x 3 shadow levels (open / shade / deep shelter), cliff columns carved from a domain-warped occupancy field (unique where a face shows, periodic variants inside the rock mass; the overhang wall carries the rock lip), covered huts with deep eaves, roofed-lane segments, canopy shelters, grain, lightworker poles, rest cairn + brazier, lantern posts, Glare Gate towers/walls, the Fallen Blade, dead trees, bones, boulders, clutter -> `content/vale/` |
| `valemap.py` | `duskhollow` ("First Gaze" demo, docs/demo-plan.md): gorge-mouth arrival (S), Lowshade under the NW overhang, the Red Fields (E), the Glare Gate arena and the Fallen Blade (N), a still pool that reflects the Eye; cliff heights capped so no column hides walkable ground; writes `.map`, `.spawns`, `.cover`, `.markers` and `preview/duskhollow*.png` (overview, cover map) |

Custom maps are named `custom_*`; the server loads them from the asset `maps/` folder with ids
from 10000 and spawns NPCs from `maps/<name>.spawns` (`entry x y orientation wander_distance`).
