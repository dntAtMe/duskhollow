//! NPC templates, their models (sprite script name, height) and loot settings, from
//! `data/npc_templates.txt`.
//!
//! One `[entry]` section per template; each template brings its own model with `id = entry`.
//! Keys: [`NpcTemplate`] field names plus `model` (sprite script under `scripts/npc/`),
//! `height` (pixels, nameplate offset), `level=MIN[-MAX]`, `resist=frost,fire,shadow,holy`,
//! `spellN=spell,chance,interval_ms,cooldown_ms,target_type` (N = 1..=4), and loot:
//! `loot=<loot table>` (`data/loot.txt`), `loot_chances=green,blue,gold,purple` (percent, -1 =
//! default), `gold_ratio=` (percent of the default gold, -1 = default), `junk=<item>,<item>`.

use super::sections::{self, Section};
use super::types::{NpcModel, NpcSpell, NpcTemplate, faction};
use crate::item::NpcLoot;
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Npcs {
    /// By template entry.
    pub templates: HashMap<i64, NpcTemplate>,
    /// By model id (`NpcTemplate::model_id`).
    pub models: HashMap<i64, NpcModel>,
    /// By template entry (missing = defaults).
    pub loot: HashMap<i64, NpcLoot>,
    /// Model id -> junk item entries.
    pub junk: HashMap<i64, Vec<i64>>,
}

/// First template entry (and model id); entries below are not ours to use.
pub const FIRST_ENTRY: i64 = 50000;

pub const KEYS: &[&str] = &[
    "name",
    "subname",
    "model",
    "height",
    "portrait",
    "level",
    "faction",
    "model_scale",
    "health",
    "mana",
    "weapon_value",
    "armor",
    "melee_speed_ms",
    "leash_range",
    "ai_type",
    "npc_flags",
    "strength",
    "agility",
    "intellect",
    "willpower",
    "courage",
    "resist",
    "elite",
    "boss",
    "spell1",
    "spell2",
    "spell3",
    "spell4",
    "loot",
    "loot_chances",
    "gold_ratio",
    "junk",
];

fn default_template(entry: i64) -> NpcTemplate {
    NpcTemplate {
        entry,
        name: String::new(),
        subname: String::new(),
        model_id: entry,
        min_level: 1,
        max_level: 1,
        faction: faction::HOSTILE,
        model_scale: 100,
        health: -1,
        mana: 0,
        weapon_value: -1,
        armor: 0,
        melee_speed_ms: 2000,
        leash_range: 0,
        ai_type: 0,
        npc_flags: 0,
        strength: 0,
        agility: 0,
        intellect: 0,
        willpower: 0,
        courage: 0,
        resist: [0; 4],
        spells: Vec::new(),
        elite: false,
        boss: false,
        portrait: String::new(),
    }
}

fn nums<T: std::str::FromStr>(s: &Section, k: &str, v: &str) -> anyhow::Result<Vec<T>> {
    v.split(',').map(|x| x.trim().parse::<T>().ok().with_context(|| format!("[{}] {k}={v:?}", s.id))).collect()
}

