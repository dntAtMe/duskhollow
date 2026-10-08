# Items, inventory, equipment, loot

Implementation: `crates/dusk_formats/src/item.rs` (tables + stat formulas, shared by server and
client tooltips), `crates/dusk_server/src/items.rs` (inventories, equip rules, item use, loot),
client `items_ui.rs` (windows, tooltips, loot) and `paper_doll.rs` (gear sprite layers).

Tags as in [combat.md](combat.md): **DB** game.db, **TEXT** in-game text / client strings,
**DESIGN** our choice.

## Data (`game.db`)

| Table | Use |
|---|---|
| `item_template` (17k) | name, icon, `model` (paper-doll script), `required_level`, `equip_type`, `weapon_type`, `armor_type`, `weapon_material`, `quality`, `stack_count` (max stack), `spell_1..5` (on use), `stat_typeN/stat_valueN` (only on a few hand-made items), `flags` |
| `affix_template` (7k) | random enchantments: name "<Prefix> <Noun>", level band, up to 5 `(stat, factor)` |
| `player_create_item` | starting items per class (count = stack size) |
| `loot` | hand-made loot tables, `lootId` = `npc_template.custom_loot` |
| `npc_models_junkloot` | npc model -> grey junk items (one per `item_level`) |
| `npc_template.loot_{green,blue,gold,purple}_chance`, `custom_gold_ratio` | per-NPC loot knobs, `-1` = default |
| `material_chance_weapon/armor` | (level, material / armour type) -> chance; which tiers drop at which level |
| `player_desirable_armor/stats` | per class armour types / stats |

Generated equipment (`generated = 1`, 16k rows) is a grid: every base item exists for
qualities 2..6 x required levels 1..25 x materials; `item_level` is only set on junk.

### Enums (from item names/models and the client's enum strings)

- **equip_type**: 1 Head, 2 Neck, 3 Chest, 4 Belt, 5 Legs, 6 Feet, 7 Hands, 8 Ring, 9 Weapon,
  10 Shield, 11 Ranged
- **weapon_type**: 1 Axe, 2 Bow, 3 Mace, 4 Sword, 5 Staff, 6 Dagger, 7 Wand
- **armor_type**: 1 basic cloth, 2-4 leather, 5-8 chain, 9-11 plate, 12-15 mage cloth (tiers
  inside each family; shields use 4-8)
- **weapon_material**: 1-7 metals (Bronze ... Titanium), 8-14 woods (Aspen ... Hickory)
- **quality**: 1 junk (grey), 2 plain (white, all starting gear), 3 green, 4 blue, 5 gold,
  6 purple. The loot chance columns are named green/blue/gold/purple, i.e. 3..6.
- **flags** (bit order assumed from the `ItemFlag_*` string order): NoSave, NoTrade, NoArena,
  NoGroupDungeon, QuestItem, Skillbook, GoldValueScales
- **Equipment slots** (ours, laid out like `equipment.png`): 0 Head, 1 Neck, 2 Chest, 3 Ranged,
  4 Hands, 5 Weapon, 6 Ring1, 7 Ring2, 8 Offhand, 9 Belt, 10 Legs, 11 Feet. Bags: 49 slots
  (the 7x7 grid of `inventory.png`).

## Item numbers (DESIGN)

The original computed them in `Shared/ItemDefiner.cpp` (not recovered); the tooltip strings
(`%d Weapon Value%sSpeed %.2f`, `%d Armor Value`, `Equip: Increases your %s by %d.`, `Durability
%d/%d`, `Requires level`) are TEXT. `lvl` = required level, `q` = quality multiplier
(junk 0.7, plain 1.0, green 1.1, blue 1.2, gold 1.3, purple 1.5).

