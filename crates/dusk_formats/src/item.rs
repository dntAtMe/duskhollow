//! Items: templates, affixes, loot rows and the item stat formulas.
//!
//! The data (`data/items.txt`, `data/item_bases.txt`, `data/affixes.txt`, `data/loot.txt`, read by
//! `crate::content::items`) only says *what* an item is (slot, weapon/armour type, material,
//! quality, required level); the numbers the tooltips show (weapon value, armour, affix bonuses)
//! come from the DESIGN formulas below (see `docs/items.md`). They live here so the server
//! (stats) and the client (tooltips) can never disagree.

use std::collections::HashMap;

/// `item_template.equip_type` (names from the client's `EquipType` enum strings / item names).
pub mod equip {
    /// `equip_type=` names in the data files.
    pub const NAMES: [(&str, i64); 11] = [
        ("head", HEAD),
        ("neck", NECK),
        ("chest", CHEST),
        ("belt", BELT),
        ("legs", LEGS),
        ("feet", FEET),
        ("hands", HANDS),
        ("ring", RING),
        ("weapon", WEAPON),
        ("shield", SHIELD),
        ("ranged", RANGED),
    ];
    pub const HEAD: i64 = 1;
    pub const NECK: i64 = 2;
    pub const CHEST: i64 = 3;
    pub const BELT: i64 = 4;
    pub const LEGS: i64 = 5;
    pub const FEET: i64 = 6;
    pub const HANDS: i64 = 7;
    pub const RING: i64 = 8;
    pub const WEAPON: i64 = 9;
    pub const SHIELD: i64 = 10;
    pub const RANGED: i64 = 11;
}

/// `item_template.weapon_type` (from item names/models: "Axe"/hand_axe, "Shortbow"...).
pub mod weapon {
    pub const AXE: i64 = 1;
    pub const BOW: i64 = 2;
    pub const MACE: i64 = 3;
    pub const SWORD: i64 = 4;
    pub const STAFF: i64 = 5;
    pub const DAGGER: i64 = 6;
    pub const WAND: i64 = 7;
    /// Not a `weapon_type` of any item: in a class's weapon list it allows shields.
    pub const SHIELD: i64 = 10;
    /// `weapon_type=` / class `weapons=` names in the data files.
    pub const NAMES: [(&str, i64); 8] = [
        ("axe", AXE),
        ("bow", BOW),
        ("mace", MACE),
        ("sword", SWORD),
        ("staff", STAFF),
        ("dagger", DAGGER),
        ("wand", WAND),
        ("shield", SHIELD),
    ];
}

/// `item_template.quality`. Loot chances in `npc_template` are named green/blue/gold/purple,
/// which are qualities 3..=6; 2 is plain (all starting gear), 1 is grey junk.
pub mod quality {
    pub const JUNK: i64 = 1;
    pub const COMMON: i64 = 2;
    pub const GREEN: i64 = 3;
    pub const BLUE: i64 = 4;
    pub const GOLD: i64 = 5;
    pub const PURPLE: i64 = 6;
    /// `quality=` names in the data files.
    pub const NAMES: [(&str, i64); 6] =
        [("junk", JUNK), ("common", COMMON), ("green", GREEN), ("blue", BLUE), ("gold", GOLD), ("purple", PURPLE)];
}

/// Armour families of `armor_type` for class `armor=` lists: `cloth` 1, `leather` 2-4, `mail`
/// 5-8, `plate` 9-11, `robe` 12-15.
pub const ARMOR_FAMILIES: [(&str, std::ops::RangeInclusive<i64>); 5] =
    [("cloth", 1..=1), ("leather", 2..=4), ("mail", 5..=8), ("plate", 9..=11), ("robe", 12..=15)];

/// A number, or a name from `names`.
pub fn lookup(names: &[(&str, i64)], v: &str) -> Option<i64> {
    let v = v.trim();
    v.parse().ok().or_else(|| names.iter().find(|(n, _)| *n == v).map(|(_, id)| *id))
}

