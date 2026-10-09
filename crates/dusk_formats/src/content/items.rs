//! Item templates, affixes, loot tables and the generated equipment grid.
//!
//! - `data/items.txt`: hand-made items, `[entry]` with `name icon model equip_type weapon_type
//!   armor_type material quality required_level item_level stack spell(repeat)
//!   stat=<key>:<value>(repeat) sell_price flags description`. Entries: 1-99 consumables and quest
//!   items, 10-19 starter gear, 100-199 junk, 1000-9999 named items.
//! - `data/item_bases.txt`: `[base N]` with `name model icon` (one value, or five: qualities 2..6),
//!   `equip_type`, `weapon_type` or `armor_type`, `material`, `levels=lo-hi`. Every base becomes
//!   one item per quality 2..=6 and level in its band ([`generate`], entries from
//!   [`item::grid_entry`]); the band is the base's tier.
//! - `data/affixes.txt`: `[N]` with `name` ("Prefix Noun" or "Noun"), `noun=1` ("of the"),
//!   `level=lo-hi`, `stat=<key>:<factor>` (repeat).
//! - `data/loot.txt`: `[loot N]` with `item=<entry>,<chance %>,<min>-<max>` (repeat).
//!
//! Names: `equip_type` [`item::equip::NAMES`], `weapon_type` [`item::weapon::NAMES`], `quality`
//! [`item::quality::NAMES`], stat keys [`item::stat::KEYS`], flags [`FLAG_NAMES`] (numbers work
//! everywhere too).

use super::sections::{self, Section};
use crate::item::{self, Affix, ItemTemplate, LootRow, equip, quality, stat, weapon};
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct ItemTables {
    pub items: HashMap<i64, ItemTemplate>,
    pub affixes: HashMap<i64, Affix>,
    /// Loot table id -> rows.
    pub loot_tables: HashMap<i64, Vec<LootRow>>,
    /// (quality, required_level) -> generated equippable entries, sorted.
    pub grid: HashMap<(i64, i64), Vec<i64>>,
}

pub const ITEM_KEYS: &[&str] = &[
    "name",
    "icon",
    "model",
    "equip_type",
    "weapon_type",
    "armor_type",
    "material",
    "quality",
    "required_level",
    "item_level",
    "stack",
    "spell",
    "stat",
    "sell_price",
    "flags",
    "description",
];
pub const BASE_KEYS: &[&str] =
    &["name", "model", "icon", "equip_type", "weapon_type", "armor_type", "material", "levels"];
pub const AFFIX_KEYS: &[&str] = &["name", "noun", "level", "stat"];
pub const LOOT_KEYS: &[&str] = &["item"];

/// `flags=` names ([`item::flags`]).
pub const FLAG_NAMES: [(&str, i64); 7] = [
    ("no_save", item::flags::NO_SAVE),
    ("no_trade", item::flags::NO_TRADE),
    ("no_arena", item::flags::NO_ARENA),
    ("no_group_dungeon", item::flags::NO_GROUP_DUNGEON),
    ("quest_item", item::flags::QUEST_ITEM),
    ("skillbook", item::flags::SKILLBOOK),
    ("gold_value_scales", item::flags::GOLD_VALUE_SCALES),
];

fn named(s: &Section, k: &str, names: &[(&str, i64)]) -> anyhow::Result<i64> {
    match s.get(k) {
        None => Ok(0),
        Some(v) => item::lookup(names, v).with_context(|| format!("[{}] {k}={v:?}: unknown", s.id)),
    }
}

fn int(s: &Section, k: &str, default: i64) -> anyhow::Result<i64> {
    match s.get(k) {
        Some(v) => v.parse().with_context(|| format!("[{}] {k}={v:?}: not a number", s.id)),
        None => Ok(default),
    }
}

/// `stat=<key>:<value>` lines.
fn stats<T: std::str::FromStr>(s: &Section) -> anyhow::Result<Vec<(i64, T)>> {
    s.all("stat")
        .map(|v| {
            let (k, x) = v.split_once(':').with_context(|| format!("[{}] stat={v:?}: want key:value", s.id))?;
            let id = stat::from_key(k).with_context(|| format!("[{}] stat={v:?}: unknown stat", s.id))?;
            Ok((id, x.trim().parse().ok().with_context(|| format!("[{}] stat={v:?}: bad value", s.id))?))
        })
        .collect()
}

/// `lo-hi` (or one level).
fn levels(s: &Section, k: &str) -> anyhow::Result<(i64, i64)> {
    let v = s.get(k).with_context(|| format!("[{}] missing {k}", s.id))?;
    let (lo, hi) = v.split_once('-').unwrap_or((v, v));
    let p = |x: &str| x.trim().parse::<i64>().with_context(|| format!("[{}] {k}={v:?}", s.id));
    let (lo, hi) = (p(lo)?, p(hi)?);
    if lo < 1 || hi < lo {
        bail!("[{}] {k}={v:?}: empty range", s.id);
    }
    Ok((lo, hi))
}