| Number | Formula |
|---|---|
| Weapon speed (s) | Dagger 1.6, Wand 1.8, Sword 2.0, Axe 2.2, Bow 2.4, Mace 2.4, Staff 2.8 |
| Weapon value | `(2 + lvl) · speed · q · (1 + 0.04 · material tier)` (DPS independent of speed) |
| Armour | `(12 + 12·lvl) · family · q · (1 + 0.05 · tier) · slot` — family cloth 0.4, leather 0.6, chain 0.8, plate 1.0; slot chest .30, legs .22, head .16, feet/hands .12 (a level-25 plate set ≈ 300 AV, the TEXT armour cap) |
| Shield | armour weight .30, block rating `5 + 2·lvl·q` |
| Affix stat | `factor · (1 + lvl/8) · m`, m = 1 / 1.25 / 1.5 / 2 for green / blue / gold / purple; Health and Mana x10 |

Item names with an affix: `"<Prefix> <Item> of (the) <Noun>"` ("the" when `name_single_noun`),
mirroring the client's `" of the "` / `" of "` strings.

## Player stats from gear

`stats::player_stats(class_stats, gear)`: attributes, Health, Mana, Weapon Value, crit, dodge,
block and resistances add the item bonuses; armour and block come from items only. The main-hand
weapon sets weapon value and swing speed; a bow does so only if no melee weapon is equipped
(DESIGN: auto attacks are melee). Unarmed: `1 + lvl` at 2.0 s (DESIGN). Changing gear keeps
current HP/Mana (clamped to the new maximum).

## Equip rules

- Level: `required_level` (DB). `required_class` when set (DB).
- Weapons / shields per class from the class descriptions (TEXT): Paladin — shields, axes, maces,
  swords (+ daggers, "melee weapons"); Mage — staves, wands; Ranger — bows, daggers; Cleric —
  shields, staves, maces.
- Body armour: the class's `player_desirable_armor` types (DB, used as the allowed list — DESIGN)
  plus basic cloth for everyone (every class starts in it).
- Rings go to the first free ring slot. Equipping swaps the old item into the bag slot.
- Starting items: equipped when possible, the rest (potions) into the bags.
- Potions: `UseItem` casts `spell_1` on yourself, consuming one (DESIGN: consumed when the cast is
  queued; refused while the spell is on cooldown).

## Loot

On a kill credited to a player (the last attacker), the corpse rolls (all DESIGN unless noted):

1. **Gold** 60%: `level · U(1, 3) · custom_gold_ratio%` (ratio DB, default 100), x2 elite, x5 boss.
2. **Junk** 40%: one item from `npc_models_junkloot` for the NPC's model with the closest
   `item_level` (DB).
3. **Custom loot** (DB): every `loot` row of `custom_loot` rolls its `chance`, count in
   `[count_min, count_max]`. Rows with quest conditions are skipped until quests exist.
4. **Gear**, at most one piece: purple, gold, blue, green chances from `npc_template` (DB) or
   0.1 / 0.4 / 1.5 / 6% (x2 elite, x5 boss), else 8% plain. The piece is a generated template
   of that quality with required level = NPC level (max 25); 75% of drops are restricted to what
   the killer's class can use; slot type uniform, then material / armour tier weighted by
   `material_chance_*` for that level (DB); green+ get a random affix from the level band (DB),
   75% of the time one whose stats are all in the class's `player_desirable_stats`.

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
- Paper doll: naked body (`default_*`, `head_short`) + gear layers in the order legs, feet, chest,
  hands, head, shield, weapon (bow if no melee weapon) — DESIGN, the original's per-direction order
  is unknown. Skipped for the generated `custom_player` sprite.
- Debug: `DUSK_OPEN_INVENTORY=1` opens both windows, `DUSK_TOOLTIP_ITEM=<n>` / `e<n>` forces the
  tooltip of bag / equipment slot n, `DUSK_AUTOPLAY=1` also loots, `DUSK_LOOT_WINDOW=1` leaves loot
  windows open instead.

## Not yet

Vendors (`npc_vendor`, `npc_vendor_random`, sell prices), sockets / gems / orbs
(`item_gems`, `item_orbs`), durability loss, soulbinding, item persistence, moving items between
bag slots, quest-conditioned loot, party loot rules, two-handed weapons, female paper dolls.
