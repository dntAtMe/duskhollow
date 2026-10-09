# Plan: make Duskhollow standalone (remove the legacy data pack)

Goal: our content is the default and only source. A fresh clone builds and runs with no external
data. Afterwards we build the Sword Barrow dungeon, so adding content must be easy.

Order: **Stream 0** (content seam, sequential) → **A, B, C, D in parallel worktrees** → **E** (cleanup,
sequential). The game stays playable at every commit. Merge order A, B, C, D, then E.
Worktrees have no legacy `assets/`: point `DUSK_LEGACY` at `E:/Github projects/dreadmyst-bevy/assets`.

## 1. Inventory (what still depends on legacy data)

### game.db queries
| Query | Feeds | Custom content actually needs |
|---|---|---|
| `db.rs::maps` | server world, client map list/music/menu | only 13 legacy maps; custom maps get ids 10000 (duskhollow), 10001 (glade) |
| `db.rs::npc_models/npc_templates` | templates, models | `glade.spawns` uses legacy entries 2 (Antling, antlion_small, neutral), 8 (Spiderite, spider, neutral), 13 (Crazed One, goblin_charger, hostile), 14 (Venomous One, goblin, hostile); custom sprite scripts exist for all 4 |
| `db.rs::npc_spawns` | legacy maps | none (server still loads all legacy maps) |
| `db.rs::class_stats` | server stats, menu level-1 stats | all; linear: hp/mana = base × level (×1.5 at 25); attributes constant; bases c1 75/30, c2 40/70, c3 60/45, c4 45/65 |
| `db.rs::exp_levels` | XP, cap, kill XP | all 25 rows |
| `db.rs::teleports` | default server start | default launch only |
| `db.rs::sprite_hotspots` | hotspots | none for custom textures |
| `spell.rs::spells` | spell tables | 81 Melee Swing, 82 Ranged Attack (auto attacks), 89 Minor Life Potion, 95 Minor Mana Potion (item spells), NPC spells 229 Rend (Alpha), 67 Harrowing Strike + 275 War Stomp (Corvin); starting spells 13/10/9, 31/27/29, 60/46/43, 69/79/77, 245 Sleep, 273 Pick Lock |
| `spell.rs::class_spells` | starting spellbook | 28 rows |
| `spell.rs::spell_visual_kits/spell_visuals` | client visuals | 24 kits used: 6,15,18,47,55,61,78,88,98,101,103,104,105,119,122,126,129,132,140,153,158,179,198,199 → 10 flipbooks (slash_001, slash_002b, slash_002c, water_001, wind_003a, wind_003b, earth_002a, effect_003, effect_004, light_003), 5 particle systems, 19 sounds |
| `sprite_fx.rs::sprite_psi/lights` | map emitters | `green_firefly.psi` in glade |
| `sprite_fx.rs::zone_night_pct` | darkness | none (dungeon will want darkness) |
| `sound.rs::sound_tables` | music/ambience/voices/proximity | `npc_sounds` for the 4 glade models (~11 files) |
| `item.rs::items` (17,179) | items | starting items 1, 7 (potions ×5), 13-16 cloth set, 17 buckler, 18 blade, 19 shortbow, 20 dagger, 23 staff, 25 mace; quest reward `EMBER_DRAUGHT = 1`; gear drops from a generated grid (128 bases × 5 qualities × 25 levels, 65 models, 155 icons) |
| `item.rs::affixes`, starting_items, class_armor, class_desirable_stats, material_chances, loot_tables, junk_loot, npc_loot | server items/loot | all; custom NPCs have gold+gear only; glade legacy NPCs use junk |

### Legacy files loaded even with `--art custom`
- Fonts `Friz Quadrata Regular/Bold.ttf` (combat_ui.rs load_font, director_ui.rs:148, menu/widgets.rs:46) — not redistributable.
- `light_source.png` (lights.rs every sprite_fx light), `shader_light.png` (darkness cut-outs).
- Particles `scripts/particles/*.psi` (binary HGE): campfire, small_light_embers, casting_fire, scorch, cheapshot, entangleshot, green_firefly, disarm. Particle texture already overridden (`content/override/fx/particles.png`).
- Flipbooks: all 104 `.sa` already overridden (`scripts/animation`, frames in `content/spellfx`).
- Sounds (~50): 20 builtin names in `dusk_formats::sound::builtin` (attack_hit_var01..05.wav, attack_sword_normal_{s,m,m2,h}.ogg, dodge_default.wav, swishverb4.ogg, attack_metal_case.ogg, e3_attack_hardhit01..03.ogg, vdamage3_mlb_4.ogg, alert_levelup_a.ogg, button_click_a.ogg, window_target_open_a.ogg, window_open_a.ogg, window_close_a.ogg); 19 kit sounds; ~11 glade NPC voices.
- Icons: item icons all overridden (`override/icons/items`); 113 spell icons overridden with legacy spell-name filenames; our skills use `content/icons/spells/skill_*.png`.
- UI: all overridden/custom. Cursors are system cursors.
- Text: `scripts/text/help.txt` (chat /help), `scripts/sprite/portrait_offset.txt` (legacy portraits only), `config.ini` (optional).
- Sprite scripts: all custom (`scripts/player`, `scripts/npc`).