/// `Stat` enum ids used by `stat_typeN` / affixes (see `docs/combat.md`).
pub mod stat {
    pub const MANA: i64 = 1;
    pub const HEALTH: i64 = 2;
    pub const ARMOR_VALUE: i64 = 3;
    pub const STRENGTH: i64 = 4;
    pub const AGILITY: i64 = 5;
    pub const WILLPOWER: i64 = 6;
    pub const INTELLIGENCE: i64 = 7;
    pub const COURAGE: i64 = 8;
    pub const REGENERATION: i64 = 9;
    pub const MEDITATE: i64 = 10;
    pub const WEAPON_VALUE: i64 = 11;
    pub const RANGED_WEAPON_VALUE: i64 = 13;
    pub const MELEE_CRITICAL: i64 = 15;
    pub const RANGED_CRITICAL: i64 = 16;
    pub const SPELL_CRITICAL: i64 = 17;
    pub const DODGE_RATING: i64 = 18;
    pub const BLOCK_RATING: i64 = 19;
    pub const RESIST_FROST: i64 = 21;
    pub const RESIST_FIRE: i64 = 22;
    pub const RESIST_SHADOW: i64 = 23;
    pub const RESIST_HOLY: i64 = 24;

    /// `stat=` keys in the data files (items, affixes, class stats).
    pub const KEYS: [(&str, i64); 23] = [
        ("mana", MANA),
        ("health", HEALTH),
        ("armor", ARMOR_VALUE),
        ("strength", STRENGTH),
        ("agility", AGILITY),
        ("willpower", WILLPOWER),
        ("intelligence", INTELLIGENCE),
        ("courage", COURAGE),
        ("regeneration", REGENERATION),
        ("meditate", MEDITATE),
        ("weapon_value", WEAPON_VALUE),
        ("melee_speed", 12),
        ("ranged_weapon_value", RANGED_WEAPON_VALUE),
        ("ranged_speed", 14),
        ("melee_crit", MELEE_CRITICAL),
        ("ranged_crit", RANGED_CRITICAL),
        ("spell_crit", SPELL_CRITICAL),
        ("dodge", DODGE_RATING),
        ("block", BLOCK_RATING),
        ("resist_frost", RESIST_FROST),
        ("resist_fire", RESIST_FIRE),
        ("resist_shadow", RESIST_SHADOW),
        ("resist_holy", RESIST_HOLY),
    ];

    /// A stat id by key (or number).
    pub fn from_key(k: &str) -> Option<i64> {
        super::lookup(&KEYS, k)
    }

    /// Display name, as in the tooltips.
    pub fn name(stat: i64) -> &'static str {
        match stat {
            1 => "Mana",
            2 => "Health",
            3 => "Armor Value",
            4 => "Strength",
            5 => "Agility",
            6 => "Willpower",
            7 => "Intelligence",
            8 => "Courage",
            9 => "Regeneration",
            10 => "Meditate",
            11 => "Weapon Value",
            12 => "Melee Speed",
            13 => "Ranged Weapon Value",
            14 => "Ranged Speed",
            15 => "Melee Critical",
            16 => "Ranged Critical",
            17 => "Spell Critical",
            18 => "Dodge",
            19 => "Block",
            21 => "Frost Resistance",
            22 => "Fire Resistance",
            23 => "Shadow Resistance",
            24 => "Holy Resistance",
            25 => "Bartering",
            26 => "Lockpicking",
            28 => "Staves",
            29 => "Maces",
            30 => "Axes",
            31 => "Swords",
            32 => "Ranged",
            33 => "Daggers",
            34 => "Wands",
            35 => "Shields",
            38 => "Parry Chance",
            39 => "Block Chance",
            40 => "Dodge Chance",
            _ => "Unknown",
        }
    }

    /// Tooltip phrasing: the original has "rating", plain and "skill" variants.
    pub fn equip_line(stat: i64, amount: i32) -> String {
        match stat {
            15..=19 | 21..=24 => format!("Equip: Increases your {} rating by {amount}.", name(stat)),
            25..=35 => format!("Equip: Increases your {} skill by {amount}.", name(stat)),
            _ => format!("Equip: Increases your {} by {amount}.", name(stat)),
        }
    }
}

/// Number of equipment slots / the slot layout of the Character window
/// (left column top-down, then right column; matches the icons on `equipment.png`).
pub const EQUIP_SLOTS: usize = 12;
pub mod slot {
    pub const HEAD: usize = 0;
    pub const NECK: usize = 1;
    pub const CHEST: usize = 2;
    pub const RANGED: usize = 3;
    pub const HANDS: usize = 4;
    pub const WEAPON: usize = 5;
    pub const RING1: usize = 6;
    pub const RING2: usize = 7;
    pub const OFFHAND: usize = 8;
    pub const BELT: usize = 9;
    pub const LEGS: usize = 10;
    pub const FEET: usize = 11;

