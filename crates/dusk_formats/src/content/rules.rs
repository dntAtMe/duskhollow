//! Character rules: per-level class stats, experience table, starting spells and items, what
//! each class may wear and wield and which stats its loot favours.
//!
//! - `data/classes.txt`: `[class]` with `name`, `role`, per-level formulas (`clvl` = level)
//!   `hp mana strength agility willpower intelligence courage`, `armor=` families
//!   (`cloth leather mail plate robe`, see [`crate::item::ARMOR_FAMILIES`]) or armour types,
//!   `weapons=` (`axe bow mace sword staff dagger wand shield`), `stats=` (stat keys its loot
//!   favours, [`crate::item::stat::KEYS`]), `start_item=<entry>,<count>` (repeat, in order).
//! - `data/exp.txt`: `[level]` with `exp` (to advance from it), `kill_exp` (for killing an enemy
//!   of that level), optional `title`. The last level is the cap.
//! - `data/class_spells.txt`: `[class]` with `spell=<entry>` (repeat, in order).

use super::sections::{self, Section};
use super::types::{ClassStats, ExpLevel};
use crate::item::{self, stat, weapon};
use crate::spell::{FormulaVars, eval_formula};
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Rules {
    /// (class, level) -> stats.
    pub class_stats: HashMap<(i64, i64), ClassStats>,
    /// Sorted by level.
    pub exp_levels: Vec<ExpLevel>,
    /// class -> starting spells, in order.
    pub class_spells: HashMap<i64, Vec<i64>>,
    /// class -> (item entry, count) given to new characters, in order.
    pub start_items: HashMap<i64, Vec<(i64, i64)>>,
    /// class -> allowed body armour types (cloth, armour type <= 1, is always allowed).
    pub class_armor: HashMap<i64, Vec<i64>>,
    /// class -> stats worth rolling on that class's loot.
    pub desirable_stats: HashMap<i64, Vec<i64>>,
    /// class -> allowed weapon types (main hand and ranged; `weapon::SHIELD` = shields).
    pub class_weapons: HashMap<i64, Vec<i64>>,
}

pub const CLASS_KEYS: &[&str] = &[
    "name",
    "role",
    "hp",
    "mana",
    "strength",
    "agility",
    "willpower",
    "intelligence",
    "courage",
    "armor",
    "weapons",
    "stats",
    "start_item",
];
pub const EXP_KEYS: &[&str] = &["exp", "kill_exp", "title"];
pub const CLASS_SPELL_KEYS: &[&str] = &["spell"];

fn id(s: &Section) -> anyhow::Result<i64> {
    s.id_int().with_context(|| format!("line {}: bad id {:?}", s.line, s.id))
}

/// Evaluates a per-level formula of `[class]`.
fn per_level(s: &Section, k: &str, level: i64) -> anyhow::Result<i64> {
    let f = s.get(k).with_context(|| format!("[{}] missing {k}", s.id))?;
    let v = FormulaVars { clvl: level as f64, splvl: 1.0, ..Default::default() };
    let x = eval_formula(f, &v).map_err(|e| anyhow::anyhow!("[{}] {k}={f}: {e}", s.id))?;
    Ok(x.round() as i64)
}

fn names(s: &Section, k: &str, f: impl Fn(&str) -> Option<Vec<i64>>) -> anyhow::Result<Vec<i64>> {
    let mut out = Vec::new();
    for v in s.list(k) {
        out.extend(f(v).with_context(|| format!("[{}] {k}: unknown {v:?}", s.id))?);
    }
    Ok(out)
}

/// Parses the three files' sections (classes, exp, class spells).
pub fn parse(classes: &[Section], exp: &[Section], class_spells: &[Section]) -> anyhow::Result<Rules> {
    let mut r = Rules::default();
    for s in exp {
        let level = id(s)?;
        r.exp_levels.push(ExpLevel {
            level,
            exp: s.int("exp", 0),
            kill_exp: s.int("kill_exp", 0),
            name: s.get("title").unwrap_or_default().to_string(),
        });
    }
    r.exp_levels.sort_by_key(|l| l.level);
    if r.exp_levels.iter().enumerate().any(|(i, l)| l.level != i as i64 + 1) {
        bail!("exp.txt: levels must run 1, 2, 3 ... without gaps");
    }
    let max_level = r.exp_levels.len() as i64;
    for s in classes {
        let class = id(s)?;
        for level in 1..=max_level {
            let f = |k| per_level(s, k, level);
            r.class_stats.insert(
                (class, level),
                ClassStats {
                    class,
                    level,
                    hp: f("hp")?,
                    mana: f("mana")?,
                    strength: f("strength")?,
                    agility: f("agility")?,
                    willpower: f("willpower")?,
                    intelligence: f("intelligence")?,
                    courage: f("courage")?,
                },
            );
        }
        let armor = names(s, "armor", |v| {
            item::ARMOR_FAMILIES
                .iter()
                .find(|(n, _)| *n == v)
                .map(|(_, range)| range.clone().collect())
                .or_else(|| v.parse().ok().map(|x| vec![x]))
        })?;
        r.class_armor.insert(class, armor);
        r.class_weapons.insert(class, names(s, "weapons", |v| item::lookup(&weapon::NAMES, v).map(|x| vec![x]))?);
        r.desirable_stats.insert(class, names(s, "stats", |v| stat::from_key(v).map(|x| vec![x]))?);
        let mut items = Vec::new();
        for v in s.all("start_item") {
            let (e, n) = v.split_once(',').unwrap_or((v, "1"));
            let parse = |x: &str| x.trim().parse::<i64>().with_context(|| format!("[{class}] start_item={v:?}"));
            items.push((parse(e)?, parse(n)?.max(1)));
        }
        r.start_items.insert(class, items);
    }
    for s in class_spells {
        let class = id(s)?;
        let mut list: Vec<i64> = Vec::new();
        for v in s.all("spell") {
            let spell = v.parse().with_context(|| format!("class_spells [{class}] spell={v:?}"))?;
            if !list.contains(&spell) {
                list.push(spell);
            }
        }
        r.class_spells.insert(class, list);
    }
    Ok(r)
}