### Legacy-only code
`crates/dusk_extract` + `zip`; `rusqlite` (db.rs, `impl GameDb` in item.rs/spell.rs/sound.rs/sprite_fx.rs); `assets_root()`/`custom_assets_root()`/`FileIndex::load(file_index.txt)`; `custom.rs::install` (mirrors custom_assets into assets at start), `CUSTOM_MAP_PREFIX`, `CUSTOM_NPC_FIRST/CUSTOM_SPELL_FIRST`, `CustomVisual`, `merge_spells`; `--art`/`DUSK_ART`/`custom_art`/Settings custom_art + legacy_maps toggles and all branches on `custom_art` (data.rs, unit.rs, paper_doll.rs, hud.rs, audio.rs, audio/custom.rs); override mechanism (`content/override/**`, `scripts/animation`); legacy paper-doll lists (`paper_doll.rs BODY`, `unit.rs PAPER_DOLL`); mp3→ogg sound fallback; `RegionGrid` zone/area music; legacy tests (real_assets.rs except `custom_maps_parse_and_resolve`, tests/sounds.rs, audio.rs `referenced_sounds_decode`, server tests gated on game.db); tools reading game.db (`icons.py`, `spellfx_measure.py`; keep `spellfx_layout.json` as our data; `mapgen.py` writes legacy glade entries); docs with RE notes (formats.md, combat.md, ui.md, audio.md) and Ghidra/`FUN_`/0x comments in ~30 source files.

## 2. Target design

### One tracked root
`custom_assets/` → `assets/` (tracked; ignore `/assets/preview` and `/assets/env_manifest.json`):
```
assets/
  data/     classes.txt exp.txt spells.txt class_spells.txt npc_templates.txt items.txt item_bases.txt
            affixes.txt loot.txt maps.txt spell_visuals.txt particles.txt sprite_sounds.txt help.txt
  maps/     <name>.map .spawns .cover .markers
  scripts/  animation/*.sa  npc/*.txt  player/*.txt
  content/  sprites/ env/ vale/ spellfx/ portraits/ music/ sfx/ fx/ fonts/ ui/ icons/items/ icons/spells/
```
`assets_root()` = `$DUSK_ASSETS`, else `<workspace>/assets`, else next to the exe. Bare-name resolution
stays: `FileIndex::scan(root)` walks `content/` at startup; case-insensitive duplicate basenames are an
error (tested).

### Data format: extend the `[id]` + `key=value` text style
Generic parser `dusk_formats::content::sections`:
```rust
pub struct Section { pub kind: Option<String>, pub id: String, pub line: usize, pub entries: Vec<(String, String)> }
impl Section {
  pub fn get(&self, k: &str) -> Option<&str>;            // last value
  pub fn all<'a>(&'a self, k: &'a str) -> impl Iterator<Item=&'a str>;  // repeated keys
  pub fn int(&self, k: &str, default: i64) -> i64;  pub fn num(&self, k: &str, default: f32) -> f32;
  pub fn list(&self, k: &str) -> Vec<&str>;
  pub fn range(&self, k: &str) -> Option<(f32, f32)>;
}
pub fn parse(text: &str) -> anyhow::Result<Vec<Section>>; // "[id]" or "[kind id]", '#' comments, errors on key outside section / duplicate (kind,id)
pub fn load(path: &Path) -> anyhow::Result<Vec<Section>>;
pub fn unknown_keys(sections: &[Section], known: &[&str]) -> Vec<String>; // tests assert shipped files have none
```
Formulas reuse `spell::eval_formula`.