pub fn parse_item(s: &Section) -> anyhow::Result<ItemTemplate> {
    let Some(entry) = s.id_int().filter(|e| *e > 0 && *e < item::GRID_FIRST) else {
        bail!("line {}: item entry {:?} outside 1..{}", s.line, s.id, item::GRID_FIRST)
    };
    let text = |k: &str| s.get(k).unwrap_or_default().to_string();
    let mut flags = 0;
    for f in s.list("flags") {
        flags |= item::lookup(&FLAG_NAMES, f).with_context(|| format!("[{entry}] unknown flag {f:?}"))?;
    }
    Ok(ItemTemplate {
        entry,
        name: text("name"),
        icon: text("icon"),
        model: text("model"),
        equip_type: named(s, "equip_type", &equip::NAMES)?,
        weapon_type: named(s, "weapon_type", &weapon::NAMES)?,
        armor_type: int(s, "armor_type", 0)?,
        weapon_material: int(s, "material", 0)?,
        quality: s.get("quality").map_or(Ok(quality::COMMON), |_| named(s, "quality", &quality::NAMES))?,
        required_level: int(s, "required_level", 1)?,
        item_level: int(s, "item_level", 0)?,
        stack_count: int(s, "stack", 1)?,
        sell_price: int(s, "sell_price", 0)?,
        flags,
        spells: s
            .all("spell")
            .map(|v| v.parse().with_context(|| format!("[{entry}] spell={v:?}")))
            .collect::<Result<_, _>>()?,
        stats: stats(s)?,
        description: text("description"),
        ..Default::default()
    })
}

/// One base of `data/item_bases.txt`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ItemBase {
    pub id: i64,
    /// Per quality 2..=6.
    pub names: [String; 5],
    pub models: [String; 5],
    pub icons: [String; 5],
    pub equip_type: i64,
    pub weapon_type: i64,
    pub armor_type: i64,
    pub material: i64,
    pub levels: (i64, i64),
}

fn per_quality(s: &Section, k: &str) -> anyhow::Result<[String; 5]> {
    let v: Vec<String> = s.get(k).unwrap_or_default().split(',').map(|x| x.trim().to_string()).collect();
    match v.len() {
        1 => Ok(std::array::from_fn(|_| v[0].clone())),
        5 => Ok(std::array::from_fn(|i| v[i].clone())),
        n => bail!("[base {}] {k}: want 1 or 5 values (qualities 2..6), got {n}", s.id),
    }
}

pub fn parse_base(s: &Section) -> anyhow::Result<ItemBase> {
    let Some(id) = s.id_int().filter(|i| (1..900).contains(i)) else {
        bail!("line {}: base id {:?} outside 1..900", s.line, s.id)
    };
    let b = ItemBase {
        id,
        names: per_quality(s, "name")?,
        models: per_quality(s, "model")?,
        icons: per_quality(s, "icon")?,
        equip_type: named(s, "equip_type", &equip::NAMES)?,
        weapon_type: named(s, "weapon_type", &weapon::NAMES)?,
        armor_type: int(s, "armor_type", 0)?,
        material: int(s, "material", 0)?,
        levels: levels(s, "levels")?,
    };
    if item::slots_for(b.equip_type).is_empty() || b.names[0].is_empty() || b.icons[0].is_empty() {
        bail!("[base {id}] needs an equip_type, a name and an icon");
    }
    Ok(b)
}

/// DESIGN: sell price of a generated item, by slot, level and quality.
fn generated_price(b: &ItemBase, q: i64, level: i64) -> i64 {
    let slot = match b.equip_type {
        equip::WEAPON | equip::RANGED => 1.5,
        equip::CHEST | equip::SHIELD => 1.2,
        equip::LEGS => 1.0,
        equip::HEAD | equip::NECK | equip::RING | equip::BELT => 0.8,
        _ => 0.6,
    };
    let qm = [1.0, 1.15, 1.3, 1.5, 1.8][(q - quality::COMMON).clamp(0, 4) as usize];
    ((6.0 + 10.0 * level as f64) * slot * qm).round() as i64
}

