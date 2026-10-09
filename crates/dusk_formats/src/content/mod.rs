//! The content seam: one loader per kind of game data, each taking our content root
//! ([`crate::content_root`]). Client and server read data only through these.
//!
//! Rules, NPCs, spells, items and maps read only our text files under `data/` (the `[id]` +
//! `key=value` format of [`sections`]). Visuals, particles, sprite effects and sounds still
//! merge the legacy data pack (`game.db` under [`crate::legacy_root`]) until their streams of
//! `docs/standalone-plan.md` land.

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
