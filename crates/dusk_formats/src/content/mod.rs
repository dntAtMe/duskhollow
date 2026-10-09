//! One loader per kind of game data, each taking the asset root ([`crate::assets_root`]).
//! Client and server read data only through these. Formats: `docs/content.md`.

pub mod items;
pub mod maps;
pub mod npcs;
pub mod particles;
pub mod rules;
pub mod sections;
pub mod sidecars;
pub mod sounds;
pub mod spells;
pub mod sprite_fx;
pub mod types;
pub mod visuals;

/// UI font pair (DejaVu Serif, free to redistribute; licence in `content/fonts/LICENSE_DEJAVU`).
pub const UI_FONT: &str = "DejaVuSerif.ttf";
pub const UI_FONT_BOLD: &str = "DejaVuSerif-Bold.ttf";
