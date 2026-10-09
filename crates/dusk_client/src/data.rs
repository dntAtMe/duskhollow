//! Static game data loaded once at startup through the content seam (`dusk_formats::content`).
//!
//! Asset paths (`GameData::asset_path`) are relative to the asset root (Bevy's asset source).

use bevy::prelude::*;
use dusk_formats::{
    FileIndex, content,
    content::types::{MapInfo, NpcModel, NpcTemplate},
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
    /// The asset root (`dusk_formats::assets_root`).
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
    /// Map sprite particle emitters / lights (`sprite_fx.txt`) by lowercase sprite filename.
    pub sprite_psi: HashMap<String, Vec<SpritePsi>>,
    pub sprite_lights: HashMap<String, Vec<SpriteLight>>,
    /// Particle systems by `content::particles::key`.
    particles: HashMap<String, ParticleSystemInfo>,
    flipbooks: Mutex<HashMap<String, Option<Arc<SpriteAnim>>>>,
    image_sizes: Mutex<HashMap<String, Option<UVec2>>>,
    scripts: Mutex<HashMap<String, Option<Arc<SpriteScript>>>>,
}

impl GameData {
    /// `root`: the asset root.
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let index = FileIndex::scan(root)?;
        info!("indexed {} content files", index.len());
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
            particles: content::particles::load(root)?,
            flipbooks: default(),
            image_sizes: default(),
            scripts: default(),
        })
    }

    /// Filesystem path of an asset path from [`GameData::asset_path`].
    pub fn fs_path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// `rel` (e.g. `maps/x.map`, `scripts/npc/x.txt`) under the asset root, if it exists.
    pub fn find_file(&self, rel: &str) -> Option<PathBuf> {
        dusk_formats::find_file(&self.root, rel)
    }

    /// Particle system of `data/particles.txt` by name (`campfire`, `.psi` suffix ignored).
    pub fn particle_system(&self, name: &str) -> Option<ParticleSystemInfo> {
        self.particles.get(&content::particles::key(name)).copied()
    }

    /// Loads a `.sa` flipbook by name (`content::visuals::flipbook_path`), cached.
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

    /// Asset path for a bare file name, relative to the asset root.
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

    /// Pivot of a map sprite: `hotspots.txt` next to the art, else `(w / 2, h / 1.25)`.
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
