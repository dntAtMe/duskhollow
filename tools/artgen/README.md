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
python -I tools/artgen/creatures.py            # monsters (replace original NPC models with --art custom)
python -I tools/artgen/valefolk.py             # Duskhollow creatures + people (glarewolf, stooped, hollowed_warden, ...)
python -I tools/artgen/portraits.py            # unit-frame portraits of the above (optionally: model names)
python -I tools/artgen/mapgen.py               # environment tiles/props (enviro.py) + the custom_glade map
python -I tools/artgen/valemap.py              # crimson vale art (vale.py) + the custom_duskhollow demo map (~9 min)
python -I tools/artgen/ui.py                   # our UI skin (override/ui, used with --art custom); --preview: contact sheet
python -I tools/artgen/windows_ui.py           # window chrome: close boxes, quest journal, Track buttons, micro-menu
python -I tools/artgen/icons.py                # 406 item + 113 spell icons (40x40) -> content/override/icons, previews
python -I tools/artgen/icons.py --only sword   # just the icons whose file name contains "sword"
python -I tools/artgen/spellfx.py              # all 104 spell flipbooks (.sa) + the particle atlas
python -I tools/artgen/spellfx_measure.py <assets>  # re-measure original flipbook layout (rarely needed)
cargo run -p dusk_client -- custom_glade --art custom   # play our map with our character
python -I tools/artgen/character.py --preview  # previews only
cargo run -p dusk_client -- --art custom         # play with the custom player sprite
```

Output follows the original sprite-script format (`scripts/player/...txt` + sheet PNG), so the
engine renders it exactly like original sprites. The client mirrors `custom_assets/content` and
`custom_assets/scripts` into its own subfolders of the extracted assets at startup.

| File | Content |
|---|---|
| `vox.py` | SDF primitives, bones, voxelizer, projection, palette/dither/outline renderer |
| `character.py` | base adventurer model + stance/run/swing/hit/block/cast/shoot/die animations, sheet packer. `run`: feet planted at the client's run speed (2-bone IK on a stance/swing foot path, no sliding). Attacks `swing` (horizontal slash), `swing2` (overhead chop), `swing3` (thrust), `cast`, `shoot` (bow draw) are keyframed (`keyframes.py`): held anticipation, strike snapped into one frame, held follow-through, eased recovery; `HITS` = hit frame, written as `hit=<ms>` so the client lines damage, flinch, sparks and sound up with the blow |
| `keyframes.py` | keyed poses (bone -> euler degrees, inherited key to key, per-key easing) and `retime` (holds and snaps for an existing smooth animation) |
| `smear.py` | weapon smear layers `<model>_smear`: the blade's swept arc between the previous and the strike / follow-through frame, depth-tested against the body, 3 dithered oxblood-to-bone bands; exported for the adventurer, every swung gear weapon, goblins, the Stooped and Corvin |
| `gear.py` | naked `custom_body` + one layer per item model (`item_template.model`: cloth/leather/chain/plate/mage sets, swords, daggers, axes, maces, staves, bows, buckler/shield). Each layer is depth-tested against the body so arms stay in front of armour and weapons held behind the back are hidden; equipped items then render with our art (`--art custom`) |
| `sheet.py` | renders animations in 8 directions, packs sheets, writes sprite scripts + previews |
| `creatures.py` | goblin / goblin_charger (humanoid rig, reuses the adventurer's animations with a hunch), spider (8 legs) and Antling (`antlion_small`, 6 legs, mandibles) on a generic arthropod rig with gait/bite/hit/death-flip; written to `scripts/npc/custom/<model>.txt` |
| `valefolk.py` | Duskhollow's creatures and people (docs/world.md) for the custom NPC templates in `custom_assets/data/npc_templates.txt`: `glarewolf` (quadruped rig: trot, snapping bite `swing` and pounce `swing2`, rolls onto its side), `stooped` (hollowed lightworker bent double, head craned at the sky, rusty sickle), `hollowed_warden` (Corvin, 1.3x: tarnished pitted plate, torn tabard and cape, halberd; `swing` = sweep, `swing2` / `cast` = overhead slam, both with a held telegraph), `cairnkeeper` (Ysolde: shawls, hood, ember-tipped poker, ember pouch), `lightworker` (straw hat + face veil, sack, hoe), `lowshade_guard` (hooded mantle over brigandine, spear, lantern, oilcloth canopy on a back pole). Ramps come from `lit()`: dark albedo under a crimson key light falling into bruised violet shadow; only fire glows |
| `portraits.py` | 80x80 close-ups on a dithered vignette (`portrait_custom_<model>.png`), used by the HUD with `--art custom` |
| `enviro.py` | ground tiles (grass/dirt/cobble/water: 4x4-cell periodic textures sliced into 16 seamless tiles), trees, pines, rocks, bushes, stone wall blocks, a 3x3-cell log hut, campfire, lamp post, crates, barrels; `hotspots.txt` (pivots) and `sprite_fx.txt` (particles/lights, same meaning as the original `sprite_psi` / `sprite_light`) |
| `ui.py` | "iron & oak" interface skin: dark oak panels in riveted blackened-iron frames with brass corner plates, oxblood leather title bars, recessed iron-rimmed wells, hand-made 5x7 pixel caps for baked labels (no commercial fonts). Writes ~59 PNGs with the ORIGINAL names and sizes to `custom_assets/content/override/ui/` (toolbar, cast bar, unit frames + bars + level badge + elite/boss rings, XP bar, nameplates, abilities window + row, inventory / character / loot windows, chat panel + buttons, speech bubble 9-slice, minimap frame / fade decal / zoom button / dots, gold pouch, faction portrait placeholders). Wells, bar tracks, holes and buttons sit at the pixel positions the client measured from the originals (numbers in each generator's docstring) |
| `windows_ui.py` | window chrome in the same skin, own names in `custom_assets/content/custom/ui/` (always available): `ui_close*` close boxes (26 / 22 px, idle / hover / press), the quest journal panel `ui_journal.png` (list + detail wells), `ui_btn_{track,untrack}_*` buttons, the micro-menu bar, plates and brass icons (`ui_micro_*`). Geometry in the docstring and `windows.rs` / `journal.rs` |
| `icons.py` (+ `iconlib.py`, `icon_items.py`, `icon_spells.py`) | one icon per original `item_icons_new` / `spell_icons_new` file name (same size) into `custom_assets/content/override/icons/`, replacing the originals with `--art custom`. Items: voxel models picked by `item_template.model`, armour icon names (`rb/lt/ch/pl` x slot) and name keywords, on a dithered card + frame tinted by quality, accents (trim metal, gem, cloth) by quality; `scroll_<spell>` = parchment with that spell's icon inset. Spells: 2D motifs + small voxel props chosen by keyword rules on name / icon name / description (effect types as fallback), palette by school. `iconlib.py`: 2D SDF shapes, 3D primitives (lathe, extrusion, convex/gem cuts, rough rocks), orthographic voxel renderer, framed canvas. Contact sheets: `custom_assets/preview/icons_*.png` |
| `spellfx.py` | spell visual effects, drawn from scratch as float intensity fields (sigils, noise-flame bursts, rings, light columns, domes, wire bubbles, sparkles, snowflakes, sky beam, lightning, slash arcs, swirls, spark rays, spirit, angel, wisps, whirlwinds, comets, portals) mapped through 7-step per-school ramps (fire/frost/holy/shadow/nature/pink/arcane/cyan/blood/earth, `white` for kits that recolour with `sprcolor`) with Bayer dithering, on opaque black (the client luma-keys that = additive look); quest markers, arrows and the obelisk use real alpha. Each original `scripts/animation/<name>.sa` gets a replacement in `scripts/override/animation/` with the same ratio, canvas, delay, loop range and frame count, `filename=sfx_<orig>` (frames in `content/custom/spellfx/`), each frame drawn into the box the original frame covered, at world resolution (canvas/ratio) scaled up by `ratio` -> placement via `spell_visual_kit.spranim_x/y` is unchanged. Also `content/override/fx/particles.png`: 16 white particle sprites in the original atlas slots, re-skinning every `.psi` |
| `spellfx_measure.py` | reads the ORIGINAL flipbooks once and stores only layout in `spellfx_layout.json` (header, per-frame bounding box, brightness envelope, dominant colour -> school ramp); no pixels are kept |
| `mapgen.py` | `custom_glade`: meadow ringed by forest, winding path, ruined plaza, a pond with fireflies, a woodcutter's camp; bakes blended tiles where ground types meet; writes the `.map` + a `.spawns` sidecar |
| `vale.py` | crimson-palette environment set (docs/world.md art rules: crimson key light from above, violet shade, only fire glows): 9 ground kinds x 3 shadow levels (open / shade / deep shelter), cliff columns carved from a domain-warped occupancy field (unique where a face shows, periodic variants inside the rock mass; the overhang wall carries the rock lip), covered huts with deep eaves, roofed-lane segments, canopy shelters, grain, lightworker poles, rest cairn + brazier, lantern posts, Glare Gate towers/walls, the Fallen Blade, dead trees, bones, boulders, clutter -> `content/custom/vale/` |
| `valemap.py` | `custom_duskhollow` ("First Gaze" demo, docs/demo-plan.md): gorge-mouth arrival (S), Lowshade under the NW overhang, the Red Fields (E), the Glare Gate arena and the Fallen Blade (N), a still pool that reflects the Eye; cliff heights capped so no column hides walkable ground; writes `.map`, `.spawns`, `.cover`, `.markers` and `preview/custom_duskhollow*.png` (overview, cover map) |

Custom maps are named `custom_*`; the server loads them from the asset `maps/` folder with ids
from 10000 and spawns NPCs from `maps/<name>.spawns` (`entry x y orientation wander_distance`).
