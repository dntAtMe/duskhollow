//! Item templates, affixes, loot tables and the generated equipment grid.

use crate::item::{Affix, ItemTemplate, LootRow};
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
    /// (weapon?, level, weapon material or armour type) -> percent. Stream 0 only: the material
    /// tier of random gear (Stream A replaces it with level-band tiers and drops the field).
    pub materials: HashMap<(bool, i64, i64), f32>,
}

pub fn load(_root: &Path) -> anyhow::Result<ItemTables> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "item_template, affix_template, loot, material_chance_weapon, material_chance_armor");
    let items = db.items()?;
    Ok(ItemTables {
        grid: generated_grid(&items),
        affixes: db.affixes()?,
        loot_tables: db.loot_tables()?,
        materials: db.material_chances()?,
        items,
    })
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