/// One template (with its model, loot settings and junk list) from its section.
pub fn parse_one(s: &Section) -> anyhow::Result<(NpcTemplate, NpcModel, NpcLoot, Vec<i64>)> {
    let Some(entry) = s.id_int() else { bail!("line {}: bad npc entry {:?}", s.line, s.id) };
    let mut t = default_template(entry);
    let mut m = NpcModel { id: entry, name: String::new(), height: 0 };
    let mut loot = NpcLoot { chances: [-1.0; 4], custom_loot: -1, gold_ratio: -1 };
    let mut junk = Vec::new();
    for (k, v) in &s.entries {
        let n = || v.parse::<i64>().with_context(|| format!("[{entry}] {k}={v:?}: not a number"));
        match k.as_str() {
            "name" => t.name = v.clone(),
            "subname" => t.subname = v.clone(),
            "portrait" => t.portrait = v.clone(),
            "model" => m.name = v.clone(),
            "height" => m.height = n()?,
            "level" => {
                let (lo, hi) = v.split_once('-').unwrap_or((v, v));
                t.min_level = lo.trim().parse().with_context(|| format!("[{entry}] level={v:?}"))?;
                t.max_level = hi.trim().parse().with_context(|| format!("[{entry}] level={v:?}"))?;
            }
            "faction" => t.faction = n()?,
            "model_scale" => t.model_scale = n()?,
            "health" => t.health = n()?,
            "mana" => t.mana = n()?,
            "weapon_value" => t.weapon_value = n()?,
            "armor" => t.armor = n()?,
            "melee_speed_ms" => t.melee_speed_ms = n()?,
            "leash_range" => t.leash_range = n()?,
            "ai_type" => t.ai_type = n()?,
            "npc_flags" => t.npc_flags = n()?,
            "strength" => t.strength = n()?,
            "agility" => t.agility = n()?,
            "intellect" => t.intellect = n()?,
            "willpower" => t.willpower = n()?,
            "courage" => t.courage = n()?,
            "elite" => t.elite = n()? != 0,
            "boss" => t.boss = n()? != 0,
            "resist" => {
                let r: Vec<i64> = nums(s, k, v)?;
                for (slot, x) in t.resist.iter_mut().zip(r) {
                    *slot = x;
                }
            }
            "loot" => loot.custom_loot = n()?,
            "gold_ratio" => loot.gold_ratio = n()?,
            "loot_chances" => {
                let c: Vec<f32> = nums(s, k, v)?;
                if c.len() != 4 {
                    bail!("[{entry}] loot_chances needs green,blue,gold,purple");
                }
                loot.chances.copy_from_slice(&c);
            }
            "junk" => junk = nums(s, k, v)?,
            _ => {
                if !k.strip_prefix("spell").and_then(|i| i.parse::<usize>().ok()).is_some_and(|i| (1..=4).contains(&i))
                {
                    bail!("[{entry}] unknown key {k}");
                }
                let f: Vec<i64> = nums(s, k, v)?;
                let [spell, chance, interval_ms, cooldown_ms, target_type] = f[..] else {
                    bail!("[{entry}] {k} needs spell,chance,interval_ms,cooldown_ms,target_type");
                };
                // In file order (spell1 first).
                t.spells.push(NpcSpell { spell, chance, interval_ms, cooldown_ms, target_type });
            }
        }
    }
    if t.max_level < t.min_level {
        bail!("[{entry}] level range {}-{}", t.min_level, t.max_level);
    }
    Ok((t, m, loot, junk))
}

pub fn parse(sections: &[Section]) -> anyhow::Result<Npcs> {
    let mut out = Npcs::default();
    for s in sections {
        let (t, m, loot, junk) = parse_one(s)?;
        if !junk.is_empty() {
            out.junk.insert(m.id, junk);
        }
        out.loot.insert(t.entry, loot);
        out.models.insert(m.id, m);
        out.templates.insert(t.entry, t);
    }
    Ok(out)
}

pub fn load(root: &Path) -> anyhow::Result<Npcs> {
    parse(&sections::load(&root.join("data/npc_templates.txt"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_templates() {
        let s = sections::parse(
            "# c\n[50001]\nname=Glarewolf\nmodel=glarewolf\nheight=40\nlevel=2-3\nboss=1\nspell1=12,50,4000,8000,14\n\
             loot=3\nloot_chances=10,-1,-1,0.5\ngold_ratio=50\njunk=100,101\n\n[50010]\nname=Ysolde\nfaction=1\n",
        )
        .unwrap();
        let n = parse(&s).unwrap();
        let w = &n.templates[&50001];
        assert_eq!((w.entry, w.model_id, w.min_level, w.max_level, w.boss), (50001, 50001, 2, 3, true));
        let m = &n.models[&50001];
        assert_eq!((m.name.as_str(), m.height, w.spells[0].spell, w.spells[0].target_type), ("glarewolf", 40, 12, 14));
        let l = n.loot[&50001];
        assert_eq!((l.custom_loot, l.gold_ratio, l.chances), (3, 50, [10.0, -1.0, -1.0, 0.5]));
        assert_eq!(n.junk[&50001], [100, 101]);
        assert_eq!((n.templates[&50010].faction, n.loot[&50010].custom_loot), (1, -1));
        assert!(parse(&sections::parse("[1]\nbogus=1\n").unwrap()).is_err());
        assert!(parse(&sections::parse("[1]\nspell1=1,2\n").unwrap()).is_err());
    }

    #[test]
    fn shipped_templates_are_valid() {
        let root = crate::assets_root();
        let s = sections::load(&root.join("data/npc_templates.txt")).unwrap();
        assert!(sections::unknown_keys(&s, KEYS).is_empty());
        let n = load(&root).unwrap();
        let spells = super::super::spells::load(&root).unwrap();
        for t in n.templates.values() {
            assert!(t.entry >= FIRST_ENTRY && !t.name.is_empty(), "{t:?}");
            let m = &n.models[&t.model_id];
            assert!(root.join(format!("scripts/npc/{}.txt", m.name)).exists(), "{}: model {}", t.name, m.name);
            for sp in &t.spells {
                assert!(spells.contains_key(&sp.spell), "{}: unknown spell {}", t.name, sp.spell);
            }
        }
        // The glade's creatures.
        for e in 50020..=50023 {
            assert!(n.templates.contains_key(&e), "{e}");
        }
    }
}
