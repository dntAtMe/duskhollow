//! Spell templates by entry (player skills, NPC spells, item spells, auto attacks).

use crate::spell::SpellTemplate;
use std::collections::HashMap;
use std::path::Path;

pub fn load(root: &Path) -> anyhow::Result<HashMap<i64, SpellTemplate>> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "spell_template");
    let mut spells = db.spells()?;
    // Ours (`data/spells.txt`, entries from `custom::CUSTOM_SPELL_FIRST`).
    for s in crate::custom::load_spells(root) {
        spells.insert(s.entry, s);
    }
    Ok(spells)
}