    pub const NAMES: [&str; super::EQUIP_SLOTS] =
        ["Head", "Neck", "Chest", "Ranged", "Hands", "Weapon", "Ring", "Ring", "Offhand", "Belt", "Legs", "Feet"];
}
/// Bag size: the 7x7 grid on `inventory.png`.
pub const BAG_SLOTS: usize = 49;

/// Equipment slots an `equip_type` may go into (rings: either ring slot).
pub fn slots_for(equip_type: i64) -> &'static [usize] {
    match equip_type {
        equip::HEAD => &[slot::HEAD],
        equip::NECK => &[slot::NECK],
        equip::CHEST => &[slot::CHEST],
        equip::BELT => &[slot::BELT],
        equip::LEGS => &[slot::LEGS],
        equip::FEET => &[slot::FEET],
        equip::HANDS => &[slot::HANDS],
        equip::RING => &[slot::RING1, slot::RING2],
        equip::WEAPON => &[slot::WEAPON],
        equip::SHIELD => &[slot::OFFHAND],
        equip::RANGED => &[slot::RANGED],
        _ => &[],
    }
}

#[derive(Debug, Clone, Default)]
pub struct ItemTemplate {
    pub entry: i64,
    pub name: String,
    pub icon: String,
    pub sound: String,
    /// Paper-doll layer: `scripts/player/<gender>/<model>.txt` (empty / "0" = none).
    pub model: String,
    pub required_level: i64,
    pub weapon_type: i64,
    pub armor_type: i64,
    pub equip_type: i64,
    pub weapon_material: i64,
    pub num_sockets: i64,
    pub quality: i64,
    /// Only set on junk; equipment uses `required_level`.
    pub item_level: i64,
    pub durability: i64,
    pub sell_price: i64,
    /// Max stack size (1 = not stackable).
    pub stack_count: i64,
    /// 0 = any class.
    pub required_class: i64,
    pub flags: i64,
    /// Part of the level x material x quality grid used for random loot.
    pub generated: bool,
    /// On-use spells (potions).
    pub spells: Vec<i64>,
    /// Explicit `(stat, value)` pairs (hand-made uniques).
    pub stats: Vec<(i64, i64)>,
    pub description: String,
}

impl ItemTemplate {
    pub fn is_equippable(&self) -> bool {
        !slots_for(self.equip_type).is_empty()
    }

    pub fn max_stack(&self) -> u32 {
        self.stack_count.max(1) as u32
    }

    /// Item level for formulas: `required_level`, else `item_level`, at least 1.
    pub fn level(&self) -> i32 {
        (if self.required_level > 0 { self.required_level } else { self.item_level }).max(1) as i32
    }

    pub fn has_model(&self) -> bool {
        !self.model.is_empty() && self.model != "0"
    }
}

/// `item_template.flags`: names from the client's `ItemFlag_*` strings; bit order assumed to
/// follow the string order (scrolls carry 96 = Skillbook | GoldValueScales, which fits).
pub mod flags {
    pub const NO_SAVE: i64 = 1;
    pub const NO_TRADE: i64 = 2;
    pub const NO_ARENA: i64 = 4;
    pub const NO_GROUP_DUNGEON: i64 = 8;
    pub const QUEST_ITEM: i64 = 16;
    pub const SKILLBOOK: i64 = 32;
    pub const GOLD_VALUE_SCALES: i64 = 64;
}

/// A random "<prefix> <item> of (the) <noun>" enchantment (`data/affixes.txt`).
/// `stats` values are per-level scaling factors, not flat amounts (1.5..3.0 for attributes,
/// ~0.3..0.5 for Weapon Value), growing with the affix's level band.
#[derive(Debug, Clone, Default)]
pub struct Affix {
    pub entry: i64,
    pub name: String,
    /// `noun=1` -> "of the X", else "of X".
    pub single_noun: bool,
    pub min_level: i64,
    pub max_level: i64,
    pub stats: Vec<(i64, f32)>,
}