- **classes.txt** `[1]`..`[4]`: `name`, `role`, `hp=75*clvl`, `mana=30*clvl`, `strength agility willpower intelligence courage`, `armor=` (allowed armour types), `weapons=` (allowed weapon types), `stats=` (desirable), `start_item=<entry>,<count>` (repeat).
- **exp.txt** `[N]`: `exp=`, `kill_exp=`, optional `title=`. Keep levels 1-6 within ±10 % of today.
- **spells.txt**: existing keys; visual keys move to spell_visuals.txt. New: 50100 Attack (melee_atk, replaces 81), 50101 Shoot (ranged_atk, cast 300, replaces 82), 50110 Ember Draught (item spell, replaces 89), 50111 Lamp Tonic (replaces 95), 51001 Rend (229), 51002 Harrowing Sweep (67), 51003 Gate Slam (275). Add `melee_atk`/`ranged_atk` to `effect_kind`. Icons `skill_<slug>.png`.
- **class_spells.txt**: the full starting list (no legacy base); every class gets 50100 + 50101. Legacy class skills go away, so author **2-3 replacement skills per class where a class gets thin** (Emberwright needs more offence, Ashpriest needs a heal for others), in the world's voice.
- **npc_templates.txt**: new keys `loot=`, `loot_chances=g,b,gold,p` (−1 = default), `gold_ratio=`, `junk=<item>,<item>`. Alpha `spell1=51001,...`; Corvin `spell1=51002,...`, `spell2=51003,...`. New glade templates 50020 (goblin, hostile, replaces 14), 50021 (goblin_charger, hostile, replaces 13), 50022 (antlion_small, neutral, replaces 2), 50023 (spider, neutral, replaces 8).
- **items.txt** `[entry]`: `name icon model equip_type weapon_type armor_type material quality required_level stack spell(repeat) stat=<id>:<value>(repeat) sell_price flags description`. Ranges: 1-99 consumables/quest (1 Ember Draught keeps `EMBER_DRAUGHT=1`; 2 Lamp Tonic); 10-19 starter gear (models cloth_gloves, cloth_pants, cloth_sandals, cloth_shirt, buckler, shortsword, shortbow, dagger, staff_grey, club); 100-199 junk (glarewolf, stooped, hollowed_warden, goblin, spider, antlion_small, 2-3 each); 1000-9999 hand-made named items.
- **item_bases.txt** `[base N]`: `name model icon equip_type weapon_type|armor_type material levels=1-25`. Generator emits qualities 2..6 × levels; entry = `100000 + N*1000 + quality*100 + level` (deterministic, shared). Tier from the level band (replaces material_chance tables). Mirror today's coverage (cloth/leather/chain/plate/mage × head/chest/legs/feet/hands; 7 weapon types; buckler; bow; neck/belt/ring); names our own.
- **affixes.txt** `[N]`: `name noun=0/1 level=1-5 stat=<id>:<factor>(repeat)`; ~30-60 of our own.
- **loot.txt** `[loot N]`: `item=<entry>,<chance%>,<min>-<max>` (repeat).
- **maps.txt** `[map_name]`: `id` (10000 duskhollow, 10001 glade), `title`, `default=1`, `music=` (empty = whole soundtrack), `ambience=`, `darkness=0..1` (replaces zone night_pct). Start = `arrival` marker.
- **spell_visuals.txt**: `[kit <name>]` (`anim=… anim_x anim_y anim_color anim_blend`, optional anim2*, `particles=… particles_x particles_y`, `sound=`, `unit_glow=`, `ground_glow=`) and `[spell N]` (`impact traveling casting go aura go_anim cast_anim`). `VisualKit.id: i64` → `name: String`.
- **particles.txt** (replaces binary .psi): `[name]` `sprite=<cell 0..15> blend=add emission lifetime life=a,b direction spread relative speed=a,b gravity=a,b radial=a,b tangential=a,b size=s,e,var spin=s,e,var color_start=r,g,b,a color_end=r,g,b,a color_var alpha_var` (degrees). Parses into the existing `psi::ParticleSystemInfo` (simulation untouched); drop the binary parser.
- **sprite_fx.txt**: `particles <sprite> <name> x y` (keep `psi` alias; strip `.psi`) and `light ...`.
- Keep our formats: `.map` (zone/area sections parsed and ignored), sprite scripts, `.sa`, `.cover`, `.markers`, `.spawns`, `hotspots.txt`, `roofs.txt`, `spellfx_layout.json`, `sprite_sounds.txt`; `help.txt` is ours.
- Remove rusqlite + zip; `db.rs` types → `content/types.rs` (MapInfo gains `darkness`).

