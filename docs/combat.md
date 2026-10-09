# Combat

The original server is not shipped and the client contains no combat code
(only `Shared/GameMap.cpp`, `MapLogic.cpp`, `ItemDefiner.cpp`, `Config.cpp`,
`MutualObject.cpp` are shared). Rules therefore come from three places:

| Tag | Source |
|---|---|
| **DATA** | our data files (`custom_assets/data`, `dusk_formats::content`) |
| **TEXT** | the in-game stat tooltips, `scripts/text/stats/*.txt` |
| **DESIGN** | our choice where nothing above says anything; tune freely |

Implementation: `crates/dusk_server/src/stats.rs` (formulas, unit-tested),
`combat.rs` (swings, death, XP, respawn, regen), `ai.rs` (aggro/chase/leash).

## Enums recovered from the client

The client's built-in DB editor registers every enum as `(id, name)` pairs
(`FUN_005039d0`); extracted by `scratchpad/re/enums.py`-style parsing of
`MOV EDX,id … PUSH "name"` sequences.

- **Faction** (`npc_template.faction`): 0 PlayerDefault, 1 Friendly, 2 Neutral, 3 Hostile
- **AI type** (`npc_template.ai_type`): 0 MeleeAI, 1 CasterAI, 2 ArcherAI
- **Hit result**: 1 Miss, 2 Resist, 3 Evade, 4 Dodge, 5 Block, 6 Parry, 7 Crit, 8 Immune
- **Spell effect** (`spell_template.effectN`): 1 SchoolDamage, 2 Teleport, 3 ApplyAura,
  4 ManaDrain, 5 HealthDrain, 6 Heal, 7 Resurrect, 8 CreateItem, 10 SummonNpc,
  11 RestoreMana, 13 Dispel, 14 WeaponDamage, 17 ManaBurn, 18 Threat, 19 TriggerSpell,
  22 InterruptCast, 23 SummonObject, 24 ScriptEffect, 25 KnockBack, 26 ApplyAreaAura,
  27 HealPct, 28 RestoreManaPct, 29 TeleportForward, 30 MeleeAtk, 31 RangedAtk,
  32 LootEffect, 33 Kill, 34 Gossip, 35 Inspect, 36 ApplyGemSocket, 37 Charge, 38 Duel,
  39 SlideFrom, 40 ApplyOrbEnchant, 41 LearnSpell, 42 NearestWp, 43 PullTo,
  44 DestroyGems, 45 CombineItem, 46 ExtractOrb, 47 ApplyBeyondEnchant
- **Aura type** (`effectN_data1` when effect = ApplyAura): 1 PeriodicDamage, 2 PeriodicHeal,
  3 InflictMechanic, 4 ModifyStat, 5 ModifyStatPct, 6 AbsorbDamage, 8 ModifyResistance,
  9 PeriodicTriggerSpell, 10 PeriodicRestoreMana, 11 ModifyMoveSpeedPct,
  12 MechanicImmunity, 13 SchoolImmunity, 14 ModifyDmgDealtPct, 15 ModifyDmgReceivedPct,
  16 ModifyMeleeSpeedPct, 17 ModifyRangedSpeedPct, 18 PeriodicMeleeDamage, 19 Model,
  20 PeriodicBurnMana, 21 Proc, 22 ModifyHealingDealtPct, 23 ModifyHealingRecvPct,
  24 PeriodicHealPct, 25 PeriodicRestoreManaPct, 26 RepopOntopOfSelf
- **Target type** (`effectN_targetType`): 1 Unit_Caster, 2 Unit_Friendly,
  3 Unit_AreaSrc_Friendly, 4 Unit_AreaDst_Friendly, 11 Misc, 13 Target_GameObject,
  14 Unit_Hostile, 15 Unit_AreaSrc_Hostile, 16 Unit_AreaDst_Hostile, 17 Unit_Any,
  18 Unit_AreaSrc_Friendly_FromDst, 19 Unit_AreaDst_Hostile_FromDst, 20 Target_Item
