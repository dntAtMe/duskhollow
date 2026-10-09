# Demo plan: "First Gaze"

A 10–15 minute playable slice on the map `duskhollow`. Lore and art
rules: [world.md](world.md).

```bash
cargo run -p dusk_client -- duskhollow
```

## The run

1. **Title card** fades in over the gorge: "DUSKHOLLOW", then "Another sword under the Eye."
   The player starts at the south gorge mouth, under open sky.
2. Walk north into **Lowshade** (deep shelter under the overhang). Strain drains. Standing by the
   **rest cairn** binds the player there (respawn point) and clears strain.
3. **Ysolde** (cairnkeeper) gives *The Red Fields*: kill 4 glarewolves harrying the lightworkers.
   Fighting in the open builds strain fast; canopy shelters in the fields slow it, so pulling
   wolves under a canopy is the lesson. Reward: ember draughts.
4. Back to Ysolde, who gives *The Warden at the Glare Gate*: Corvin stood there thirty-one years.
   At the gate the **Eye opens wide** (scripted): the sky flares, strain climbs everywhere, the
   Stooped rise. Defeat **Hollowed Warden Corvin** (boss) and the Eye settles back to half-lidded.
5. Return to Ysolde, then the **end card**: "The Eye half-closes. It never shuts." with time and
   deaths. The player can keep wandering.

Optional: lightworker barks (speech bubbles), the Fallen Blade landmark, a still pool that "looks
back".

## Mechanics (server-authoritative)

**Cover**, per cell, from the map's `.cover` sidecar:

| Char | Kind | Strain per second |
|---|---|---|
| `.` | open sky | +0.35 |
| `s` | shade (canopy, eaves) | −0.6 (+0.15 while fighting) |
| `S` | deep shelter (overhang, roofed lane) | −5 |
| `C` | rest cairn (cell centre of a cairn) | — (cells within 3 of a `C` count as cairn range: −15/s, bind) |

- Combat (dealt or took damage within the last 5 s) multiplies gains ×2.
- Eye states: `Lidded`, `Opening`, `Open`, `Closing`. `Open` doubles gains. The demo opens it at
  the Glare Gate boss; elsewhere it opens on a timer (roughly every 6 min for 45 s).
- **Gaze spot**: while lidded the Eye still sweeps a wandering spot (radius ~5 cells) across open
  ground; inside it, gains ×2.5. The client draws it as a slow pale-crimson wash.
- **Ender resistance**: gains × (1 − min(WIL × 0.5 %, 40 %)).
- **Thresholds**: ≥ 40 *Weary* (regen −50 %), ≥ 70 *Gaze-sick* (no HP regen, move −10 %, damage
  dealt −15 %), 100 *Overwhelmed* (lose 2 % max HP per second, corruption +1 per second).
- **Nobody is fully rested**: outside deep shelter, strain never drops below 10.
- **Corruption** 0–100 builds while Overwhelmed and is only cleared by resting at a cairn
  (slowly). In the demo it is shown, and above 50 the player's portrait/vignette tints.

## Contracts between work streams

- **Map files**: `assets/maps/duskhollow.{map,spawns,cover,markers}`. `.cover` is
  plain text: first line `W H`, then H rows of W chars (row = cell y, column = cell x); parsed by
  `dusk_formats::content::sidecars::CoverGrid`. `.markers` is `name x y [radius]` per line
  (`dusk_formats::content::sidecars::parse_markers`); demo names: `arrival`, `lowshade`, `red_fields`,
  `glare_gate`, `fallen_blade`, `still_pool`.
- **NPC templates**: `assets/data/npc_templates.txt`, `[entry]` sections of `key=value`
  (`dusk_formats::content::npcs`, docs/content.md), entries ≥ 50000, loaded by both server and
  client, with `model=` naming the sprite script in `scripts/npc/`. The demo's entries:

  | Entry | Name | Model | Role |
  |---|---|---|---|
  | 50001 | Glarewolf | `glarewolf` | hostile, packs of 2–3, level 2–3 |
  | 50002 | Glarewolf Alpha | `glarewolf` (larger) | hostile elite, level 4 |
  | 50003 | The Stooped | `stooped` | hostile, slow, level 3–4, risen at the gate |
  | 50004 | Hollowed Warden Corvin | `hollowed_warden` | boss, level 6, at the Glare Gate |
  | 50010 | Ysolde | `cairnkeeper` | friendly quest giver, Lowshade |
  | 50011 | Lightworker | `lightworker` | friendly, wanders the fields, barks |
  | 50012 | Lowshade Guard | `lowshade_guard` | friendly, stands at the hamlet edge |

- **Sound names** (`assets/content/sfx/<name>.wav`, played via `audio::PlaySfx` by
  bare filename): `gaze_open`, `gaze_close`, `gaze_spot_enter`, `strain_heartbeat`,
  `strain_breath`, `strain_whisper`, `strain_overwhelm`, `cairn_kindle`, `cairn_rest`,
  `quest_accept`, `quest_progress`, `quest_complete`, `dialogue_open`, `title_sting`, `end_sting`,
  `hit_heavy`. Loops: `amb_open_sky`, `amb_shelter`, `amb_eye_open`, `loop_cairn_fire`.
  NPC voices by model: `npc_<model>_{aggro,attack,hit,death}.wav`.
- **Client gaze state**: `gaze::GazeView` resource (eye state, openness 0–1, strain, corruption,
  cover under the player, in-combat flag), written by the gaze stream, read by audio and HUD.

## Work streams

1. **Gaze**: protocol messages, server strain/eye/cover/cairn, client `GazeView`, crimson sky grade,
   shade rendering, gaze spot, strain HUD + vignette, minimap eye.
2. **Vale**: environment art in the crimson palette (cliffs and overhangs, covered huts, canopy
   shelters, red grain, cairn, Glare Gate, Fallen Blade), the map with spawns and cover.
3. **Creatures and people**: NPC templates (server + client), models, sheets and portraits
   for the table above.
4. **Sound**: procedural SFX and ambience generator, gaze-driven ambience mixing, NPC voices.
5. **Director and feel**: NPC interaction + dialogue window, quest flow and tracker, scripted Eye
   opening, title and end cards; game feel (hit-stop, screen shake, hit flash, floating damage).
