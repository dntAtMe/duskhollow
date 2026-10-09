# HUD

All HUD pieces use our art from `assets/content/ui/` (`tools/artgen/ui.py` and friends) at native
pixel size (laid out for 1920x1080; at our default 1280x720 the pieces are placed so they don't
overlap).

| Module | What |
|---|---|
| `hud.rs` | Player / target unit frames, portraits, XP bar, `HudConfig` (name plate / minimap options) |
| `chat.rs` | Chat panel, text entry, slash commands, system lines, speech bubbles |
| `nameplates.rs` | Names + health bars over units |
| `minimap.rs` | Minimap (render-to-texture), unit dots, zoom; owns the render-layer split |
| `ui_input.rs` | `UiInputCaptured`: shared "the UI owns the keyboard / pointer" flags |
| `combat_ui.rs` | Floating combat text, death notice, low-health warning, click targeting (unchanged rules) |
| `windows.rs` | Window manager (stacking, placement, dragging, close boxes), `Esc` priority (`EscAction`), micro-menu, hint tooltips, cursor shapes |
| `journal.rs` | Quest journal (`J`) |
| `items_ui.rs` | Inventory, Character window (tabs), loot window, item tooltips |

## Fonts

Every UI text uses DejaVu Serif (`combat_ui::UI_FONT`, resource `UiFont`) and DejaVu Serif Bold
for titles (`UI_FONT_BOLD`, `UiFontBold`), shipped in `content/fonts/` with their licence
(`LICENSE_DEJAVU`: Bitstream Vera licence + public-domain DejaVu changes; free to redistribute,
renamed derivatives only). DejaVu runs wider than the condensed MMO faces, so fixed-width labels
are sized for it.

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
- The menu backdrop is the real `duskhollow` map around Lowshade's cairn: the camera drifts,
  the Eye's openness breathes (`GazeView`, so the crimson grade and lit sprites follow), embers rise,
  a dithered vignette darkens edges and scrims the title / buttons. The custom soundtrack plays
  because the audio listener falls back to the camera when there is no player.
- Screens: main (Play, Join Server, Options, Quit), Play / Join (name, class medallions with
  role and level-1 stats from `player_class_stats`, start map / server address + recent servers),
  Connecting (cancel), Options (Sound, Display, Game, Controls tabs) and the in-game game menu.