impl Affix {
    /// Splits "Undying Dolphin" into ("Undying", "Dolphin"); one-word affixes have no prefix.
    pub fn parts(&self) -> (Option<&str>, &str) {
        match self.name.split_once(' ') {
            Some((pre, noun)) => (Some(pre), noun),
            None => (None, self.name.as_str()),
        }
    }
}

/// "Undying Shiv of the Dolphin". Mirrors the client's " of the " / " of " strings.
pub fn display_name(t: &ItemTemplate, affix: Option<&Affix>) -> String {
    let Some(a) = affix else { return t.name.clone() };
    let (pre, noun) = a.parts();
    let of = if a.single_noun { "of the" } else { "of" };
    match pre {
        Some(p) => format!("{p} {} {of} {noun}", t.name),
        None => format!("{} {of} {noun}", t.name),
    }
}

/// Everything an item contributes when equipped.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemStats {
    pub weapon_value: i32,
    /// Swing interval of a weapon (0 = not a weapon).
    pub speed_ms: u32,
    pub armor: i32,
    pub block: i32,
    /// Flat `(stat, amount)` bonuses (explicit stats + affix).
    pub bonuses: Vec<(i64, i32)>,
}

/// DESIGN: base swing interval per weapon type, seconds. Weapon value scales with it so
/// damage per second only depends on level/quality.
pub fn weapon_speed(weapon_type: i64) -> f32 {
    match weapon_type {
        weapon::DAGGER => 1.6,
        weapon::WAND => 1.8,
        weapon::SWORD => 2.0,
        weapon::AXE => 2.2,
        weapon::BOW => 2.4,
        weapon::MACE => 2.4,
        weapon::STAFF => 2.8,
        _ => 2.0,
    }
}

/// DESIGN: base numbers scale with quality.
fn quality_mult(q: i64) -> f32 {
    match q {
        quality::JUNK => 0.7,
        quality::GREEN => 1.1,
        quality::BLUE => 1.2,
        quality::GOLD => 1.3,
        quality::PURPLE => 1.5,
        _ => 1.0,
    }
}

/// DESIGN: affix strength by quality (green items get the plain affix).
fn affix_quality_mult(q: i64) -> f32 {
    match q {
        quality::BLUE => 1.25,
        quality::GOLD => 1.5,
        quality::PURPLE => 2.0,
        _ => 1.0,
    }
}

/// Armour family of an `armor_type` (cloth, leather, chain, plate) and the tier inside it.
/// From the models: 1 = basic cloth, 2-4 leather, 5-8 chain, 9-11 plate, 12-15 mage cloth.
pub fn armor_family(armor_type: i64) -> (f32, i64) {
    match armor_type {
        2..=4 => (0.6, armor_type - 2),
        5..=8 => (0.8, armor_type - 5),
        9..=11 => (1.0, armor_type - 9),
        12..=15 => (0.4, armor_type - 12),
        _ => (0.4, 0),
    }
}

/// DESIGN: share of a full set's armour per slot (sums to 0.92 without the shield).
fn armor_slot_weight(equip_type: i64) -> f32 {
    match equip_type {
        equip::CHEST => 0.30,
        equip::LEGS => 0.22,
        equip::HEAD => 0.16,
        equip::FEET | equip::HANDS => 0.12,
        equip::SHIELD => 0.30,
        _ => 0.0,
    }
}

/// Stat amount of one affix entry on an item of `level` / `quality`. DESIGN.
pub fn affix_amount(stat_id: i64, factor: f32, level: i32, q: i64) -> i32 {
    let mut v = factor * (1.0 + level as f32 / 8.0) * affix_quality_mult(q);
    // Health / Mana pools are ~10x larger than attributes.
    if matches!(stat_id, stat::HEALTH | stat::MANA) {
        v *= 10.0;
    }
    v.round().max(1.0) as i32
}

