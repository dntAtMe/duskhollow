# Visuals: spell kits, particles, map effects, lights

Everything here is our own data under `custom_assets/` (no legacy files are read).

| File | What | Loader |
|---|---|---|
| `data/spell_visuals.txt` | named visual kits + which kit each spell plays | `dusk_formats::content::visuals` |
| `data/particles.txt` | particle systems | `dusk_formats::content::particles` (simulation: `dusk_formats::psi`) |
| `content/custom/{env,vale}/sprite_fx.txt` | particle emitters and lights on map sprites | `dusk_formats::content::sprite_fx` |
| `scripts/override/animation/*.sa` + `content/custom/spellfx/` | flipbooks | `tools/artgen/spellfx.py` |
| `content/custom/fx/fx_particles.png` | particle atlas (4x4 cells of 32 px, white) | `tools/artgen/spellfx.py --atlas` |
| `content/custom/fx/fx_light_glow.png`, `fx_light_mask.png` | light glow and darkness cut-out | `tools/artgen/lightfx.py` |

Art rules (docs/world.md): only fire glows; ember, rust, oxblood, dull bone; nothing near-white.

## Spell kits (`data/spell_visuals.txt`)

```text
[kit ember_burst]
anim=effect_004.sa      # flipbook; anim_x = canvas left edge from the feet (+ = left),
anim_x=48               # anim_y = canvas bottom from the feet (+ = down, may use `height`)
anim_y=18
anim_blend=0            # optional; anim_color=rrggbbaa tint; anim2* = a second flipbook
particles=ember_sparks  # optional particle system + offset (px, y down, may use `height`)
particles_y=-height/2
sound=spell_ember_burst # bare name: content/custom/sfx/<name>.wav
unit_glow=f08c283f      # rrggbbaa; colours the orb of a projectile without flipbook/particles

[spell 50007]
casting=fire_cast       # on the caster while casting (particles stop with the cast)
traveling=ember_trail   # rides the projectile (particles stay behind: a trail)
impact=ember_burst      # on each target when the projectile lands (particles: a short burst)
go_anim=swing           # go = on the caster at release; aura = on a unit while the aura lasts
cast_anim=cast          # swing | cast | shoot | cast_alt | block | hit
```

Every spell of `data/spells.txt` needs a `[spell N]` section; an unknown kit name is a load error
(tested: `content::visuals` tests check flipbooks, frames, particles and generated sounds).

| Kit | Flipbook / particles | Sound |
|---|---|---|
| melee_hit | slash_001 (47, 30) | |
| bow_draw | | spell_bow_draw |
| arrow_flight | orb (bone) | spell_arrow_release |
| arrow_hit | slash_001 (47, 25) | spell_arrow_hit |
| item_use | | item_use |
| heavy_slash | slash_002c (47, 23) | spell_heavy_slash |
| bleed_hit | slash_002b (47, 25, blend 3) + blood_spray | spell_bleed_hit |
| skull_crack | water_001 (50, 30) | spell_skull_crack |
| stun_ring | wind_003b (13, -height+20) | |
| dash | wind_003a (23, 5) + dash_dust | spell_dash |
| rundown_hit | slash_001 (47, 25) | spell_blade_hit |
| ground_slam | earth_002a (93, 33) + slam_debris | spell_ground_slam |
| knife_flight | knife_trail (0, -20) | spell_knife_throw |
| knife_hit | slash_001 (50, 25) | |
| hook_flight | hook_trail (0, -20) | spell_hook_throw |
| hook_hit | slash_001 (50, 25) | spell_hook_hit |
| fire_cast | fire_cast (0, -height) | spell_fire_cast |
| ember_trail | ember_trail (0, -25) | spell_ember_whoosh |
| ember_burst | effect_004 (48, 18) + ember_sparks | spell_ember_burst |
| ember_warmth | light_003 (23, -5) + ember_mend | spell_ember_heal |
| veil | effect_003 (23, -height/10) + ash_veil | spell_veil |
| warden_sweep | slash_001 x2 (45, 19 / 47, 29) | spell_warden_sweep |
| warden_wind_up | | spell_warden_roar |

## Particles (`data/particles.txt`)

```text
[campfire]
sprite=0                # atlas cell 0..15 (see the file header for what each cell is)
blend=add               # add | alpha
emission=34             # particles per second
lifetime=-1             # seconds the system emits (<= 0: until stopped; default -1)
life=0.8,1.4            # particle life (s)
direction=0             # degrees, 0 = up on screen, clockwise
spread=50               # full cone width (degrees)
relative=0              # 1: add the emitter's direction of movement
speed=8,20              # px/s
gravity=-38,-20         # px/s^2, + = down
radial=-12,-4           # px/s^2 away from the emitter
tangential=-6,6         # px/s^2 sideways
size=0.46,0.12,0.35     # start, end, variation (x 32 px)
spin=0,0,0              # (not drawn)
color_start=1,0.56,0.16,0.5
color_end=0.5,0.04,0.02,0
color_var=0.3           # how far towards color_end a start colour may be randomised
alpha_var=0.3
```

Size, spin and colour go linearly from start to end over a particle's life. At most 500 live
particles per system. Shipped: `campfire`, `cairn_sparks`, `lantern_embers` (map fires),
`fireflies` (glade ember-moths), `fire_cast`, `ember_trail`, `ember_sparks`, `ember_mend`,
`ash_veil`, `knife_trail`, `hook_trail`, `blood_spray`, `dash_dust`, `slam_debris` (spell kits).
To tune one: `DUSK_FX_TEST=<spell id>` replays a spell's casting/aura particles, its projectile
every second and its impact flipbook; `DUSK_CAMERA_CELL=x,y` pins the camera on a map cell.

## Map effects (`sprite_fx.txt`)

```text
particles <sprite> <system> <x> <y>                            # offset from the sprite image's top-left
light <sprite> <rrggbbaa> <x> <y> <ground 0/1> <top 0/1> <scale>  # offset from the cell's render position
```

Written next to the art by `enviro.py` / `vale.py` (`psi` is accepted for `particles`, a `.psi`
suffix is dropped). A map texture named `*.psi` is an invisible sprite that only carries effects
(hotspot 1, 1): the glade's `green_firefly.psi` sprites carry the `fireflies` system.

A light draws `fx_light_glow.png` tinted with its colour and scaled (`top`: in the sprite's depth
slot; `ground`: at +(16, 8) px above all uprights) and cuts `fx_light_mask.png` (at +(16, 8),
scaled) out of the map darkness. Nearby fires also warm the per-cell light tint (`env_light.rs`).

## Darkness

`maps.txt` `darkness=0..1` (`MapInfo.darkness`, default 0): a black overlay with that alpha,
multiplied by the mask of up to 64 visible lights. `DUSK_DARKNESS=0..1` overrides it for testing.