pub fn load(root: &Path) -> anyhow::Result<Rules> {
    let data = root.join("data");
    parse(
        &sections::load(&data.join("classes.txt"))?,
        &sections::load(&data.join("exp.txt"))?,
        &sections::load(&data.join("class_spells.txt"))?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_rules() {
        let classes = sections::parse(
            "[1]\nname=Vanguard\nhp=75*clvl\nmana=30*clvl\nstrength=15\nagility=10\nwillpower=5\nintelligence=5\n\
             courage=15\narmor=leather,mail\nweapons=sword,shield\nstats=strength,health\nstart_item=1,5\nstart_item=15\n",
        )
        .unwrap();
        let exp = sections::parse("[1]\nexp=100\nkill_exp=20\n[2]\nexp=120\nkill_exp=24\ntitle=Sword\n").unwrap();
        let spells = sections::parse("[1]\nspell=50100\nspell=50001\nspell=50100\n").unwrap();
        let r = parse(&classes, &exp, &spells).unwrap();
        let c = r.class_stats[&(1, 2)];
        assert_eq!((c.hp, c.mana, c.strength, c.courage), (150, 60, 15, 15));
        assert_eq!(r.exp_levels.len(), 2);
        assert_eq!(r.exp_levels[1].name, "Sword");
        assert_eq!(r.class_armor[&1], [2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(r.class_weapons[&1], [weapon::SWORD, weapon::SHIELD]);
        assert_eq!(r.desirable_stats[&1], [stat::STRENGTH, stat::HEALTH]);
        assert_eq!(r.start_items[&1], [(1, 5), (15, 1)]);
        assert_eq!(r.class_spells[&1], [50100, 50001]);
        assert!(parse(&classes, &sections::parse("[2]\nexp=1\n").unwrap(), &spells).is_err());
    }

    #[test]
    fn shipped_rules_are_valid() {
        let root = crate::assets_root();
        let data = root.join("data");
        for (file, keys) in [("classes.txt", CLASS_KEYS), ("exp.txt", EXP_KEYS), ("class_spells.txt", CLASS_SPELL_KEYS)]
        {
            let s = sections::load(&data.join(file)).unwrap();
            assert!(sections::unknown_keys(&s, keys).is_empty(), "{file}: {:?}", sections::unknown_keys(&s, keys));
        }
        let r = load(&root).unwrap();
        assert_eq!(r.exp_levels.len(), 25);
        let spells = super::super::spells::load(&root).unwrap();
        let items = super::super::items::load(&root).unwrap();
        for class in 1..=4 {
            let at1 = r.class_stats[&(class, 1)];
            assert!(at1.hp > 0 && at1.mana > 0);
            let known = &r.class_spells[&class];
            // Every class swings and shoots.
            assert!(known.contains(&super::super::spells::ATTACK) && known.contains(&super::super::spells::SHOOT));
            for s in known {
                assert!(spells.contains_key(s), "class {class}: unknown spell {s}");
            }
            for (e, n) in &r.start_items[&class] {
                let t = items.items.get(e).unwrap_or_else(|| panic!("class {class}: unknown start item {e}"));
                assert!(*n as u32 <= t.max_stack());
            }
            assert!(!r.class_weapons[&class].is_empty() && !r.desirable_stats[&class].is_empty());
        }
    }

    /// Levels 1-6 stay within 10 % of the pacing the game was tuned with.
    #[test]
    fn early_pacing_is_kept() {
        let r = load(&crate::assets_root()).unwrap();
        let tuned = [(100, 20), (100, 20), (123, 24), (150, 30), (195, 39), (267, 44)];
        for (l, (exp, kill)) in r.exp_levels.iter().zip(tuned) {
            assert!((l.exp as f32 / exp as f32 - 1.0).abs() <= 0.1, "level {} exp {}", l.level, l.exp);
            assert!((l.kill_exp as f32 / kill as f32 - 1.0).abs() <= 0.1, "level {} kill_exp {}", l.level, l.kill_exp);
        }
        let bases = [(1, 75, 30), (2, 40, 70), (3, 60, 45), (4, 45, 65)];
        for (class, hp, mana) in bases {
            for level in 1..=6 {
                let c = r.class_stats[&(class, level)];
                let want = (hp * level, mana * level);
                assert!((c.hp as f32 / want.0 as f32 - 1.0).abs() <= 0.1, "class {class} level {level} hp {}", c.hp);
                assert!((c.mana as f32 / want.1 as f32 - 1.0).abs() <= 0.1, "class {class} level {level}");
            }
        }
    }
}
