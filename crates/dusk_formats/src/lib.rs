//! Parsers for our content and the legacy data files. Engine-agnostic: no Bevy here,
//! so both client and server (and tests) can use them.
//!
//! See `docs/formats.md` for the reverse-engineered layouts.

pub mod content;
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

/// Asset-source prefix of files served from [`legacy_root`] (registered by the client as a
/// Bevy asset source; [`fs_path`] maps it back to a filesystem path).
pub const LEGACY_SOURCE: &str = "legacy://";

/// Our own content (data, maps, scripts, art produced by `tools/artgen`):
/// `$DUSK_CUSTOM_ASSETS`, else `<workspace>/custom_assets`, else `custom_assets/` next to the
/// executable.
pub fn content_root() -> PathBuf {
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

/// The legacy data pack (`game.db`, `file_index.txt`, legacy maps/scripts/content):
/// `$DUSK_LEGACY`, else `$DUSK_ASSETS`, else `<workspace>/assets` (dev builds), else `assets/`
/// next to the executable.
pub fn legacy_root() -> PathBuf {
    for var in ["DUSK_LEGACY", "DUSK_ASSETS"] {
        if let Some(p) = std::env::var_os(var) {
            return PathBuf::from(p);
        }
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    if dev.join("game.db").exists() {
        return dev.canonicalize().unwrap_or(dev);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("assets"))).unwrap_or_else(|| "assets".into())
}

/// Alias of [`legacy_root`] (legacy tests and tools).
pub fn assets_root() -> PathBuf {
    legacy_root()
}

/// `DUSK_LEGACY_LOG=1`: prints every distinct legacy access once (`[legacy] kind: what`), the
/// runtime inventory of what still comes from the legacy data pack.
pub fn legacy_log_enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("DUSK_LEGACY_LOG").is_ok_and(|v| v == "1"))
}

/// Records one legacy access (see [`legacy_log_enabled`]); repeated accesses print once.
pub fn legacy_note(kind: &str, what: impl std::fmt::Display) {
    if !legacy_log_enabled() {
        return;
    }
    static SEEN: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> = std::sync::OnceLock::new();
    let line = format!("{kind}: {what}");
    if SEEN.get_or_init(Default::default).lock().unwrap().insert(line.clone()) {
        eprintln!("[legacy] {line}");
    }
}

/// Filesystem path of an asset path as returned by [`FileIndex::resolve`]: `legacy://<rel>` is
/// under [`legacy_root`] (logged), anything else under [`content_root`].
pub fn fs_path(rel: &str) -> PathBuf {
    match rel.strip_prefix(LEGACY_SOURCE) {
        Some(rest) => {
            legacy_note("file", rest);
            legacy_root().join(rest)
        }
        None => content_root().join(rel),
    }
}

/// `<root>/<rel>` if it exists, else the legacy file of the same relative path (logged), else
/// `None`. Scripts and maps are read this way.
pub fn find_file(root: &Path, rel: &str) -> Option<PathBuf> {
    let own = root.join(rel);
    if own.exists() {
        return Some(own);
    }
    let legacy = legacy_root().join(rel);
    legacy.exists().then(|| {
        legacy_note("file", rel);
        legacy
    })
}

/// Basename -> asset path lookup (the legacy `file_index.txt` plus our content files).
/// The original client resolves textures by bare, case-insensitive filename.
#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    map: HashMap<String, String>,
}

impl FileIndex {
    pub fn load(path: impl AsRef<Path>) -> std::io::Result<Self> {
        Self::load_prefixed(path, "")
    }

    /// Like [`FileIndex::load`], with `prefix` (e.g. [`LEGACY_SOURCE`]) put before every path.
    pub fn load_prefixed(path: impl AsRef<Path>, prefix: &str) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let map = text
            .lines()
            .filter_map(|l| l.split_once('\t'))
            .map(|(k, v)| (k.to_string(), format!("{prefix}{v}")))
            .collect();
        Ok(Self { map })
    }

    /// The legacy `file_index.txt` of [`legacy_root`], every path as `legacy://<rel>` (empty if
    /// there is no legacy data).
    pub fn load_legacy() -> Self {
        let path = legacy_root().join("file_index.txt");
        if !path.exists() {
            return Self::default();
        }
        legacy_note("file", "file_index.txt");
        Self::load_prefixed(path, LEGACY_SOURCE).unwrap_or_default()
    }

    /// Adds (or overrides) a basename -> relative path entry.
    pub fn insert(&mut self, name: &str, rel: &str) {
        self.map.insert(name.to_lowercase(), rel.replace('\\', "/"));
    }

    /// Asset path: relative to [`content_root`], or `legacy://<path relative to legacy_root>`.
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.map.get(&name.to_lowercase()).map(String::as_str)
    }
}