- Game menu: opens on `windows::pause_menu_requested` (an Esc nothing else consumed, or the
  micro-menu's Menu button). It is an overlay: the world keeps running; while open it takes the
  keyboard (`ButtonInput` reset and `KeyboardInput` drained before chat and `windows::EscSet`, so
  the next Esc only closes it) and the pointer (`CapturesPointer`).
- `settings.ini`: see `settings.rs` for the keys and location (`DUSK_SETTINGS` overrides).

## Unit frames

Fixed layout per frame style (frame-local pixels; `tools/artgen/ui.py` draws the art to match):

| Field | Style 1 (player, `unit_frame.png`) | Style 2 (target, `unit_frame_reverse.png`) | Style 3 (party) |
|---|---|---|---|
| HP bar (`unit_frame_hp*.png`) | 74,39 | 3,39 | 45,11 |
| MP bar (`unit_frame_mp*.png`) | 84,70 | 23,70 | 55,33 |
| Level badge | 71,105 | 330,112 | 51,54 |
| Portrait centre, radius | 44,73 r45 | 328,73 r45 | 44,73 r13 |
| HP / MP text | 91,46 / 91,74 | 250,46 / 250,74 | 66,13 / 66,33 |
| Aura row | 95,108 | 265,108 | 76,58 |
| Cast bar (style 2 only) | – | 300,10 | – |

Bars are clipped by percentage (a clip node shrinks, the bar image stays put); the reverse
style drains towards the portrait. Elite/boss targets (`elite=1` / `boss=1` in `npc_templates.txt`)
get the `unit_frame_elite/boss.png` dragon ring; its circle centre is (65.5, 75.5) in the art,
fitted by least squares, and is placed on the portrait centre. We draw the level badge at
(330,108) and names above the HP bar.

Target mana: the server doesn't send NPC mana; the bar is shown full for casters
(`ai_type = 1` or `mana > 0`) and empty otherwise.

### Portraits

Portraits are our close-ups (`content/portraits/portrait_custom_<model>.png`,
`tools/artgen/portraits.py`) plus 80x80 faction placeholders
(`portrait_{hostile,friendly,grey}.png`). The client bakes a round 78 px thumbnail on the CPU
(80x80 images whole, larger cards: a 130 px square around y=115).

NPC lookup: `portrait_custom_<npc model name>.png`, else
`portrait_<portrait=>.png` (`npc_templates.txt`), else the faction placeholder. The player uses
`portrait_custom_adventurer.png`.

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
  `data/help.txt` (our text: controls and the strain rule; `#` lines skipped); other commands
  answer "not available yet".
- System lines: welcome, level up, death. Other modules post with the `ChatSystemLine` message.
- Speech bubbles: `saybox_*` 9-slice (4 px corners) in a 3x3 UI grid, max 220 px wide, over the
  speaking player for 5 s + 40 ms per character.
- While the input is open `UiInputCaptured::keyboard` is true (and stays true for the frame that
  closes it), so movement, action-bar keys, `P`, `Esc` and minimap keys are ignored.

## Name plates

`nameplate_bg.png` (102x12, fill well x 2..100, y 2..10) with `nameplate_hp.png` (hostile/neutral)
or `nameplate_hp_party.png` (friendly, players) cropped to the health ratio, plus the name
above in the UI font at 13 px: hostile red, neutral yellow, friendly/players green. Children of
the unit, counter-scaled, drawn at local z 500 so they sit above the world.

Visibility (`HudConfig` defaults; no options screen yet):

| Field | Default | Effect |
|---|---|---|
| `enemy_nameplates` | on | bars over hostile/neutral NPCs |
| `friendly_nameplates` | off | bars over friendly NPCs / players |
| `your_name` | on | our own name |
| `your_nameplate` | off | our own bar |
| `show_npc_names` / `show_player_names` | on | names |

Other units' bars also show while targeted or damaged.

## Minimap

`minimap.png` (241x293) top-right; the map view is the 230x227 rectangle at (6,46) that
`miniamp_decal.png` covers. A second `Camera2d` renders the live world
around the player into a 2x supersampled texture (linear-filtered down). The decal's alpha is
inverted into a dark overlay so the map fades out at the frame's brushed edges.

- Dots: `minimap_enemy` (hostile), `minimap_neutral`, `minimap_friendly` (friendly NPCs and
  players), `minimap_dead` (corpses), 18x18. The player is a generated arrow rotated to our facing.
- Zoom: 3 / 4.5 / 6 / 9 / 13 world px per minimap px; `HudConfig::minimap_zoom`
  (default 1) picks the start. Mouse wheel over the minimap, numpad `+` / `-`, or the
  `minimap_button` (cycles).

### Render layers and cameras

- Layer 0: the world (map tiles, units, spell effects) — both cameras.
- Layer 1 (`minimap::OVERLAY_LAYER`): world-space overlays (name plates, floating combat text) —
  main camera only. Use `minimap::overlay_layer()` for new world-space UI.
- The game camera carries `player::MainCamera` (and `IsDefaultUiCamera`). Query
  `With<MainCamera>`, never `With<Camera2d>` (there are two 2D cameras).

## Windows (`windows.rs`)

Toggleable windows are root nodes with `UiWindow(WindowId)`: Character (`C`), Inventory (`I`),
Abilities (`P`), Journal (`J`) and the Loot window (opened by the server's loot reply). The
window manager owns their `Visibility`, `GlobalZIndex` and position; other code asks for changes
with the `WindowCommand` message (`Open` / `Close` / `Toggle`) and reads `Windows::is_open`,
`Windows::top`, `Windows::any_open`.

- Opening a window, or clicking anywhere on it, brings it to the front (z 10 + 2 x stack rank).
- A newly opened window goes to the preferred spot (Character / Abilities left at (16, 118),
  Inventory left of the minimap, Journal centred, Loot centre-left) that overlaps the open windows
  least; if every spot is crowded it cascades by 28 px past windows on the same corner.
- Every window has a title-bar `DragHandle` and a `CloseButton` (`ui_close*`). A dragged window
  keeps its spot for the session.
- `ButtonArt` swaps idle / hover / press images, `HoverTint` lights invisible hot spots laid over
  buttons baked into the art (tabs, "Take All"), `Hint { title, body }` shows a small tooltip after
  0.3 s of hovering.
- Every visible `Button` (not only `CapturesPointer` nodes) now keeps clicks from reaching the world
  (`ui_input.rs`).
- Cursor: hand over buttons and friendly NPCs, crosshair over enemies, grab over lootable corpses
  (system cursors).
- Window open / close play `ui_window_open` / `ui_window_close`.

### Esc priority

`Esc` is resolved once per frame in `PreUpdate` (`windows::EscSet`, after UI focus and the chat /
dialogue keyboard capture) into the `EscAction` resource, and every consumer only acts on its own
variant, so one press does exactly one thing:

1. `TextInput`: the chat input was open (chat cancels it itself).
2. `Card`: the end card was up (any key dismisses it).
3. `CancelCast`: our cast is in progress (`spells_ui`).
4. `CloseWindow(id)`: the most recently opened / focused window closes.
5. `CloseDialogue`: the NPC dialogue closes.
6. `ClearTarget`: the target is dropped (and auto-attack stops).
7. `Unhandled`: nothing in the game wanted it; the pause menu opens.

The micro-menu's menu button produces `EscAction::MenuButton`. The pause menu should run its
"open" system with `.run_if(windows::pause_menu_requested)` (true for `Unhandled` and
`MenuButton`) in `Update`.

### Micro-menu

`ui_micro_bar.png` at the bottom right (6 px from the right edge, 8 from the bottom), five 32 px
`ui_micro_*` plates: Character, Inventory, Abilities, Journal, Menu, each with its key under the
icon and a hint. The plate of an open window stays pressed.

## Character window tabs

`equipment.png` bakes in GENERAL / COMBAT / SKILLS labels (centres x 157 / 296 / 431, y 67..108).
Hot spots of 124x37 over them switch `items_ui::CharTab`; the active label gets a faint crimson
wash and a gold underline, the others are dimmed. The right column (378, 122, 206x432) shows the
tab's page: General (name, health, mana, gold, attributes), Combat (offence, defence,
resistances, with rule hints from [combat.md](combat.md)), Skills (known spells and actions; a
click opens the Abilities window). Equipment slots and the progression column stay on every tab.

## Quest journal (`journal.rs`)

`ui_journal.png` (580x460, `tools/artgen/windows_ui.py`): quest list well inner (24, 74) 186x318,
detail well inner (226, 74) 330x318 with a rule at y 120 under the title, Track button at
(226, 406) 120x30, close box at (530, 23). The list shows Active quests (newest first) and then
Completed ones; the detail shows the objective with progress, the log text for the current status,
"Given by", "Where" / "Turn in" and "Reward". Track / Untrack toggles `director_ui::Untracked`;
the tracker under the minimap shows every active quest that is not untracked, and clicking a
tracker entry opens the journal on it. A newly accepted quest is selected automatically.

The text comes from the server (`QuestInfo.description`, `giver`, `location`, `reward`, written in
`dusk_server::director::script::journal`); on join the server also replays completed quests, which
the client adds silently (no toast).

## Feedback

- Floating combat text has a drop shadow; damage is bone rather than white, crits are bigger, gold
  and get a "!", heals green, incoming red, "+N XP" purple over the player, "Level N" on a level up.
- Below 30 % health the screen edges pulse crimson, faster the lower it gets.
- The death notice is centred with a second line.

## UI audit

Fixed:

| Where | Problem | Fix |
|---|---|---|
| Character | GENERAL / COMBAT / SKILLS tabs were only art | working tabs, pages, hints |
| Character | stats were one long text blob | labelled rows per tab, rule hints |
| Character | close box / Inventory button had no hover or press state | close box art, hover tint, hints |
| Inventory | no way to close it with the mouse | close box |
| Abilities | no close box; clicks fell through to the world (targeting, walking) | close box, window captures the pointer |
| Abilities | tabs gave no feedback | active / dimmed tabs, hover tint |
| Loot | close / Take All gave no feedback | close box art, hover tint, hint |
| All windows | fixed z-order (Abilities < Character < Inventory), fixed spots on top of each other | stacking, focus on click, placement, dragging |
| Esc | one press cancelled the cast, cleared the target and closed loot at once; never closed Inventory / Character / Abilities | `EscAction` priority chain |
| Esc | no way for a pause menu to know Esc was free | `EscAction::Unhandled`, `pause_menu_requested` |
| Quests | no quest journal | journal window (`J`), Track / Untrack, clickable tracker |
| Quests | "Quest complete" toasts replayed for old quests on join | silent resync |
| XP bar | hover text toggled whenever any UI node's interaction changed | only its own |
| Auras | icons rebuilt every 0.25 s, so they could not be hovered; no tooltips | stable icons, spell tooltip with time left |
| Action bar | empty slots had no tooltip; the bar art let clicks through | "Empty slot" hint, bar captures the pointer |
| Equipment | empty slots had no tooltip | slot name |
| Item tooltips | no hint what a click does | "Click to equip / use", "Click to unequip", "Click to take" |
| Dialogue | close box was a text "x" | close box art |
| Chat, minimap | buttons had no tooltips | hints |
| Combat text | pure white, no outline, hard to read on bright ground | shadow, palette colours, crit "!", XP and level-up text |
| Death notice | off-centre single line | centred, two lines |
| Low health | no warning | pulsing crimson edges |
| Cursor | always the arrow | hand / crosshair / grab |
| Micro-menu | none | Character, Inventory, Abilities, Journal, Menu buttons with key labels |

Deferred:

- Full-screen map (no `M` map yet; `M` toggles music), so the micro-menu has no map button.
- Options / pause menu itself (main-menu work stream); the Menu button only raises
  `EscAction::MenuButton`.
- Remembering window positions across sessions (needs the settings file), key rebinding.
- Dragging items between bag slots and spells by drag-and-drop (both are click-to-pick).
- `Esc` does not drop a spell held on the cursor (right-click does).
- Party frames, target-of-target, cast bar on the target frame (see below).

## Not done yet

Full-screen map (`M`, `mapgui.png`), party frames (style 3), combat indicator
(`unitframe_combat.png`), target-of-target, cast bar on the target frame, chat channels/tabs,
whisper/yell routing, player portraits chosen at character creation.