/// The DESIGN item formulas (see module docs / `docs/items.md`).
pub fn item_stats(t: &ItemTemplate, affix: Option<&Affix>) -> ItemStats {
    let lvl = t.level() as f32;
    let qm = quality_mult(t.quality);
    let mut s = ItemStats::default();
    match t.equip_type {
        equip::WEAPON | equip::RANGED => {
            let speed = weapon_speed(t.weapon_type);
            // Materials: 1-7 metals, 8-14 woods; higher tiers drop at higher levels.
            let tier = ((t.weapon_material - 1).max(0) % 7) as f32;
            s.speed_ms = (speed * 1000.0) as u32;
            s.weapon_value = ((2.0 + lvl) * speed * qm * (1.0 + 0.04 * tier)).round().max(1.0) as i32;
        }
        equip::SHIELD => {
            let tier = (t.armor_type - 4).clamp(0, 4) as f32;
            s.armor =
                ((12.0 + 12.0 * lvl) * armor_slot_weight(equip::SHIELD) * qm * (1.0 + 0.05 * tier)).round() as i32;
            s.block = (5.0 + 2.0 * lvl * qm).round() as i32;
        }
        equip::HEAD | equip::CHEST | equip::LEGS | equip::FEET | equip::HANDS => {
            let (fam, tier) = armor_family(t.armor_type);
            let total = (12.0 + 12.0 * lvl) * fam * qm * (1.0 + 0.05 * tier as f32);
            s.armor = (total * armor_slot_weight(t.equip_type)).round().max(1.0) as i32;
        }
        _ => {}
    }
    for &(st, v) in &t.stats {
        if st > 0 && v != 0 {
            add_bonus(&mut s.bonuses, st, v as i32);
        }
    }
    if let Some(a) = affix {
        for &(st, f) in &a.stats {
            if st > 0 && f > 0.0 {
                add_bonus(&mut s.bonuses, st, affix_amount(st, f, t.level(), t.quality));
            }
        }
    }
    s
}

fn add_bonus(v: &mut Vec<(i64, i32)>, stat: i64, amount: i32) {
    match v.iter_mut().find(|(s, _)| *s == stat) {
        Some((_, a)) => *a += amount,
        None => v.push((stat, amount)),
    }
}

/// Class restrictions (`data/classes.txt`): weapons and shields from the class's `weapons=` list
/// (`weapon::SHIELD` allows shields), body armour from its `armor=` list plus basic cloth
/// (armor_type 1) for everyone, since every class starts in it. Classes missing from the
/// lists may use anything.
pub fn class_can_use(
    class: i64,
    t: &ItemTemplate,
    allowed_armor: &HashMap<i64, Vec<i64>>,
    allowed_weapons: &HashMap<i64, Vec<i64>>,
) -> bool {
    if t.required_class > 0 && t.required_class != class {
        return false;
    }
    let weapons = allowed_weapons.get(&class);
    match t.equip_type {
        equip::WEAPON | equip::RANGED => weapons.is_none_or(|w| w.contains(&t.weapon_type)),
        equip::SHIELD => weapons.is_none_or(|w| w.contains(&weapon::SHIELD)),
        equip::HEAD | equip::CHEST | equip::LEGS | equip::FEET | equip::HANDS => {
            t.armor_type <= 1 || allowed_armor.get(&class).is_none_or(|v| v.contains(&t.armor_type))
        }
        _ => true,
    }
}

/// First generated item entry.
pub const GRID_FIRST: i64 = 100_000;

/// Entry of a generated item: base `N` of `data/item_bases.txt`, quality 2..=6, level 1..=99.
pub fn grid_entry(base: i64, quality: i64, level: i64) -> i64 {
    GRID_FIRST + base * 1000 + quality * 100 + level
}

/// (base, quality, level) of a generated entry ([`grid_entry`]).
pub fn grid_parts(entry: i64) -> Option<(i64, i64, i64)> {
    let rest = entry.checked_sub(GRID_FIRST).filter(|r| *r >= 1000)?;
    let (base, q, level) = (rest / 1000, rest % 1000 / 100, rest % 100);
    ((quality::COMMON..=quality::PURPLE).contains(&q) && (1..=99).contains(&level)).then_some((base, q, level))
}

/// One row of a loot table (`data/loot.txt`, `[loot N]` = an NPC's `loot=N`).
#[derive(Debug, Clone)]
pub struct LootRow {
    pub loot_id: i64,
    pub item: i64,
    /// Percent.
    pub chance: f32,
    pub count_min: i64,
    pub count_max: i64,
    /// Gated by quest/state conditions (legacy tables only; ours have none).
    pub conditional: bool,
}

/// Loot settings of an NPC template (`-1` = default).
#[derive(Debug, Clone, Copy)]
pub struct NpcLoot {
    /// Percent chances for green / blue / gold / purple drops (`loot_chances=`), -1 = default.
    pub chances: [f32; 4],
    pub custom_loot: i64,
    /// Percent of the default gold drop, -1 = default (100).
    pub gold_ratio: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmpl(equip_type: i64, level: i64) -> ItemTemplate {
        ItemTemplate { equip_type, required_level: level, quality: quality::COMMON, ..Default::default() }
    }