/// Every item of a base: qualities 2..=6 x its level band.
pub fn generate(b: &ItemBase) -> Vec<ItemTemplate> {
    let mut out = Vec::new();
    for q in quality::COMMON..=quality::PURPLE {
        let i = (q - quality::COMMON) as usize;
        for level in b.levels.0..=b.levels.1 {
            out.push(ItemTemplate {
                entry: item::grid_entry(b.id, q, level),
                name: b.names[i].clone(),
                icon: b.icons[i].clone(),
                model: b.models[i].clone(),
                required_level: level,
                weapon_type: b.weapon_type,
                armor_type: b.armor_type,
                equip_type: b.equip_type,
                weapon_material: b.material,
                quality: q,
                sell_price: generated_price(b, q, level),
                // DESIGN: 40 plain .. 120 purple (shown in tooltips; no wear yet).
                durability: 40 + 20 * (q - quality::COMMON),
                stack_count: 1,
                generated: true,
                ..Default::default()
            });
        }
    }
    out
}

pub fn parse_affix(s: &Section) -> anyhow::Result<Affix> {
    let Some(entry) = s.id_int().filter(|e| *e > 0) else { bail!("line {}: bad affix id {:?}", s.line, s.id) };
    let (min_level, max_level) = levels(s, "level")?;
    let a = Affix {
        entry,
        name: s.get("name").unwrap_or_default().to_string(),
        single_noun: int(s, "noun", 0)? != 0,
        min_level,
        max_level,
        stats: stats(s)?,
    };
    if a.name.is_empty() || a.stats.is_empty() {
        bail!("[{entry}] an affix needs a name and a stat");
    }
    Ok(a)
}

pub fn parse_loot(s: &Section) -> anyhow::Result<Vec<LootRow>> {
    let Some(loot_id) = s.id_int().filter(|_| s.kind.as_deref() == Some("loot")) else {
        bail!("line {}: want [loot N]", s.line)
    };
    s.all("item")
        .map(|v| {
            let f: Vec<&str> = v.split(',').map(str::trim).collect();
            let bad = || format!("[loot {loot_id}] item={v:?}: want entry,chance,min-max");
            let [item, chance, count] = f[..] else { bail!(bad()) };
            let (lo, hi) = count.split_once('-').unwrap_or((count, count));
            Ok(LootRow {
                loot_id,
                item: item.parse().with_context(bad)?,
                chance: chance.parse().with_context(bad)?,
                count_min: lo.trim().parse::<i64>().with_context(bad)?.max(1),
                count_max: hi.trim().parse::<i64>().with_context(bad)?.max(1),
                conditional: false,
            })
        })
        .collect()
}

/// Builds the tables from the four files' sections.
pub fn parse(
    items: &[Section],
    bases: &[Section],
    affixes: &[Section],
    loot: &[Section],
) -> anyhow::Result<ItemTables> {
    let mut t = ItemTables::default();
    for s in items {
        let it = parse_item(s)?;
        t.items.insert(it.entry, it);
    }
    for s in bases {
        if s.kind.as_deref() != Some("base") {
            bail!("item_bases.txt line {}: want [base N]", s.line);
        }
        for it in generate(&parse_base(s)?) {
            if t.items.insert(it.entry, it).is_some() {
                bail!("item_bases.txt [base {}]: entry clash", s.id);
            }
        }
    }
    for s in affixes {
        let a = parse_affix(s)?;
        t.affixes.insert(a.entry, a);
    }
    for s in loot {
        let rows = parse_loot(s)?;
        t.loot_tables.insert(s.id_int().unwrap_or_default(), rows);
    }
    t.grid = generated_grid(&t.items);
    Ok(t)
}

pub fn load(root: &Path) -> anyhow::Result<ItemTables> {
    let data = root.join("data");
    let load = |f: &str| sections::load(&data.join(f));
    parse(&load("items.txt")?, &load("item_bases.txt")?, &load("affixes.txt")?, &load("loot.txt")?)
}

