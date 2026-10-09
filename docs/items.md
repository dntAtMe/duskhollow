# Items, inventory, equipment, loot

Implementation: `crates/dusk_formats/src/item.rs` (tables + stat formulas, shared by server and
client tooltips), `crates/dusk_server/src/items.rs` (inventories, equip rules, item use, loot),
client `items_ui.rs` (windows, tooltips, loot) and `paper_doll.rs` (gear sprite layers).

Tags as in [combat.md](combat.md): **DATA** our data files, **TEXT** in-game text,
**DESIGN** our choice.

## Data (`assets/data`, read by `dusk_formats::content::items` / `rules` / `npcs`)

| File | Use |
|---|---|
| `items.txt` | hand-made items `[entry]`: 1-99 consumables / quest items (1 Ember Draught, 2 Lamp Tonic), 10-19 starter gear, 100-199 junk, 1000-9999 named items. Keys: `name icon model equip_type weapon_type armor_type material quality required_level item_level stack spell stat=<key>:<amount> sell_price flags description` (names or numbers for the enums below) |
| `item_bases.txt` | random gear `[base N]`: `name model icon` (one value or five, for qualities 2..6), `equip_type`, `weapon_type` / `armor_type`, `material`, `levels=lo-hi` |
| `affixes.txt` | random enchantments `[N]`: `name` ("Noun" or "Prefix Noun"), `noun=1` ("of the"), `level=lo-hi`, `stat=<key>:<factor>` |
| `loot.txt` | hand-made loot tables `[loot N]`: `item=<entry>,<chance %>,<min>-<max>` |
| `classes.txt` | per class: `start_item=<entry>,<count>`, `armor=` families, `weapons=` (incl. `shield`), `stats=` its loot favours |
| `npc_templates.txt` | per NPC: `junk=<items>`, `loot=<table>`, `loot_chances=green,blue,gold,purple`, `gold_ratio=` (-1 = default) |

Random gear is generated from the bases: every base becomes one item per quality 2..6 and
level of its band, entry `100000 + base·1000 + quality·100 + level`
(`dusk_formats::item::grid_entry` / `grid_parts`). 130 bases: 15 armour tiers x head, chest,
legs, feet, hands; pendant, belt, band (levels 1-25); 7 metal tiers x axe, mace, sword, dagger;
7 wood tiers x staff, wand, bow; 3 shields. The band is the tier: higher armour types and
weapon materials only exist (and so only drop) at higher levels. Names, models and icons can
change with quality (a bog-iron Shortsword is a Longsword at green, a Zweihander at blue).
64 affixes: 16 stat themes ("of the Hauler" Strength, "of the Vigil" Willpower, "of Embers"
Mana, "of the Shade" shadow resistance ...) in 4 level bands (1-5 plain, 6-13 "Steady",
14-19 "Grim", 20-25 "Unblinking"), factors 1.7 / 2.1 / 2.5 / 3.0 (attributes), 0.28 .. 0.5
(weapon value), 0.57 .. 1.0 (crit, ranged weapon value).

Stat keys: `mana health armor strength agility willpower intelligence courage regeneration
meditate weapon_value melee_speed ranged_weapon_value ranged_speed melee_crit ranged_crit
spell_crit dodge block resist_frost resist_fire resist_shadow resist_holy`.

### Enums

- **equip_type**: 1 Head, 2 Neck, 3 Chest, 4 Belt, 5 Legs, 6 Feet, 7 Hands, 8 Ring, 9 Weapon,
  10 Shield, 11 Ranged
- **weapon_type**: 1 Axe, 2 Bow, 3 Mace, 4 Sword, 5 Staff, 6 Dagger, 7 Wand
- **armor_type**: 1 basic cloth, 2-4 leather, 5-8 chain, 9-11 plate, 12-15 mage cloth (tiers
  inside each family; shields use 4-8)
- **weapon_material**: 1-7 metals (Bronze ... Titanium), 8-14 woods (Aspen ... Hickory)
- **quality**: 1 junk (grey), 2 plain (white, all starting gear), 3 green, 4 blue, 5 gold,
  6 purple. The loot chance columns are named green/blue/gold/purple, i.e. 3..6.
- **flags**: `no_save`, `no_trade`, `no_arena`, `no_group_dungeon`, `quest_item`, `skillbook`,
  `gold_value_scales` (bits 1, 2, 4 ... 64)
- **Equipment slots** (ours, laid out like `equipment.png`): 0 Head, 1 Neck, 2 Chest, 3 Ranged,
  4 Hands, 5 Weapon, 6 Ring1, 7 Ring2, 8 Offhand, 9 Belt, 10 Legs, 11 Feet. Bags: 49 slots
  (the 7x7 grid of `inventory.png`).

## Item numbers (DESIGN)

The tooltip strings (`%d Weapon Value  Speed %.2f`, `%d Armor Value`, `Equip: Increases your %s
by %d.`, `Durability %d/%d`, `Requires level`) are TEXT. `lvl` = required level, `q` = quality
multiplier (junk 0.7, plain 1.0, green 1.1, blue 1.2, gold 1.3, purple 1.5).

