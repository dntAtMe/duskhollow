//! Character rules: per-level class stats, experience table, starting spells and items, what
//! each class may wear and wield and which stats its loot favours.

use crate::db::{ClassStats, ExpLevel};
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
    /// class -> allowed weapon types (main hand and ranged).
    pub class_weapons: HashMap<i64, Vec<i64>>,
}

pub fn load(root: &Path) -> anyhow::Result<Rules> {
    let db = super::legacy_db()?;
    crate::legacy_note(
        "game.db",
        "player_class_stats, player_exp_levels, player_create_spell, player_create_item, player_desirable_armor, player_desirable_stats",
    );
    let mut class_spells = db.class_spells()?;
    // Ours after each class's legacy starting spells (duplicates skipped).
    for (class, spell) in crate::custom::load_class_spells(root) {
        let list = class_spells.entry(class).or_default();
        if !list.contains(&spell) {
            list.push(spell);
        }
    }
    Ok(Rules {
        class_stats: db.class_stats()?.into_iter().map(|c| ((c.class, c.level), c)).collect(),
        exp_levels: db.exp_levels()?,
        class_spells,
        start_items: db.starting_items()?,
        class_armor: db.class_armor()?,
        desirable_stats: db.class_desirable_stats()?,
        class_weapons: (1..=4).map(|c| (c, crate::item::class_weapons(c).to_vec())).collect(),
    })
}
