//! Particle system definitions by name (map emitters and spell kits).

use crate::psi::ParticleSystemInfo;
use std::collections::HashMap;
use std::path::Path;

/// Lookup key of a particle system name: lowercase, without a `.psi` extension.
pub fn key(name: &str) -> String {
    let lower = name.to_lowercase();
    lower.strip_suffix(".psi").map(str::to_string).unwrap_or(lower)
}

/// Every particle system, keyed by [`key`]. Stream 0: the legacy binary `.psi` files of
/// `scripts/particles/` (ours under the content root first, then legacy); unreadable files are
/// skipped.
pub fn load(root: &Path) -> anyhow::Result<HashMap<String, ParticleSystemInfo>> {
    let mut out = HashMap::new();
    let legacy = crate::legacy_root().join("scripts/particles");
    for dir in [root.join("scripts/particles"), legacy.clone()] {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        if dir == legacy {
            crate::legacy_note("dir", "scripts/particles/*.psi");
        }
        for e in entries.flatten() {
            let path = e.path();
            if !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("psi")) {
                continue;
            }
            let name = key(&e.file_name().to_string_lossy());
            if out.contains_key(&name) {
                continue;
            }
            if let Ok(info) = ParticleSystemInfo::load(&path) {
                out.insert(name, info);
            }
        }
    }
    Ok(out)
}