## 3. Replacement content
| What | How |
|---|---|
| UI font (regular + bold + license) | DejaVu Serif / Serif Bold from `C:/Users/Kacper/AppData/Local/Programs/Python/Python311/Lib/site-packages/matplotlib/mpl-data/fonts/ttf/` (free redistribution, ship `LICENSE_DEJAVU`), in `content/fonts/`, constants `UI_FONT`, `UI_FONT_BOLD` |
| `fx_light_glow.png` | new `tools/artgen/lightfx.py`: ~422×193 elliptical white glow, dithered |
| `fx_light_mask.png` | same tool: 1024×512 alpha mask (~0.04 centre → 1 edge) |
| Particle atlas | `override/fx/particles.png` → `content/fx/fx_particles.png` |
| 7 particle defs | campfire, lantern_embers, fireflies, fire_cast, ember_trail, knife_trail, hook_trail |
| ~26 builtin sounds | sfxgen: hit_npc_1..4, hit_blade_1..4, hit_blunt_1..4, miss, dodge, parry, block_1..3, player_hurt_1..3, level_up, ui_click, ui_target, ui_window_open, ui_window_close, item_use |
| 19 kit sounds | sfxgen: spell_heavy_slash, spell_bleed_hit, spell_skull_crack, spell_dash, spell_blade_hit, spell_ground_slam, spell_knife_throw, spell_fire_cast, spell_ember_whoosh, spell_ember_burst, spell_hook_throw, spell_hook_hit, spell_bow_draw, spell_arrow_release, spell_arrow_hit, spell_ember_heal, spell_veil, spell_warden_sweep, spell_warden_roar |
| 12 glade voices | sfxgen: npc_{goblin,spider,antlion_small}_{aggro,attack,hit,death} |
| Icons | `icons.py` driven by our spells/items data: 7 new spell icons + ~20 item icons + icons for any new class skills |
| help.txt | `data/help.txt` |

## 4. Streams

### Stream 0: content seam (refactor only, no visible change)
1. `lib.rs`: `content_root()` (= today's custom_assets root) and `legacy_root()` (`$DUSK_LEGACY`, else today's assets root with game.db); `assets_root()` aliases `legacy_root()` until E.
2. `dusk_formats/src/content/{mod,sections,rules,npcs,spells,items,maps,visuals,particles,sprite_fx,sounds}.rs`, bodies delegating to GameDb + existing custom merges. Contract:
```rust
rules::load(root) -> anyhow::Result<Rules{ class_stats: HashMap<(i64,i64),ClassStats>, exp_levels: Vec<ExpLevel>,
    class_spells: HashMap<i64,Vec<i64>>, start_items: HashMap<i64,Vec<(i64,i64)>>, class_armor: HashMap<i64,Vec<i64>>,
    desirable_stats: HashMap<i64,Vec<i64>>, class_weapons: HashMap<i64,Vec<i64>> }>
npcs::load(root) -> anyhow::Result<Npcs{ templates, models, loot: HashMap<i64,NpcLoot>, junk: HashMap<i64,Vec<i64>> }>
spells::load(root) -> anyhow::Result<HashMap<i64,SpellTemplate>>
items::load(root) -> anyhow::Result<ItemTables{ items, affixes, loot_tables, grid: HashMap<(i64,i64),Vec<i64>> }>
maps::load(root) -> anyhow::Result<Vec<MapInfo>>;  maps::spawns(root, &MapInfo) -> Vec<NpcSpawn>;  maps::default_map(&[MapInfo]) -> Option<&MapInfo>
visuals::load(root, &spells) -> anyhow::Result<HashMap<i64,SpellVisual>>;  visuals::flipbook_path(root, name) -> Option<PathBuf>
particles::load(root) -> anyhow::Result<HashMap<String, ParticleSystemInfo>>   // lowercase name, no extension
sprite_fx::load(root) -> anyhow::Result<SpriteFx{ psi, lights, hotspots, roofs }>
sounds::load(root) -> anyhow::Result<SoundTables>
```
3. Move `VisualKit`, `KitAnim`, `SpellVisual` from spell.rs to `content/visuals.rs` (re-export); move `CustomVisual` there; move all `visual=`, `*_kit=`, `*_anim=` lines from `custom_assets/data/spells.txt` into new `custom_assets/data/spell_visuals.txt` as `[spell N]` sections (legacy kit ids for now).
4. Rewire every consumer: client data.rs, items_ui.rs::load_item_db, menu/mod.rs (level-1 stats), audio.rs, particles.rs (preloaded map, no lazy file reads); server world.rs, items.rs::ItemData::load, lib.rs. Flipbook override unconditional.
5. Two asset sources, no mirroring: Bevy `AssetPlugin.file_path = content_root()` + registered asset source `legacy://` at `legacy_root()`. FileIndex loads `legacy_root()/file_index.txt` with `legacy://` prefixes, then indexes content_root files over it. `GameData::fs_path(rel)` maps `legacy://` to a filesystem path; `GameData::find_file(rel)` reads scripts/maps from content_root first, then legacy. Delete `custom::install` calls; server reads custom maps from content_root and legacy maps from legacy_root.
6. `DUSK_LEGACY_LOG=1` logs each distinct legacy load / fs read once (the runtime inventory each stream checks).
7. Acceptance: `cargo test --workspace` green; First Gaze plays unchanged; quest_bot passes.

