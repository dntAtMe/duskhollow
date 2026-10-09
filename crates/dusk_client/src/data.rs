//! Static game data loaded once at startup through the content seam (`dusk_formats::content`).
//!
//! Asset paths (`GameData::asset_path`) are relative to our content root (Bevy's default asset
//! source) or `legacy://...` for files of the legacy data pack (a second asset source).

use bevy::prelude::*;
use dusk_formats::{
    FileIndex, content,
    db::{MapInfo, NpcModel, NpcTemplate},
    psi::ParticleSystemInfo,
    spell::{SpellTemplate, SpellVisual},
    sprite_anim::SpriteAnim,
    sprite_fx::{SpriteLight, SpritePsi},
    sprite_script::SpriteScript,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[derive(Resource)]
pub struct GameData {
    /// Our content root (`dusk_formats::content_root`).
    pub root: PathBuf,
    pub index: FileIndex,
    pub maps: Vec<MapInfo>,
    pub npc_models: HashMap<i64, NpcModel>,
    pub npc_templates: HashMap<i64, NpcTemplate>,
    pub hotspots: HashMap<String, (i32, i32)>,
    pub spells: HashMap<i64, SpellTemplate>,
    /// Roof sprites from `roofs.txt`: (lowercase name prefix, cells roofed beyond the back cell).
    pub roofs: Vec<(String, IVec2)>,
    pub spell_visuals: HashMap<i64, SpellVisual>,
    /// `sprite_psi` / `sprite_light` by lowercase sprite filename.
    pub sprite_psi: HashMap<String, Vec<SpritePsi>>,
    pub sprite_lights: HashMap<String, Vec<SpriteLight>>,
    /// `zone_template.night_pct` by zone id.
    pub zone_night: HashMap<u32, f32>,
    /// Particle systems by `content::particles::key`.
    particles: HashMap<String, ParticleSystemInfo>,
    flipbooks: Mutex<HashMap<String, Option<Arc<SpriteAnim>>>>,
    image_sizes: Mutex<HashMap<String, Option<UVec2>>>,
    scripts: Mutex<HashMap<String, Option<Arc<SpriteScript>>>>,
}

/// Indexes every file under `<root>/content` by bare name over the legacy index (ours win).
/// Files under `content/override/` keep the bare names the data refers to (icons, interface art).
fn index_content(root: &Path, index: &mut FileIndex) {
    let mut found = 0;
    let mut stack = vec![root.join("content")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let Ok(rel) = path.strip_prefix(root) else { continue };
            let rel = rel.to_string_lossy().replace('\\', "/");
            found += 1;
            index.insert(&e.file_name().to_string_lossy(), &rel);
        }
    }
    info!("indexed {found} content files");
}

impl GameData {
    /// `root`: our content root.
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let mut index = FileIndex::load_legacy();
        index_content(root, &mut index);
        let npcs = content::npcs::load(root)?;
        let spells = content::spells::load(root)?;
        let spell_visuals = content::visuals::load(root, &spells)?;
        let fx = content::sprite_fx::load(root)?;
        Ok(Self {
            root: root.to_path_buf(),
            index,
            maps: content::maps::load(root)?,
            npc_models: npcs.models,
            npc_templates: npcs.templates,
            hotspots: fx.hotspots,
            spells,
            roofs: fx.roofs.into_iter().map(|(name, (x, y))| (name, IVec2::new(x, y))).collect(),
            spell_visuals,
            sprite_psi: fx.psi,
            sprite_lights: fx.lights,
            zone_night: fx.zone_night,
            particles: content::particles::load(root)?,
            flipbooks: default(),
            image_sizes: default(),
            scripts: default(),
        })
    }

    /// Filesystem path of an asset path from [`GameData::asset_path`] (`legacy://` included).
    pub fn fs_path(&self, rel: &str) -> PathBuf {
        match rel.strip_prefix(dusk_formats::LEGACY_SOURCE) {
            Some(_) => dusk_formats::fs_path(rel),
            None => self.root.join(rel),
        }
    }

    /// `rel` (e.g. `maps/x.map`, `scripts/npc/x.txt`) from our content root, else from the legacy
    /// data (logged with `DUSK_LEGACY_LOG=1`).
    pub fn find_file(&self, rel: &str) -> Option<PathBuf> {
        dusk_formats::find_file(&self.root, rel)
    }

    /// Particle system by name (`campfire.psi` or `campfire`).
    pub fn particle_system(&self, name: &str) -> Option<ParticleSystemInfo> {
        let key = content::particles::key(name);
        let info = self.particles.get(&key).copied();
        if info.is_some() {
            dusk_formats::legacy_note("particles", &key);
        }
        info
    }

    /// Loads a `.sa` flipbook by name (`content::visuals::flipbook_path`: ours first), cached.
    pub fn flipbook(&self, name: &str) -> Option<Arc<SpriteAnim>> {
        let mut cache = self.flipbooks.lock().unwrap();
        cache
            .entry(name.to_string())
            .or_insert_with(|| {
                let path = content::visuals::flipbook_path(&self.root, name)?;
                let text = std::fs::read_to_string(path).ok()?;
                SpriteAnim::parse(&text).ok().map(Arc::new)
            })
            .clone()
    }

    /// Asset path for a bare file name: relative to our content root, or `legacy://...`.
    pub fn asset_path(&self, name: &str) -> Option<String> {
        self.index.resolve(name).map(str::to_string)
    }

    /// PNG dimensions read from the file header (no decode).
    pub fn image_size(&self, name: &str) -> Option<UVec2> {
        let key = name.to_lowercase();
        let mut cache = self.image_sizes.lock().unwrap();
        *cache.entry(key).or_insert_with(|| {
            let path = self.fs_path(self.index.resolve(name)?);
            let mut header = [0u8; 24];
            use std::io::Read;
            std::fs::File::open(path).ok()?.read_exact(&mut header).ok()?;
            (&header[1..4] == b"PNG").then(|| {
                UVec2::new(
                    u32::from_be_bytes(header[16..20].try_into().unwrap()),
                    u32::from_be_bytes(header[20..24].try_into().unwrap()),
                )
            })
        })
    }

    /// Pivot of a map sprite: `sprite_hotspot` (plus `hotspots.txt` files shipped with our own
    /// art), else the original client's default `(w / 2, h / 1.25)` (`Sprite::renderScript`).
    pub fn hotspot(&self, name: &str) -> Option<Vec2> {
        if let Some(&(x, y)) = self.hotspots.get(&name.to_lowercase()) {
            return Some(Vec2::new(x as f32, y as f32));
        }
        let size = self.image_size(name)?;
        Some(Vec2::new(size.x as f32 / 2.0, size.y as f32 / 1.25))
    }

    /// Loads `scripts/<dir>/<name>.txt`, cached.
    pub fn sprite_script(&self, dir: &str, name: &str) -> Option<Arc<SpriteScript>> {
        let key = format!("{dir}/{name}");
        let mut cache = self.scripts.lock().unwrap();
        cache
            .entry(key)
            .or_insert_with(|| {
                let path = self.find_file(&format!("scripts/{dir}/{name}.txt"))?;
                let text = std::fs::read_to_string(&path).ok()?;
                match SpriteScript::parse(&text) {
                    Ok(s) => Some(Arc::new(s)),
                    Err(e) => {
                        warn!("{}: {e}", path.display());
                        None
                    }
                }
            })
            .clone()
    }
}
