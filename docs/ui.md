# HUD

All HUD pieces use the original art from `content/interface/` at native pixel size (the
original targets 1920x1080; at our default 1280x720 the pieces are placed so they don't overlap).

| Module | What |
|---|---|
| `hud.rs` | Player / target unit frames, portraits, XP bar, `HudConfig` (config.ini options) |
| `chat.rs` | Chat panel, text entry, slash commands, system lines, speech bubbles |
| `nameplates.rs` | Names + health bars over units |
| `minimap.rs` | Minimap (render-to-texture), unit dots, zoom; owns the render-layer split |
| `ui_input.rs` | `UiInputCaptured`: shared "the UI owns the keyboard / pointer" flags |
| `combat_ui.rs` | Floating combat text, death notice, click targeting (unchanged rules) |

## Front end (`state.rs`, `menu/`, `settings.rs`)

App states: `Boot` -> `Menu` -> `Connecting` -> `InGame`, back to `Menu` on Quit to Menu, a lost
connection or a refused/failed join (the reason shows on the panel the player came from). A
command-line launch (map / `--connect` / `--name` / `--class`, or a `DUSK_*` debug variable) goes
`Boot` -> `Connecting` directly and quits on disconnect, as before the menu.

- Gameplay systems run only `InGame` (`state::in_game`); HUD pieces spawn on `OnEnter(InGame)`.
  When a session ends, every root entity with a `Transform` / `Node` created since it started is
  despawned (map tiles, audio and `menu::KeepOnMenu` roots excepted), `Net` / `PlayerState` /
  `OfflineServer` (the embedded server, stopped on drop) are removed, and gameplay resources are
  reset with `state::reset` on the next `OnEnter(Connecting)`.
- The menu backdrop is the real `custom_duskhollow` map around Lowshade's cairn: the camera drifts,
  the Eye's openness breathes (`GazeView`, so the crimson grade and lit sprites follow), embers rise,
  a dithered vignette darkens edges and scrims the title / buttons. The custom soundtrack plays
  because the audio listener falls back to the camera when there is no player.
- Screens: main (Play, Join Server, Options, Quit), Play / Join (name, class medallions with
  role and level-1 stats from `player_class_stats`, start map / server address + recent servers),
  Connecting (cancel), Options (Sound, Display, Game, Controls tabs) and the in-game game menu.
- Game menu: Esc opens it only when nothing else wants Esc (`menu::EscProbe::esc_free`: chat input,
  dialogue / end card, own cast, loot window, selected target). It is an overlay: the world keeps
  running; while open it takes the keyboard (`ButtonInput` reset, `KeyboardInput` drained before chat)
  and the pointer (`CapturesPointer`).
- `settings.ini`: see `settings.rs` for the keys and location (`DUSK_SETTINGS` overrides).

## Unit frames

`UnitFrame::setFrameStyle` (`FUN_0052f460`, `UnitFrame.cpp`) hard-codes
the layout per style. Recovered offsets (frame-local pixels):

| Field | Style 1 (player, `unit_frame.png`) | Style 2 (target, `unit_frame_reverse.png`) | Style 3 (party) |
|---|---|---|---|
| HP bar (`unit_frame_hp*.png`) | 74,39 | 3,39 | 45,11 |
| MP bar (`unit_frame_mp*.png`) | 84,70 | 23,70 | 55,33 |
| Level badge (`+0x100`) | 71,105 | 330,112 | 51,54 |
| Portrait centre (`+0x150`), radius (`+0x15c`) | 44,73 r45 | 328,73 r45 | 44,73 r13 |
| HP / MP text (`+0x110`, `+0x118`) | 91,46 / 91,74 | 250,46 / 250,74 | 66,13 / 66,33 |
| Aura row (`+0x90`) | 95,108 | 265,108 | 76,58 |
| Cast bar (style 2 only, `+0x120`) | – | 300,10 | – |

Bars are clipped by percentage (a clip node shrinks, the bar image stays put); the reverse
style drains towards the portrait. Elite/boss targets (`npc_template.bool_elite/bool_boss`)
get the `unit_frame_elite/boss.png` dragon ring; its circle centre is (65.5, 75.5) in the art,
fitted by least squares, and is placed on the portrait centre. We draw the level badge at
(330,108) and names above the HP bar (the original name position wasn't identified).

Target mana: the server doesn't send NPC mana; the bar is shown full for casters
(`ai_type = 1` or `mana > 0`) and empty otherwise.

### Portraits

Portraits are cards of 210x330 (`content/portraits/npc_ports`, `ports_1..5`) plus 80x80
faction placeholders (`portraits/portrait_{hostile,friendly,grey}.png`). The client bakes a
round 78 px thumbnail on the CPU: a 130 px square around the face, where the face height of
player portraits comes from `scripts/sprite/portrait_offset.txt` (`name=y`), else y=115.

NPC lookup: `portrait_<npc_template.portrait>.png` (13 templates set it), else
`portrait_<npc model name>.png` (covers most monsters: goblin, wolf, skeleton...), else the
faction placeholder. The player uses `portrait_male (90).png` until character creation exists.

## XP bar

`xp_bar.png` (633x12) above the action bar; a dimmed copy is the empty track. Hover shows
"Experience x / y".

## Chat

`game_chat_backdrop.png` (537x237) bottom-left, 70 px up so it clears the action bar at
1280x720. Log area 16,10 484x184 (14 lines visible, 200 kept), input row at y 201,
`game_chat_enter_*` button, `game_chat_fullup/up/down/fulldown_*` scroll buttons; mouse wheel
over the panel scrolls.

- `Enter` opens the input, `Enter` sends, `Escape` cancels. Plain text and `/say`, `/s` send
  `ClientMsg::Chat` (the server relays it map-wide, including back to us); `/help` prints
  `scripts/text/help.txt`; other commands from that list answer "not available yet".
- System lines: welcome, level up, death. Other modules post with the `ChatSystemLine` message.
- Speech bubbles: `saybox_*` 9-slice (4 px corners) in a 3x3 UI grid, max 220 px wide, over the
  speaking player for 5 s + 40 ms per character.
- While the input is open `UiInputCaptured::keyboard` is true (and stays true for the frame that
  closes it), so movement, action-bar keys, `P`, `Esc` and minimap keys are ignored.

## Name plates

`nameplate_bg.png` (102x12, fill well x 2..100, y 2..10) with `nameplate_hp.png` (hostile/neutral)
or `nameplate_hp_party.png` (friendly, players) cropped to the health ratio, plus the name
above in Friz Quadrata 13 px: hostile red, neutral yellow, friendly/players green. Children of
the unit, counter-scaled, drawn at local z 500 so they sit above the world.

Visibility follows the original `config.ini` `[System]` defaults:

| Option | Default | Effect |
|---|---|---|
| `EnemyNameplateTick` | 1 | bars over hostile/neutral NPCs |
| `FriendlyNameplateTick` | 0 | bars over friendly NPCs / players |
| `YourNameTick` | 1 | our own name |
| `YourNameplateTick` | 0 | our own bar |
| `ShowNpcNameTick` / `ShowPlayerNameTick` | 1 | names |

Other units' bars also show while targeted or damaged. `HudConfig` reads `<assets>/config.ini`
if present (`dusk_extract` doesn't copy it; drop the install's file there to override).

## Minimap

`minimap.png` (241x293) top-right; the map view is the 230x227 rectangle at (6,46) that
`miniamp_decal.png` covers. The shipped `content/minimap/fanadin_map.jpg` (3200x1800) is a
pre-rendered overview of Fanadin only, so instead a second `Camera2d` renders the live world
around the player into a 2x supersampled texture (linear-filtered down). The decal's alpha is
inverted into a dark overlay so the map fades out at the frame's brushed edges.

- Dots: `minimap_enemy` (hostile), `minimap_neutral`, `minimap_friendly` (friendly NPCs and
  players), `minimap_dead` (corpses), 18x18. The player is a generated arrow rotated to our facing.
- Zoom: 3 / 4.5 / 6 / 9 / 13 world px per minimap px; `MinimapZoom` (config.ini `[UI]`,
  default 1) picks the start. Mouse wheel over the minimap, numpad `+` / `-`, or the
  `minimap_button` (cycles).

### Render layers and cameras

- Layer 0: the world (map tiles, units, spell effects) — both cameras.
- Layer 1 (`minimap::OVERLAY_LAYER`): world-space overlays (name plates, floating combat text) —
  main camera only. Use `minimap::overlay_layer()` for new world-space UI.
- The game camera carries `player::MainCamera` (and `IsDefaultUiCamera`). Query
  `With<MainCamera>`, never `With<Camera2d>` (there are two 2D cameras).

## Not done yet

Full-screen map (`M`, `mapgui.png`), party frames (style 3), combat indicator
(`unitframe_combat.png`), target-of-target, cast bar on the target frame, chat channels/tabs,
whisper/yell routing, player portraits chosen at character creation.