### Stream A: rules and gameplay data
Owns: `content/{rules,npcs,spells,items,maps}.rs` bodies; `dusk_formats/src/{db.rs,item.rs,spell.rs (except re-export),custom.rs}`; `dusk_server/src/**`; `custom_assets/data/{classes,exp,spells,class_spells,npc_templates,items,item_bases,affixes,loot,maps}.txt`; `custom_assets/maps/*.spawns`; spawn-writing parts of `tools/artgen/mapgen.py`/`valemap.py`; `docs/items.md`, `docs/combat.md`.
Tasks: author all data files above (read legacy rows once read-only to re-author faithfully: spells 81, 82, 89, 95, 229, 67, 275, starter items); `items::generate_grid` with level-band tiers; `maps::load` from maps.txt only, server starts on `default=1` map; glade spawns → 50020-50023; replacement class skills; remove every GameDb call from server/item.rs/spell.rs; tests (no unknown keys, formulas evaluate, all spell/start_item/junk/loot/NPC-spell refs resolve, grid entry round-trips, skill_tests and loot test run without legacy).

### Stream B: visuals
Owns: `content/{visuals,particles,sprite_fx}.rs`; `dusk_formats/src/{psi.rs,sprite_fx.rs,sprite_anim.rs}`; client `{particles.rs,spell_particles.rs,spell_fx.rs,lights.rs,env_light.rs,map_render.rs}`; `custom_assets/data/{spell_visuals.txt,particles.txt}`; `custom_assets/content/fx/**`; env/vale sprite_fx lines in `enviro.py`/`vale.py`; `tools/artgen/{spellfx.py,lightfx.py}`; delete `spellfx_measure.py`; new `docs/visuals.md`.
Named kits (read exact numbers from `spell_visual_kit` while legacy exists): melee_hit (slash_001 47,30), arrow_flight, arrow_hit (slash_001 47,25), bow_draw, item_use, heavy_slash (slash_002c 47,23), bleed_hit (slash_002b 47,25 blend 3 glow ff00007f), skull_crack (water_001 50,30), stun_ring (wind_003b 13,-height+20), dash (wind_003a 23,5 glow), rundown_hit (slash_001 47,25 glow), ground_slam (earth_002a 93,33), knife_flight (particles knife_trail 0,-20), knife_hit (slash_001 50,25), fire_cast (particles fire_cast 0,-height), ember_trail (particles ember_trail 0,-25), ember_burst (effect_004 48,18 glow), hook_flight (particles hook_trail 0,-20), hook_hit (slash_001 50,25), ember_warmth (light_003 23,-5 glow), veil (effect_003 23,-height/10), warden_sweep (slash_001 ×2 45,19), warden_wind_up.
Spell mappings: 50001 impact heavy_slash; 50002 impact bleed_hit; 50003 impact skull_crack, aura stun_ring; 50004 go dash, impact rundown_hit; 50005 go ground_slam; 50006 traveling knife_flight, impact knife_hit; 50007 casting fire_cast, traveling ember_trail, impact ember_burst, cast_anim cast; 50008 casting bow_draw, traveling hook_flight, impact hook_hit, anims shoot; 50009 casting fire_cast, impact ember_warmth; 50010 impact veil; 50100 impact melee_hit; 50101 casting bow_draw, traveling arrow_flight, impact arrow_hit, cast_anim shoot; 50110/50111 impact item_use, go_anim block; 51001 impact bleed_hit; 51002 impact warden_sweep; 51003 casting warden_wind_up, go ground_slam, aura stun_ring. Keep today's resolved go_anim values. Plus visuals for Stream A's new class skills.
Also: particles text format + 7 defs, remove binary .psi parser; glade fireflies via env sprite_fx (`.psi`-named map sprites spawn effects without a file); `lightfx.py` light textures; lights.rs darkness = `1 - MapInfo.darkness`, zone_night removed; atlas → `fx_particles.png`; remove `CustomVisual`; tests (kits resolve .sa/particles/sound; particles parse + simulate).

### Stream C: audio
Owns: `content/sounds.rs`; `dusk_formats/src/sound.rs`; `dusk_formats/tests/sounds.rs` (rewrite); client `audio.rs`, `audio/custom.rs`; `tools/sfxgen/**`; `custom_assets/content/sfx/**`; `custom_assets/data/sprite_sounds.txt`; `docs/audio.md`.
Tasks: generate the 26 builtin + 19 kit + 12 voice sounds; point `builtin` constants at our names (keep identifiers); kit sounds via `resolve_sfx` (bare names, .wav implied); NPC voices custom only; proximity loops from sprite_sounds.txt only; music from `MapInfo.music` or the whole soundtrack; drop RegionGrid zone/area playlists, `custom_playlist` gating, `CUSTOM_MAP_PREFIX` checks (footsteps everywhere), config.ini reading, mp3→ogg fallback; test that every referenced sound exists and decodes.

