# Duskhollow

An isometric online ARPG in Rust + [Bevy](https://bevyengine.org/), with its own authoritative
server, procedurally generated oldschool pre-rendered art and an original soundtrack.

The engine reads a legacy data pack (`game.db`, `.map` files, sprite archives; formats in
[docs/formats.md](docs/formats.md)) that is **not** distributed here. Everything under
`custom_assets/` (sprites, portraits, tiles, icons, UI skin, spell effects, music, the
`custom_glade` map) is original and progressively replaces legacy visuals with `--art custom`.

## Setup

```bash
# 1. Unpack a legacy data pack into ./assets
cargo run -p dusk_extract --release -- <DATA_DIR>

# 2a. Offline: the client runs the server embedded (optionally pick a start map / class 1-4)
cargo run -p dusk_client
cargo run -p dusk_client -- goblin_cave --class 2

# 2b. Online: start the server, then any number of clients
cargo run -p dusk_server
cargo run -p dusk_server -- --start-map goblin_cave   # optional: different start map
cargo run -p dusk_client -- --connect 127.0.0.1:16383 --name Alice

# Headless bot: joins, pathfinds to the nearest attackable NPC and fights
# (DUSK_AUTOPLAY=1 makes the GUI client do the same)
cargo run -p dusk_protocol --example bot
```

Controls: `WASD` move, left-click an enemy to walk up and auto-attack, `1`-`0` `-` `=` cast from the action bar, `P` Abilities window (click a spell, then a slot to place it; right-click a slot to clear), `Esc` cancel cast / clear target, `Enter` chat (`Enter` send, `Esc` cancel, `/help`), mouse wheel over chat / minimap scrolls / zooms, numpad `+`/`-` minimap zoom, `M` toggle music, `N` toggle sound effects, `I` Inventory (click gear to equip, potions to drink; shift + right-click destroys), `C` Character window (click a slot to unequip), left-click a corpse with a pouch over it to loot.

Debug aids (env vars): `DUSK_SCREENSHOT=out.png` (+ `DUSK_SCREENSHOT_AT=secs`), `DUSK_AUTOPLAY=1`, `DUSK_OPEN_BOOK=1`, `DUSK_TOOLTIP_SLOT=n`, `DUSK_CHAT=text` (say `text`, then leave it typed in the input).

## Workspace

| Crate | Role |
|---|---|
| `dusk_formats` | Engine-agnostic parsers: `.map`, sprite scripts, `.sa`, `.psi` (+ simulation), `game.db` |
| `dusk_extract` | Unpacks the install into `assets/` + builds `file_index.txt` |
| `dusk_client` | Bevy client (rendering, input, UI) |
| `dusk_protocol` | Client/server messages, framing, TCP transport |
| `dusk_server` | Authoritative server (lib + bin): maps, NPC AI (wander/aggro/chase/leash), melee combat, XP, respawn |

Own art: `tools/artgen` generates oldschool pre-rendered sprites (`cargo run -p dusk_client -- --art custom`), see [tools/artgen/README.md](tools/artgen/README.md).

File format notes: [docs/formats.md](docs/formats.md). Combat rules and recovered enums: [docs/combat.md](docs/combat.md). Music, ambience and sound effects: [docs/audio.md](docs/audio.md). HUD layout (unit frames, chat, name plates, minimap): [docs/ui.md](docs/ui.md).

## Roadmap

1. ✅ Asset extraction, format parsers, map + NPC + paper-doll rendering, WASD movement
2. ✅ Map terrain + walk flags (Ghidra), `.psi` particles (map sprites, spell kits), `sprite_light` glows + zone darkness. Next: clouds overlay, unit/spell glows
3. ✅ `dusk_protocol` + networking; server owns NPC spawns, wandering, validates movement
4. 🟡 Combat: ✅ melee, aggro/leash, death/respawn, XP/levels, HUD, spells (casts, cooldowns, damage/heal/weapon/aura effects, NPC casting), action bar, Abilities window, tooltips, `.sa` spell visuals, kit particles, sounds — next: remaining effect types, spell ranks
5. 🟡 Items: ✅ item templates, inventory, equipment + stats, paper doll, starting gear, potions, NPC loot (tables, junk, random affixed gear, gold), loot window — next: vendors, sockets/gems, durability, persistence ([docs/items.md](docs/items.md))
6. 🟡 UI: ✅ unit frames with portraits (elite/boss rings), XP bar, chat + speech bubbles, name plates, minimap — next: full-screen map, party frames, chat channels, quests, gossip, `.bscript` buttons
7. ✅ Audio: zone/area/map music + ambience with crossfades, melee/spell/NPC-voice/UI sound effects, sprite proximity loops, volume keys — next: footsteps, item/object sounds, stereo panning
8. Persistence (characters), parties, guilds, arena, dungeons