- **Stat**: 1 Mana, 2 Health, 3 ArmorValue, 4 Strength, 5 Agility, 6 Willpower,
  7 Intelligence, 8 Courage, 9 Regeneration, 10 Meditate, 11 WeaponValue,
  12 MeleeCooldown, 13 RangedWeaponValue, 14 RangedCooldown, 15 MeleeCritical,
  16 RangedCritical, 17 SpellCritical, 18 DodgeRating, 19 BlockRating, 21 ResistFrost,
  22 ResistFire, 23 ResistShadow, 24 ResistHoly, 25 Bartering, 26 Lockpicking,
  28 StaffSkill, 29 MaceSkill, 30 AxesSkill, 31 SwordSkill, 32 RangedSkill,
  33 DaggerSkill, 34 WandSkill, 35 ShieldSkill, 38 ParryChanceBonus,
  39 BlockChanceBonus, 40 DodgeChanceBonus, 43 NpcMeleeSkill, 44 NpcRangedSkill
- **Mechanic**: 1 Confused, 2 Pacify, 3 Fear, 4 Root, 5 Silence, 6 Sleep, 7 Snare,
  8 Stun, 9 Incapacitated, 11 Polymorph, 13 Stealth, 14 Disrupt
- **Spell attribute** (bit flags in `spell_template.attributes`, bit numbering TBD):
  1 CanTargetDead, 2 CantCrit, 3 IgnoreArmor, 4 IgnoreStun, 5 IgnoreIncapacitated,
  6 IgnoreSleep, 7 IgnoreInvulnerability, 8 IgnoreLOS, 9 IgnoreResistances, 12 IgnoreConfused,
  13 IgnoreFear, 14 IgnorePolymorph, 15 ImpossibleBlock, 16 ImpossibleDodge,
  17 ImpossibleMiss, 18 ImpossibleParry, 19 NoHealBonus, 20 NoSpellBonus, 21 NoThreat,
  22 NoAggro, 23 NotInCombat, 24 OnePerCaster, 25 OnePerTarget, 26 Passive,
  27 SameStackForAllCasters, 28 TargetNotInCombat, 29 Triggered, 30 CantTargetSelf,
  31 AnimLockStart, 32 AutoApproach, 34 TargetsGround, 35 NoGoLock, 36 TargetsItem,
  37 DontStopCastingSound, 38 TargetPlayersOnly, 39 MouseoverTargeting,
  40 PersistsThroughDeath, 41 NotInArena, 42 NotInDungeon, 43 HalfDurationPlayers

Spell formulas (`mana_formula`, `effectN_scale_formula`, `duration_formula`) use
`clvl`, `splvl`, `value` and `+ - * /` without spaces (`scripts/text/STF_*.txt`).

## Melee rules

| Rule | Value | Tag |
|---|---|---|
| Weapon value | equipped weapon (`docs/items.md`) + item bonuses + Strength / 2; unarmed `1 + lvl` | TEXT (½ Str) / DESIGN |
| Damage roll | weapon value × U(0.8, 1.2) | DESIGN |
| Armour | −50% × min(AV / 300, 1) | TEXT |
| Miss | 5% + 2% per level defender is above attacker | DESIGN |
| Dodge | DodgeRating / attacker level %, cap 30% | TEXT (cap DESIGN) |
| Parry | half damage; ParryRating / attacker level %, cap 25% | TEXT (chance DESIGN) |
| Block | −⅔ damage; BlockRating / attacker level %, cap 50% | TEXT |
| Crit | MeleeCritical / defender level %, ×1.5, cap 50% | TEXT (×1.5 DESIGN) |
| Swing interval | NPC `melee_speed_ms`; players: weapon speed (2000 unarmed) | DATA / DESIGN |
| Melee range | 1.6 cells | DESIGN |

## NPCs

- Templates: `data/npc_templates.txt` (`[entry]`, entries from 50000; each brings its own model,
  `id = entry`). Level: uniform in `level=min-max` per spawn. Spawns: `maps/<name>.spawns`.
- Health `-1`: `20 + 30·lvl` (×2.5 elite, ×6 boss); weapon value `-1`: `3 + 2·lvl`
  (×1.5 elite, ×2 boss). DESIGN; explicit values win.