### Stream D: client presentation and front end
Owns: client `{main.rs,data.rs,settings.rs,menu/**,hud.rs,unit.rs,paper_doll.rs,items_ui.rs,chat.rs,combat_ui.rs,director_ui.rs,windows.rs,nameplates.rs,minimap.rs,journal.rs,spells_ui.rs,dialogue.rs}`; `tools/artgen/{icons.py,iconlib.py,icon_items.py,icon_spells.py,ui.py,portraits.py}`; `custom_assets/content/fonts/**`; `custom_assets/data/help.txt`; `docs/ui.md`.
Tasks: fonts + license + constants; remove `--art`, `DUSK_ART`, `custom_art` (GameData, Settings, Options toggle, restart notice) and `legacy_maps`; always our paper doll/NPC scripts/portraits, delete legacy BODY/PAPER_DOLL lists and portrait_offset.txt; overrides indexed unconditionally; menu map list from maps.txt with the default first; `/help` from data/help.txt; `icons.py` reads our data and generates missing icons (final run after A merges); screenshot every window/menu with the new font and fix overflows.

### Stream E: cleanup (after A-D merged, no other worktrees active)
1. Move the local legacy folder out of the repo first (`mv assets ../duskhollow-legacy`).
2. `git mv custom_assets assets` and flatten: `content/custom/*` → `content/*` (sheets → `content/sprites/`), `override/ui` → `content/ui`, `override/icons` + `custom/icons` → `content/icons/{items,spells}`, `scripts/animation` → `scripts/animation`, `scripts/npc` → `scripts/npc`, `scripts/player` → `scripts/player` (update unit.rs/paper_doll.rs); update .gitignore.
3. `assets_root()` per §2; delete content_root/legacy_root/custom_assets_root, `legacy://`, `DUSK_LEGACY_LOG`; `FileIndex::scan` + duplicate check; drop `DUSK_CUSTOM_ASSETS` from MENU_SAFE_VARS.
4. Delete `crates/dusk_extract`, rusqlite, zip, GameDb + all `impl GameDb`, `db.rs` → `content/types.rs`, `custom::install`, `CUSTOM_MAP_PREFIX`, `CUSTOM_*_FIRST`, legacy tests, config.ini reading.
5. Tools: `tools/artgen/paths.py` (`ASSETS = ROOT/"assets"`); repoint every generator (~20 `custom_assets` refs in artgen + sfxgen).
6. Docs: `docs/formats.md` → `docs/content.md` (only our formats + "how to add a map / NPC / spell / item / sound / effect" checklist for the dungeon); strip Ghidra/`FUN_`/0x refs from docs and code comments (keep enum tables as our enums); README (no extraction step, no dusk_extract).
7. Global test `dusk_formats/tests/content.rs`: every reference resolves (map textures, spawns, NPC/item sprite scripts + `image=` sheets, portraits, icons, kits → .sa/particles/sounds, sprite_fx, sprite_sounds, fonts, music).
8. Rename maps `duskhollow` → `duskhollow`, `glade` → `glade` (settings DEFAULT_MAP, menu VISTA_MAP, director TITLE_MAP, server tests, docs; old settings fall back to the first map).

### Verification (end of E)
Fresh clone with no legacy folder and no `DUSK_ASSETS`/`DUSK_LEGACY`: `cargo build --workspace && cargo test --workspace`; `cargo run -p dusk_server` + `cargo run -p dusk_protocol --example quest_bot -- 127.0.0.1:16383 300` exits 0; client Play works; screenshots of duskhollow and glade with no missing sprite/sheet/texture/sound warnings (`DUSK_AUDIO_LOG=1`); `cargo run -p dusk_protocol --example bot`; `cargo tree | grep -E "rusqlite|zip"` empty; grep for `game\.db|dusk_extract|--art|custom_art|legacy_maps|Ghidra|FUN_00` clean; `git status --ignored` shows only target/, assets/preview/, __pycache__.

## 5. Visible changes
Legacy maps gone (default server start → Duskhollow); legacy class skills gone (replaced by our own per class); new item names, our base set and a smaller affix pool; all combat/UI/spell sounds procedural; new UI font (check overflows); re-authored particles and glows; per-map darkness; "Custom art" and "Legacy maps" options removed.

## Runtime legacy inventory (Stream 0)

Recorded with `DUSK_LEGACY_LOG=1` (each distinct legacy access printed once as `[legacy] kind: what` on
stderr) on `duskhollow` and `glade` with `--art custom`: arrival, `DUSK_AUTOPLAY=1` fights
(autoplay also casts the action bar) for classes 1-4 on both maps, `DUSK_DIALOGUE_TEST=50010` (Lowshade,
cairn fire), `DUSK_BOSS_TEST=1`, `DUSK_OPEN=character,inventory,abilities,journal`, `DUSK_OPEN_BOOK=1`,
plus the quest bot against a standalone server. Re-run the same set after each stream; whatever a
stream owns must be gone from the log.

