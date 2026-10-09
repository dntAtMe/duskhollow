//! The content seam: one loader per kind of game data, each taking our content root
//! ([`crate::content_root`]). Client and server read data only through these.
//!
//! Stream 0 (see `docs/standalone-plan.md`): the bodies still merge the legacy data pack
//! (`game.db` under [`crate::legacy_root`]) with our own files; later streams replace the bodies
//! while the signatures stay.

pub mod items;
pub mod maps;
pub mod npcs;
pub mod particles;
pub mod rules;
pub mod sections;
pub mod sounds;
pub mod spells;
pub mod sprite_fx;
pub mod visuals;

use crate::db::GameDb;

/// The legacy `game.db` (read-only). Callers note the tables they read with
/// [`crate::legacy_note`].
pub(crate) fn legacy_db() -> anyhow::Result<GameDb> {
    let path = crate::legacy_root().join("game.db");
    GameDb::open(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}