    #[test]
    fn names_with_affixes() {
        let t = ItemTemplate { name: "Shiv".into(), ..Default::default() };
        let a = Affix { name: "Undying Dolphin".into(), single_noun: true, ..Default::default() };
        assert_eq!(display_name(&t, Some(&a)), "Undying Shiv of the Dolphin");
        let a = Affix { name: "Novice".into(), single_noun: true, ..Default::default() };
        assert_eq!(display_name(&t, Some(&a)), "Shiv of the Novice");
        let a = Affix { name: "Herculean Perseverance".into(), single_noun: false, ..Default::default() };
        assert_eq!(display_name(&t, Some(&a)), "Herculean Shiv of Perseverance");
    }

    #[test]
    fn weapon_value_scales_with_speed_level_quality() {
        let mut sword = tmpl(equip::WEAPON, 1);
        sword.weapon_type = weapon::SWORD;
        sword.weapon_material = 1;
        let s = item_stats(&sword, None);
        assert_eq!((s.weapon_value, s.speed_ms), (6, 2000));
        sword.required_level = 10;
        sword.quality = quality::PURPLE;
        assert!(item_stats(&sword, None).weapon_value > 30);
    }

    #[test]
    fn plate_set_reaches_the_armor_cap_at_max_level() {
        let total: i32 = [equip::HEAD, equip::CHEST, equip::LEGS, equip::FEET, equip::HANDS]
            .iter()
            .map(|&e| {
                let mut t = tmpl(e, 25);
                t.armor_type = 11;
                item_stats(&t, None).armor
            })
            .sum();
        assert!((280..=360).contains(&total), "{total}");
    }

    #[test]
    fn affix_bonuses_merge() {
        let mut t = tmpl(equip::RING, 25);
        t.quality = quality::GREEN;
        let a = Affix { stats: vec![(stat::STRENGTH, 3.0), (stat::STRENGTH, 3.0)], ..Default::default() };
        let s = item_stats(&t, Some(&a));
        assert_eq!(s.bonuses.len(), 1);
        assert_eq!(s.bonuses[0].1, 2 * affix_amount(stat::STRENGTH, 3.0, 25, quality::GREEN));
    }

    #[test]
    fn class_rules() {
        let armor = HashMap::from([(2, vec![1, 12, 13, 14, 15])]);
        let weapons = HashMap::from([(2, vec![weapon::STAFF, weapon::WAND]), (3, vec![weapon::BOW, weapon::DAGGER])]);
        let mut bow = tmpl(equip::RANGED, 1);
        bow.weapon_type = weapon::BOW;
        assert!(class_can_use(3, &bow, &armor, &weapons));
        assert!(!class_can_use(2, &bow, &armor, &weapons));
        let mut plate = tmpl(equip::CHEST, 1);
        plate.armor_type = 9;
        assert!(!class_can_use(2, &plate, &armor, &weapons));
        plate.armor_type = 1;
        assert!(class_can_use(2, &plate, &armor, &weapons));
        let shield = tmpl(equip::SHIELD, 1);
        assert!(!class_can_use(3, &shield, &armor, &weapons));
        assert!(class_can_use(1, &shield, &armor, &weapons), "unlisted classes use anything");
    }

    #[test]
    fn grid_entries_round_trip() {
        for (b, q, l) in [(1, 2, 1), (130, 6, 25), (77, 4, 13)] {
            assert_eq!(grid_parts(grid_entry(b, q, l)), Some((b, q, l)));
        }
        assert_eq!(grid_entry(3, 4, 5), 103_405);
        assert_eq!(grid_parts(1), None);
        assert_eq!(grid_parts(100_150), None);
    }

    #[test]
    fn names_resolve() {
        assert_eq!(lookup(&equip::NAMES, "ranged"), Some(equip::RANGED));
        assert_eq!(lookup(&weapon::NAMES, "7"), Some(weapon::WAND));
        assert_eq!(lookup(&quality::NAMES, "nope"), None);
        assert_eq!((stat::from_key("spell_crit"), stat::from_key("resist_holy")), (Some(17), Some(24)));
    }
}