**Load time (every launch).** `game.db` tables: `map, teleport_names` + `npc` (maps); `npc_template,
npc_models, npc_models_junkloot`; `spell_template`; `spell_visual, spell_visual_kit`; `player_class_stats,
player_exp_levels, player_create_spell, player_create_item, player_desirable_armor, player_desirable_stats`;
`item_template, affix_template, loot, material_chance_weapon, material_chance_armor`; `sprite_psi,
sprite_light, sprite_hotspot, zone_template.night_pct`; `zone_template, area_template, npc_sounds,
sprite_proximity_sound`. Files: `file_index.txt`, `scripts/particles/*.psi` (whole directory), the 13
legacy `maps/*.map` (server).

**Runtime (our maps).**
| Kind | Seen |
|---|---|
| Fonts | `Friz Quadrata Regular.ttf`, `Friz Quadrata Bold.ttf` |
| Light textures | `content/misc/light_source.png`, `content/misc/shader_light.png` |
| Text | `scripts/sprite/portrait_offset.txt` (always read by the HUD); `scripts/text/help.txt` on `/help`; `config.ini` when present |
| NPC templates | glade: 2 Antling, 8 Spiderite, 13 Crazed One, 14 Venomous One |
| Spells cast | legacy class skills 9 Holy Wrath, 10 Radiance, 13 Mighty Blow, 27 Ignite, 29 Fireball, 31 Chains of Ice, 43 Sinister Strike, 46 Entangling Shot, 60 Mark Target, 69 Holy Bolt, 77 Rejuvenation, 79 Plague; NPC spells 229 Rend (Alpha), 67 Harrowing Strike (Corvin; 275 War Stomp did not fire in these runs). Auto attacks 81/82 and potions 89/95 are not logged (no cast request) but are still legacy |
| Items | starters 13-20, 23, 25; glade junk 50, 53, 54; generated gear (7376, 7377, 10429, 11026, 13827, 13926, 19176, 19376, 19427 ...) |
| Particles | campfire, small_light_embers (cairn fires), green_firefly (glade), casting_fire, casting_frost, casting_holy, cheapshot, entangleshot, fireball, holybolt, scorch |
| Kits, our spells | 50001: 18 impact (slash_002c), 153 aura (disarm.psi); 50002: 179 impact (slash_002b); 50003: 15 impact (water_001), 61 aura (wind_003b); 50004: 6 go (wind_003a), 129 impact (slash_001); 50005: 199 go (earth_002a); 50006: 78 traveling (cheapshot.psi), 132 impact (slash_001); 50007: 119 casting (casting_fire.psi), 122 traveling (scorch.psi), 47 impact (effect_004); 50008: 105 casting, 55 traveling (entangleshot.psi), 126 impact (slash_001); 50009: 119 casting, 98 impact (light_003); 50010: 158 impact (effect_003); 229 Rend: 179 |
| Kits, legacy skills | 13, 14, 17, 32, 33, 38, 42, 52, 81, 90, 99, 113, 114, 119, 121, 125 (go away with the legacy class skills) |
| Sounds, builtin | attack_hit_var01..05.wav, attack_sword_normal_{s,m,m2,h}.ogg, dodge_default.wav, swishverb4.ogg, attack_metal_case.ogg, e3_attack_hardhit01..03.ogg, vdamage3_mlb_4.ogg, alert_levelup_a.ogg, window_open_a.ogg, window_target_open_a.ogg |
| Sounds, kits of our spells | skill_dpatk1_hit.wav, e3_attack_insecthit04.ogg, attack_book_normal_critical.ogg, skill_symbolofvalkyrie.wav, item_att_big_sword.ogg, skill_bleeding_shot.wav, swish_2weapon_02.ogg, magic_cast_fire.ogg, magic_fire_point_fire2.wav, skill_explosivearrow.wav, squish_3.ogg, arrow_fire1.ogg, skill_warpoweratk_hit_a.ogg, skill_heal_poison.wav, skill_mighty_blow_fire.ogg (plus the legacy-skill kit sounds) |
| Sounds, glade NPC voices | shulack_ranger_attack_01/02, shulack_ranger_damage_01, shulack_wizard_damage_01/02, ratman_voice_05_damage, starcrab_voice_02_idle/03_damage/04_die, e3_frillfaimam_die |

No legacy sprite scripts, sheets, portraits, icons, UI art or `.sa` scripts were read with `--art custom`
(flipbooks now always come from `scripts/animation` first).