| Number | Formula |
|---|---|
| Weapon speed (s) | Dagger 1.6, Wand 1.8, Sword 2.0, Axe 2.2, Bow 2.4, Mace 2.4, Staff 2.8 |
| Weapon value | `(2 + lvl) · speed · q · (1 + 0.04 · material tier)` (DPS independent of speed) |
| Armour | `(12 + 12·lvl) · family · q · (1 + 0.05 · tier) · slot` — family cloth 0.4, leather 0.6, chain 0.8, plate 1.0; slot chest .30, legs .22, head .16, feet/hands .12 (a level-25 plate set ≈ 300 AV, the TEXT armour cap) |
| Shield | armour weight .30, block rating `5 + 2·lvl·q` |
| Affix stat | `factor · (1 + lvl/8) · m`, m = 1 / 1.25 / 1.5 / 2 for green / blue / gold / purple; Health and Mana x10 |
| Generated items | durability 40 / 60 / 80 / 100 / 120 by quality (shown, no wear yet); sell price `(6 + 10·lvl) · slot · q'` (slot weapon 1.5, chest/shield 1.2, legs 1, head/trinkets 0.8, feet/hands 0.6; q' 1 .. 1.8) |

Item names with an affix: `"<Prefix> <Item> of (the) <Noun>"` ("the" with `noun=1`), e.g.
"Grim Riveted Hauberk of the Warden".

## Player stats from gear

`stats::player_stats(class_stats, gear)`: attributes, Health, Mana, Weapon Value, crit, dodge,
block and resistances add the item bonuses; armour and block come from items only. The main-hand
weapon sets weapon value and swing speed; a bow does so only if no melee weapon is equipped
(DESIGN: auto attacks are melee). Unarmed: `1 + lvl` at 2.0 s (DESIGN). Changing gear keeps
current HP/Mana (clamped to the new maximum).

## Equip rules

- Level: `required_level`. `required_class` when set.
- Weapons and shields: the class's `weapons=` list (`data/classes.txt`): Vanguard axes, maces,
  swords, daggers, shields; Emberwright staves, wands; Cutthroat bows, daggers; Ashpriest staves,
  maces, shields.
- Body armour: the class's `armor=` families plus basic cloth for everyone (every class starts
  in it): Vanguard leather, mail, plate; Cutthroat leather, mail; Emberwright and Ashpriest
  cloth and robes.
- Rings go to the first free ring slot. Equipping swaps the old item into the bag slot.
- Starting items (`start_item=`): equipped when possible, the rest (5 Ember Draughts, 5 Lamp
  Tonics) into the bags.
- Potions: `UseItem` casts the item's `spell` on yourself, consuming one (DESIGN: consumed when
  the cast is queued; refused while the spell is on cooldown, 60 s). Ember Draught: 9 health
  every 2 s for 20 s; Lamp Tonic: 11 mana every 2 s for 20 s.

## Loot

On a kill credited to a player (the last attacker), the corpse rolls (all DESIGN unless noted):

1. **Gold** 60%: `level · U(1, 3) · gold_ratio%` (default 100), x2 elite, x5 boss.
2. **Junk** 40%: one of the NPC's `junk=` items with the closest `item_level`.
3. **Loot table**: every row of the NPC's `loot=` table rolls its chance, count in `[min, max]`
   (Corvin: Gatewatch Mace, Thirty-One Winters, Post Gorget, Ember Draughts; the Glarewolf
   Alpha: Alpha's Fang Charm).
4. **Gear**, at most one piece: purple, gold, blue, green chances from `loot_chances` or
   0.1 / 0.4 / 1.5 / 6% (x2 elite, x5 boss), else 8% plain. The piece is a generated item of
   that quality with required level = NPC level (max 25); 75% of drops are restricted to what
   the killer's class can use; slot type uniform, then a base whose level band covers the level;
   green+ get a random affix from the level band, 75% of the time one whose stats are all in
   the class's `stats=`.

Only the killer may loot (no parties yet). A corpse with loot stays 60 s (instead of 8 s) and
disappears 3 s after being emptied. Range 3 cells. The server tells the killer `Lootable`;
the client shows a pouch over the corpse. `OpenLoot` -> `LootWindow`; `TakeLoot { index }`
takes one entry, `index: None` takes everything incl. gold. What doesn't fit stays on the corpse.

## Protocol

Client: `EquipItem`, `UnequipItem`, `UseItem`, `DestroyItem`, `OpenLoot`, `TakeLoot`.
Server: `Inventory` (full snapshot after every change), `Appearance { id, gear }` (equipped
entries per slot, to the whole map; also sent to newcomers for existing players), `CombatStats`,
`Lootable`, `LootWindow`, `ItemError`, `Received` (loot log / chat line).

## Client

- `I` Inventory, `C` Character window (equipment, level/XP, attributes, combat numbers).
  Click a bag item: equip / use; shift + right-click: destroy. Click an equipment slot: unequip.
- Tooltips: quality-coloured name, slot/type, weapon value + speed, armour, block, affix bonuses,
  "Use:" effect, durability, required level (red if too high), sell price.
- Paper doll: naked body (`scripts/player/custom_body.txt`) + gear layers (`scripts/player/<model>.txt`)
  in the order legs, feet, chest, hands, head, shield, weapon (bow if no melee weapon), DESIGN.
- Debug: `DUSK_OPEN_INVENTORY=1` opens both windows, `DUSK_TOOLTIP_ITEM=<n>` / `e<n>` forces the
  tooltip of bag / equipment slot n, `DUSK_AUTOPLAY=1` also loots, `DUSK_LOOT_WINDOW=1` leaves loot
  windows open instead.

## Not yet

Vendors (sell prices are in the data), sockets / gems / orbs, durability loss, soulbinding, item persistence, moving items between
bag slots, quest-conditioned loot, party loot rules, two-handed weapons, female paper dolls.