- Hostile NPCs aggro within 5 cells; neutral ones only fight back. Friendly NPCs can't be attacked.
- Chase via A* (`dusk_formats::path`), leash at `leash_range` (default 20) → evade home,
  immune while evading, heal to full on arrival.
- Corpse 8 s, then respawn after 60 s.

## Players

- Classes (`data/classes.txt`): 1 Vanguard, 2 Emberwright, 3 Cutthroat, 4 Ashpriest. Health
  and mana per level are formulas, `base · clvl · (59 + clvl) / 60` (bases 75/30, 40/70,
  60/45, 45/65); attributes are flat (Str/Agi/Wil/Int/Cou 15/10/5/5/15, 5/5/15/20/10,
  10/15/10/10/10, 10/5/20/15/5).
- Crit rating = (Agi + Cou) / 2, dodge = Agi / 2, parry = Str / 3 (DESIGN); armour, block, swing speed and attribute bonuses come from equipped items (`docs/items.md`).
- XP (`data/exp.txt`): `kill_exp` of the victim's level, ±10% per level difference,
  nothing for victims 5+ levels below. Level up at `exp`; 25 levels (100, 100, 123, 150, 195,
  265 ... to advance from levels 1-6).
- Death: revive at the start point after 5 s with full HP/mana (DESIGN).
- Regen out of combat (5 s without swings): 5% per 2 s players, 10% NPCs (DESIGN).
- Auto attacks: `Attack` starts swinging (50100 Attack / 50101 Shoot are the spellbook entries,
  `melee_atk` / `ranged_atk`, never cast and kept off the action bar).

## Spells

Implementation: `crates/dusk_server/src/spells.rs`, client `spells_ui.rs` / `spell_fx.rs`,
shared formulas `dusk_formats::spell`.

| Rule | Value | Tag |
|---|---|---|
| Effect values | `effectN_formula` with `value` = data2 (damage/heal/weapon %) or data3 (auras) | DATA |
| ApplyAura fields | data1 = aura type, data2 = mechanic / school mask / stat, data3 = value | DATA |
| Damage/heal roll | ×U(0.9, 1.1) | DESIGN |
| Spell crit | SpellCritical / target level %, ×1.5, cap 50%; players (Int + Cou) / 2 | TEXT / DESIGN |
| Resist | ResistX / attacker level %, half damage, cap 75% | TEXT |
| Periodic auras | formula = total over duration, unless the tooltip says "every" (then per tick) | DESIGN (from descriptions) |
| Range | `range` / 64 cells (130 ≈ melee, 610 ≈ 9.5 cells) | DESIGN |
| Projectiles | travel = distance / (speed × 0.75 cells/s) | DESIGN |
| Global cooldown | 1 s | DESIGN |
| Casting | moving, stuns, or the target dying interrupt | DESIGN |
| Snare/Root/Stun/Sleep/Incapacitate | mechanics 7/4/8/6/9; incapacitate & sleep break on damage | DATA |
| Damage taken % (aura 15) | scales spell and melee damage (wards) | DESIGN |
| Periodic mana (aura 10) | data3 mana per tick (Lamp Tonic) | DESIGN |
| NPC spells | `spellN=spell,chance,interval_ms,cooldown_ms,target_type` while in combat | DATA |
| Charge (effect 37) | dash straight at the target up to 1.1 cells short of it, stopping at the first unwalkable step; the caster is snapped there (`Correct`) | DESIGN |
| Not yet | threat, teleports, items/gameobject targets, summons, dispel, procs, spell ranks (`splvl` = 1) | |

### Spells (`custom_assets/data/spells.txt`)

Every spell is ours: `[entry]` sections of `key=value` (format in `dusk_formats::content::spells`).
Player skills 50001.., auto attacks 50100/50101, item spells 50110.., NPC spells 51001...
`custom_assets/data/class_spells.txt` lists what each class knows (`[class]`, `spell=` lines).
Visuals are in `custom_assets/data/spell_visuals.txt` (`[spell N]` sections,
`dusk_formats::content::visuals`); icons come from `python -I tools/artgen/icons.py --custom-spells`
(`content/custom/icons/spells/skill_<name>.png`).

