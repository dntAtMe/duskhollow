//! NPC templates, their models (sprite script name, height) and loot settings.

use crate::db::{NpcModel, NpcTemplate};
use crate::item::NpcLoot;
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

pub fn load(root: &Path) -> anyhow::Result<Npcs> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "npc_template, npc_models, npc_models_junkloot");
    let mut templates = db.npc_templates()?;
    let mut models = db.npc_models()?;
    // Ours (`data/npc_templates.txt`): each template brings its own model with `id = entry`.
    for (t, m) in crate::custom::load_npc_templates(root) {
        models.insert(m.id, m);
        templates.insert(t.entry, t);
    }
    Ok(Npcs { templates, models, loot: db.npc_loot()?, junk: db.junk_loot()? })
}