### Final contract (as implemented)
Every loader takes our content root (`dusk_formats::assets_root()`); legacy access stays inside the
bodies (`content::legacy_db()`, `legacy_root()`).
```rust
// dusk_formats (lib.rs)
pub fn content_root() -> PathBuf;  pub fn legacy_root() -> PathBuf;  pub fn assets_root() -> PathBuf; // = legacy_root
pub const LEGACY_SOURCE: &str = "legacy://";
pub fn legacy_log_enabled() -> bool;  pub fn legacy_note(kind: &str, what: impl Display);
pub fn fs_path(rel: &str) -> PathBuf;                        // legacy://x -> legacy_root/x (logged), else content_root/x
pub fn find_file(root: &Path, rel: &str) -> Option<PathBuf>; // root/rel, else legacy_root/rel (logged)
FileIndex::load_legacy() -> FileIndex;  FileIndex::load_prefixed(path, prefix) -> io::Result<FileIndex>
// content::sections: as planned, plus Section::id_int(&self) -> Option<i64>
// content::rules
pub fn load(root: &Path) -> anyhow::Result<Rules>; // Rules { class_stats: HashMap<(i64,i64),ClassStats>, exp_levels: Vec<ExpLevel>,
//   class_spells: HashMap<i64,Vec<i64>>, start_items: HashMap<i64,Vec<(i64,i64)>>, class_armor: HashMap<i64,Vec<i64>>,
//   desirable_stats: HashMap<i64,Vec<i64>>, class_weapons: HashMap<i64,Vec<i64>> }
// content::npcs
pub fn load(root: &Path) -> anyhow::Result<Npcs>; // Npcs { templates: HashMap<i64,NpcTemplate>, models: HashMap<i64,NpcModel>,
//   loot: HashMap<i64,NpcLoot> /* by template entry */, junk: HashMap<i64,Vec<i64>> /* by model id */ }
// content::spells
pub fn load(root: &Path) -> anyhow::Result<HashMap<i64, SpellTemplate>>;
// content::items
pub fn load(root: &Path) -> anyhow::Result<ItemTables>; // ItemTables { items, affixes, loot_tables: HashMap<i64,Vec<LootRow>>,
//   grid: HashMap<(i64,i64),Vec<i64>>, materials: HashMap<(bool,i64,i64),f32> /* Stream 0 only, A drops it */ }
pub fn generated_grid(items: &HashMap<i64, ItemTemplate>) -> HashMap<(i64, i64), Vec<i64>>;
// content::maps
pub fn load(root: &Path) -> anyhow::Result<Vec<MapInfo>>; // MapInfo gains `default: bool`; the default map's `start` is the start spot
pub fn spawns(root: &Path, map: &MapInfo) -> Vec<NpcSpawn>;
pub fn default_map(maps: &[MapInfo]) -> Option<&MapInfo>;
pub fn map_file(root: &Path, name: &str, ext: &str) -> Option<PathBuf>; // ours, else legacy
pub const CUSTOM_MAP_FIRST: i64 = 10_000;
// content::visuals (KitAnim, VisualKit, SpellVisual, CustomVisual live here; spell.rs re-exports the first three)
pub fn load(root: &Path, spells: &HashMap<i64, SpellTemplate>) -> anyhow::Result<HashMap<i64, SpellVisual>>; // keys ⊆ spells
pub fn flipbook_path(root: &Path, name: &str) -> Option<PathBuf>;
pub fn parse_spell_visuals(sections: &[Section]) -> anyhow::Result<HashMap<i64, CustomVisual>>;
pub const SPELL_VISUAL_KEYS: &[&str];  pub fn unit_anim_id(v: &str) -> i64;
// content::particles
pub fn load(root: &Path) -> anyhow::Result<HashMap<String, ParticleSystemInfo>>; pub fn key(name: &str) -> String; // lowercase, no .psi
// content::sprite_fx
pub fn load(root: &Path) -> anyhow::Result<SpriteFx>; // SpriteFx { psi, lights, hotspots: HashMap<String,(i32,i32)>,
//   roofs: Vec<(String,(i32,i32))>, zone_night: HashMap<u32,f32> /* Stream 0 only, B replaces with MapInfo.darkness */ }
// content::sounds
pub fn load(root: &Path) -> anyhow::Result<SoundTables>;
```
Client: `GameData.root` is the content root; `GameData::{fs_path, find_file, particle_system}`;
`asset_path()` returns content-relative paths or `legacy://...` (Bevy source `legacy` registered in
`main.rs`, logging reader). `ServerConfig.assets` is the content root. Removed: `custom::install`,
`custom::merge_spells`, `CustomSpell`, `custom_assets_root()`, `FileIndex::resolve_path`;
`custom::parse_spells` returns `Vec<SpellTemplate>`. Deviations from the planned signatures:
`ItemTables.materials`, `SpriteFx.zone_night`, `MapInfo.default`, `maps::map_file`, `items::generated_grid`.