Aura type **100 ModifyStrainGainPct** is ours: data3 percent applied to positive gaze strain gains
(Draw the Veil: −50 %). Tested end to end in `dusk_server::spells::skill_tests`.

| Skill | Classes | Role |
|---|---|---|
| 50001 Cairnbreaker | V | 0.7 s wind-up, 185 % weapon damage, 9 s |
| 50002 Open Vein | V, C | 60 % weapon + bleed (3 + 3·lvl + Str/3 over 9 s), 8 s |
| 50003 Skullcrack | V, A | 80 % weapon + 2 s stun, 16 s |
| 50004 Run Down | V, C | charge (6 cells) + 70 % weapon + 50 % snare 3 s, 15 s |
| 50005 Clear the Row | V | 70 % weapon to every enemy within 2 cells, 12 s |
| 50006 Flung Blade | V, C | thrown knife, 2 + 2·lvl + Agi/2 physical, 7 cells, 5 s, 1 mana |
| 50007 Ember Bolt | E, A | 1.2 s cast fire bolt, 2 + 0.45·(Int + Cou) + burn, 9 cells, 2 s |
| 50008 Drag-Hook | C | hook on a chain, 3 + lvl + Agi/3 physical + 50 % snare 4 s, 10 s |
| 50009 Kept Ember | all | self heal over 12 s: 6 + 4·lvl + Wil/2, 20 s |
| 50010 Draw the Veil | all | strain gain −50 %, move −15 % for 15 s, 40 s |
| 50011 Hurled Brand | E | 1.8 s cast fire nuke, 3 + 0.8·(Int + Cou), 9 cells, 7 s |
| 50012 Scatter the Coals | E | coals on the target: 1 + 0.15·(Int + Cou), then 2 + 0.3·(Int + Cou) to everything within 2 cells of it (the target too), 30 % snare 3 s, 12 s |
| 50013 Blinding Flare | E | 1 s cast, incapacitate 5 s (breaks on damage), 7 cells, 20 s |
| 50014 Between the Ribs | C | 155 % weapon damage, 12 s |
| 50015 Ember Prayer | A | 1.5 s cast heal on a friend (or yourself): 4 + 3·lvl + (Wil + Int)/2, 9 cells, 3 s |
| 50016 Ash Ward | A | a friend (or yourself) takes 25 % less damage for 10 s, 25 s |
| 50017 Set Your Feet | V | 30 % less damage taken, 30 % slower, 8 s, 30 s |

V Vanguard (8 skills + Set Your Feet), E Emberwright, C Cutthroat, A Ashpriest. Friendly
spells land on the selected target when it is a friend, else on the caster.

| Other spells | |
|---|---|
| 50100 Attack, 50101 Shoot | auto attacks (`melee_atk` / `ranged_atk`), every class |
| 50110 Ember Draught | item 1: 9 health every 2 s for 20 s, 60 s cooldown |
| 50111 Lamp Tonic | item 2: 11 mana every 2 s for 20 s, 60 s cooldown |
| 51001 Rend | Glarewolf Alpha: bleed, 2·lvl over 8 s |
| 51002 Harrowing Sweep | Corvin: 150 % weapon damage, target deals 5 % less for 10 s |
| 51003 Gate Slam | Corvin: 0.5 s wind-up, stuns everything within 4 cells for 4 s |
| 51004 Mandible Nip | Pit Crawler: 75 % weapon damage |

Tooltip numbers: `$E1min`-style tokens are computed client-side from the same formulas and the
attributes in `PlayerStats`.

Visuals: `spell_visual` → kits (`traveling` / `impact` / `casting` / `go`, the last on the caster at release) → `.sa` flipbooks.
`.sa` frames are drawn at scale `1/ratio`; the canvas left edge is `feet.x - spranim_x`, its bottom
is `feet + spranim_y` (y-down; may use `height`). Opaque frames are luma-keyed to emulate the
original's screen/additive blending. Particles (`.psi`) and sounds are not implemented.
