//! Parsers for our content. Engine-agnostic: no Bevy here, so both client and server (and tests)
//! can use them.
//!
//! See `docs/content.md` for every file format and how to add content.

pub mod content;
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

/// The asset root (`data/`, `maps/`, `scripts/`, `content/`): `$DUSK_ASSETS`, else
/// `<workspace>/assets` (dev builds), else `assets/` next to the executable.
pub fn assets_root() -> PathBuf {
    if let Some(p) = std::env::var_os("DUSK_ASSETS") {
        return PathBuf::from(p);
    }
    let dev = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets");
    if dev.join("data").exists() {
        return dev.canonicalize().unwrap_or(dev);
    }
    std::env::current_exe().ok().and_then(|e| e.parent().map(|p| p.join("assets"))).unwrap_or_else(|| "assets".into())
}

/// `<root>/<rel>` if it exists. Scripts and maps are read this way.
pub fn find_file(root: &Path, rel: &str) -> Option<PathBuf> {
    let p = root.join(rel);
    p.exists().then_some(p)
}

/// Basename -> asset path lookup over every file under `<root>/content`. Data files, maps and
/// sprite scripts name art, sounds and fonts by bare, case-insensitive file name.
#[derive(Debug, Default, Clone)]
pub struct FileIndex {
    map: HashMap<String, String>,
}

/// Files that are never looked up by bare name: metadata written next to the art
/// (`sprite_fx.txt`, `hotspots.txt`, `roofs.txt`) and notes (`README.md`).
fn indexed(name: &str) -> bool {
    let n = name.to_lowercase();
    !(n.ends_with(".txt") || n.ends_with(".md"))
}

impl FileIndex {
    /// Walks `<root>/content`; paths are relative to `root` with `/` separators. Two indexed
    /// files with the same case-insensitive name are an error (a bare name must be unambiguous).
    pub fn scan(root: &Path) -> anyhow::Result<Self> {
        let mut index = Self::default();
        let mut stack = vec![root.join("content")];
        let mut dupes = Vec::new();
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir).map_err(|e| anyhow::anyhow!("{}: {e}", dir.display()))?;
            for e in entries.flatten() {
                let path = e.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if !indexed(&name) {
                    continue;
                }
                let Ok(rel) = path.strip_prefix(root) else { continue };
                let rel = rel.to_string_lossy().replace('\\', "/");
                if let Some(old) = index.map.insert(name.to_lowercase(), rel.clone()) {
                    dupes.push(format!("{old} and {rel}"));
                }
            }
        }
        if !dupes.is_empty() {
            dupes.sort();
            anyhow::bail!("duplicate file names under {}/content: {}", root.display(), dupes.join(", "));
        }
        Ok(index)
    }

    /// Adds (or replaces) a basename -> relative path entry.
    pub fn insert(&mut self, name: &str, rel: &str) {
        self.map.insert(name.to_lowercase(), rel.replace('\\', "/"));
    }

    /// Asset path relative to the asset root.
    pub fn resolve(&self, name: &str) -> Option<&str> {
        self.map.get(&name.to_lowercase()).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Every `(lowercase name, path)`.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.map.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(name: &str, files: &[&str]) -> PathBuf {
        let root = std::env::temp_dir().join(format!("dusk_index_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for f in files {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        root
    }

    #[test]
    fn scan_indexes_bare_names() {
        let root = tree(
            "ok",
            &["content/ui/A.png", "content/sfx/hit.wav", "content/env/sprite_fx.txt", "content/vale/sprite_fx.txt"],
        );
        let index = FileIndex::scan(&root).unwrap();
        assert_eq!(index.resolve("a.PNG"), Some("content/ui/A.png"));
        assert_eq!(index.resolve("hit.wav"), Some("content/sfx/hit.wav"));
        assert_eq!(index.resolve("sprite_fx.txt"), None, "metadata is not indexed");
        assert_eq!(index.len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn scan_rejects_duplicate_names() {
        let root = tree("dupe", &["content/ui/a.png", "content/sprites/A.png"]);
        let err = FileIndex::scan(&root).unwrap_err().to_string();
        assert!(err.contains("duplicate") && err.contains("a.png") && err.contains("A.png"), "{err}");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn shipped_content_has_unique_names() {
        let index = FileIndex::scan(&assets_root()).unwrap();
        assert!(index.len() > 1000);
    }
}