/// (quality, required_level) -> the generated equippable templates, sorted.
pub fn generated_grid(items: &HashMap<i64, ItemTemplate>) -> HashMap<(i64, i64), Vec<i64>> {
    let mut grid: HashMap<(i64, i64), Vec<i64>> = HashMap::new();
    for t in items.values().filter(|t| t.generated && t.is_equippable()) {
        grid.entry((t.quality, t.required_level.max(1))).or_default().push(t.entry);
    }
    grid.values_mut().for_each(|v| v.sort_unstable());
    grid
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_items_bases_affixes_loot() {
        let items = sections::parse(
            "[1]\nname=Ember Draught\nicon=a.png\nstack=5\nspell=50110\nsell_price=5\n\
             [1001]\nname=Ring\nicon=r.png\nequip_type=ring\nquality=blue\nstat=courage:3\nstat=health:20\nflags=quest_item\n",
        )
        .unwrap();
        let bases = sections::parse(
            "[base 2]\nname=Knife,Dirk,Kris,Kris,Kris\nmodel=dagger\nicon=a,b,c,d,e\nequip_type=weapon\n\
             weapon_type=dagger\nmaterial=1\nlevels=1-3\n",
        )
        .unwrap();
        let affixes = sections::parse("[11]\nname=Hauler\nnoun=1\nlevel=1-5\nstat=strength:1.7\n").unwrap();
        let loot = sections::parse("[loot 1]\nitem=1001,50,1-1\nitem=1,25,1-3\n").unwrap();
        let t = parse(&items, &bases, &affixes, &loot).unwrap();
        assert_eq!((t.items[&1].max_stack(), t.items[&1].spells.as_slice()), (5, &[50110][..]));
        let ring = &t.items[&1001];
        assert_eq!((ring.equip_type, ring.quality, ring.flags), (equip::RING, quality::BLUE, item::flags::QUEST_ITEM));
        assert_eq!(ring.stats, [(stat::COURAGE, 3), (stat::HEALTH, 20)]);
        assert_eq!(t.items.len(), 2 + 5 * 3);
        let kris = &t.items[&item::grid_entry(2, quality::PURPLE, 3)];
        assert_eq!((kris.name.as_str(), kris.icon.as_str(), kris.required_level), ("Kris", "e", 3));
        assert!(kris.generated && kris.sell_price > 0);
        assert_eq!(t.grid[&(quality::GREEN, 2)], [item::grid_entry(2, 3, 2)]);
        assert!(!t.grid.contains_key(&(quality::GREEN, 4)));
        let a = &t.affixes[&11];
        assert_eq!(
            (a.single_noun, a.min_level, a.max_level, a.stats.clone()),
            (true, 1, 5, vec![(stat::STRENGTH, 1.7)])
        );
        assert_eq!(t.loot_tables[&1].len(), 2);
        assert_eq!((t.loot_tables[&1][1].count_min, t.loot_tables[&1][1].count_max), (1, 3));
        let bad = sections::parse("[1]\nequip_type=elbow\n").unwrap();
        assert!(parse(&bad, &[], &[], &[]).is_err());
    }

    #[test]
    fn shipped_items_are_valid() {
        let root = crate::content_root();
        let data = root.join("data");
        for (file, keys) in [
            ("items.txt", ITEM_KEYS),
            ("item_bases.txt", BASE_KEYS),
            ("affixes.txt", AFFIX_KEYS),
            ("loot.txt", LOOT_KEYS),
        ] {
            let s = sections::load(&data.join(file)).unwrap();
            assert!(sections::unknown_keys(&s, keys).is_empty(), "{file}: {:?}", sections::unknown_keys(&s, keys));
        }
        let t = load(&root).unwrap();
        let spells = super::super::spells::load(&root).unwrap();
        let npcs = super::super::npcs::load(&root).unwrap();
        // Hand-made items: names, icons, item spells.
        for it in t.items.values() {
            assert!(!it.name.is_empty() && !it.icon.is_empty(), "{}", it.entry);
            for s in &it.spells {
                assert!(spells.contains_key(s), "{}: unknown spell {s}", it.name);
            }
            if it.has_model() {
                let script = root.join(format!("scripts/player/custom/{}.txt", it.model));
                assert!(script.exists(), "{}: no paper-doll script {}", it.name, it.model);
            }
            if it.generated {
                assert_eq!(item::grid_parts(it.entry).map(|p| (p.1, p.2)), Some((it.quality, it.required_level)));
            }
        }
        // The quest reward and the starter potions.
        assert_eq!(t.items[&1].name, "Ember Draught");
        // Every level and quality has gear of every kind of slot.
        for q in quality::COMMON..=quality::PURPLE {
            for level in 1..=25 {
                let g = &t.grid[&(q, level)];
                let has = |e: i64| g.iter().any(|x| t.items[x].equip_type == e);
                assert!(
                    [equip::HEAD, equip::CHEST, equip::WEAPON, equip::RANGED, equip::SHIELD, equip::RING]
                        .into_iter()
                        .all(has)
                );
            }
        }
        // Affix bands cover every level; loot and junk point at items.
        for level in 1..=25 {
            assert!(t.affixes.values().filter(|a| a.min_level <= level && level <= a.max_level).count() >= 10);
        }
        for rows in t.loot_tables.values() {
            for r in rows {
                assert!(t.items.contains_key(&r.item), "loot {}: unknown item {}", r.loot_id, r.item);
            }
        }
        for (model, list) in &npcs.junk {
            for e in list {
                assert!(t.items.get(e).is_some_and(|i| i.quality == quality::JUNK), "junk of {model}: {e}");
            }
        }
        for (entry, l) in &npcs.loot {
            assert!(
                l.custom_loot <= 0 || t.loot_tables.contains_key(&l.custom_loot),
                "npc {entry}: loot {}",
                l.custom_loot
            );
        }
    }
}
