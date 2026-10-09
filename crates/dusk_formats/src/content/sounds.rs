//! Sound tables: region music/ambience, NPC voice sets, looping sounds next to map sprites.

use crate::sound::SoundTables;
use std::path::Path;

pub fn load(_root: &Path) -> anyhow::Result<SoundTables> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "zone_template, area_template, npc_sounds, sprite_proximity_sound");
    Ok(db.sound_tables()?)
}
