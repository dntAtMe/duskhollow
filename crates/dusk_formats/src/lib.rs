//! Parsers for legacy data files. Engine-agnostic: no Bevy here,
//! so both client and server (and tests) can use them.
//!
//! See `docs/formats.md` for the reverse-engineered layouts.

pub mod custom;
pub mod db;
pub mod item;
pub mod map;
pub mod path;
pub mod psi;
pub mod sound;
pub mod spell;
pub mod sprite_anim;
pub mod sprite_fx;
pub mod sprite_script;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Extracted-assets directory: `$DUSK_ASSETS`, else `<workspace>/assets` (dev builds),
/// else `assets/` next to the executable.
pub fn assets_root() -> PathBuf {
    if let Some(p) = std::env::var_os("DUSK_ASSETS") {
        return PathBuf::from(p);
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    if dev.join("game.db").exists() {
        return dev.canonicalize().unwrap_or(dev);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("assets"))).unwrap_or_else(|| "assets".into())
}

/// Our own (non-original) art produced by `tools/artgen`: `$DUSK_CUSTOM_ASSETS`, else
/// `<workspace>/custom_assets`, else `custom_assets/` next to the executable.
pub fn custom_assets_root() -> PathBuf {
    if let Some(p) = std::env::var_os("DUSK_CUSTOM_ASSETS") {
        return PathBuf::from(p);
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../custom_assets");
    if dev.exists() {
        return dev.canonicalize().unwrap_or(dev);
    }
    std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(|p| p.join("custom_assets")))
        .unwrap_or_else(|| "custom_assets".into())
}

/// Basename -> path lookup produced by `dusk_extract` (`assets/file_index.txt`).
/// The original client resolves textures by bare, case-insensitive filename.
#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    map: HashMap<String, String>,
}

impl FileIndex {
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let map =
            text.lines().filter_map(|l| l.split_once('\t')).map(|(k, v)| (k.to_string(), v.to_string())).collect();
        Ok(Self { map })
    }

    /// Adds (or overrides) a basename -> relative path entry.
    pub fn insert(&mut self, name: &str, rel: &str) {
        self.map.insert(name.to_lowercase(), rel.replace('\\', "/"));
    }

    /// Path relative to the assets root.
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.map.get(&name.to_lowercase()).map(String::as_str)
    }

    pub fn resolve_path(&self, root: &Path, name: &str) -> Option<PathBuf> {
        self.resolve(name).map(|r| root.join(r))
    }
}
