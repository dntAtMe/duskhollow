//! Static game data loaded once at startup from the extracted assets.

use bevy::prelude::*;
use dusk_formats::{
    FileIndex,
    db::{GameDb, MapInfo, NpcModel, NpcTemplate},
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
    pub root: PathBuf,
    pub index: FileIndex,
    pub maps: Vec<MapInfo>,
    pub npc_models: HashMap<i64, NpcModel>,
    pub npc_templates: HashMap<i64, NpcTemplate>,
    pub hotspots: HashMap<String, (i32, i32)>,
    pub spells: HashMap<i64, SpellTemplate>,
    /// `--art custom`: render players (and NPC models that have one) with our own `tools/artgen`
    /// sprites instead of the original art.
    pub custom_art: bool,
    pub spell_visuals: HashMap<i64, SpellVisual>,
    /// `sprite_psi` / `sprite_light` by lowercase sprite filename.
    pub sprite_psi: HashMap<String, Vec<SpritePsi>>,
    pub sprite_lights: HashMap<String, Vec<SpriteLight>>,
    /// `zone_template.night_pct` by zone id.
    pub zone_night: HashMap<u32, f32>,
    flipbooks: Mutex<HashMap<String, Option<Arc<SpriteAnim>>>>,
    image_sizes: Mutex<HashMap<String, Option<UVec2>>>,
    scripts: Mutex<HashMap<String, Option<Arc<SpriteScript>>>>,
}

/// Installs `custom_assets` (our own art and maps) into the asset root and indexes its images.
/// Returns whether any custom content exists.
///
/// Files under `content/override/` carry ORIGINAL file names (icons, interface art, spell
/// flipbook frames...) and replace those originals in the index only when `overrides` is set
/// (`--art custom`); everything else has custom-only names and is always indexed.
fn install_custom_assets(root: &Path, index: &mut FileIndex, overrides: bool) -> bool {
    match dusk_formats::custom::install(&dusk_formats::custom_assets_root(), root) {
        Ok(files) => {
            let mut replaced = 0;
            for (name, rel) in &files {
                let is_override = rel.starts_with("content/override/");
                if !is_override || overrides {
                    index.insert(name, rel);
                    replaced += is_override as usize;
                }
            }
            if replaced > 0 {
                info!("custom art replaces {replaced} original files");
            }
            !files.is_empty()
        }
        Err(e) => {
            warn!("custom assets not installed: {e}");
            false
        }
    }
}

/// Contents of every file called `file_name` under `content/custom` (metadata written by tools/artgen).
fn custom_metadata(root: &Path, file_name: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("content/custom")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.file_name().is_some_and(|n| n == file_name) {
                out.push(std::fs::read_to_string(&p).unwrap_or_default());
            }
        }
    }
    out
}

/// Adds custom-art effects to a table loaded from the db.
fn with_custom<T>(mut table: HashMap<String, Vec<T>>, extra: Vec<(String, T)>) -> HashMap<String, Vec<T>> {
    for (k, v) in extra {
        table.entry(k).or_default().push(v);
    }
    table
}

/// `name x y` lines from the custom `hotspots.txt` files.
fn custom_hotspots(root: &Path) -> Vec<(String, (i32, i32))> {
    let text = custom_metadata(root, "hotspots.txt").join("\n");
    text.lines()
        .filter_map(|line| {
            let v: Vec<&str> = line.split_whitespace().collect();
            let [name, x, y] = v[..] else { return None };
            Some((name.to_lowercase(), (x.parse().ok()?, y.parse().ok()?)))
        })
        .collect()
}

impl GameData {
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        let mut index = FileIndex::load(root.join("file_index.txt"))?;
        let art_requested = std::env::var("DUSK_ART").is_ok_and(|v| v == "custom")
            || std::env::args().collect::<Vec<_>>().windows(2).any(|w| w[0] == "--art" && w[1] == "custom");
        let custom = install_custom_assets(root, &mut index, art_requested);
        let db = GameDb::open(root.join("game.db"))?;
        let custom_fx = dusk_formats::sprite_fx::parse_custom_fx(&custom_metadata(root, "sprite_fx.txt").join("\n"));
        Ok(Self {
            root: root.to_path_buf(),
            index,
            maps: db.maps()?,
            npc_models: db.npc_models()?,
            npc_templates: db.npc_templates()?,
            hotspots: db.sprite_hotspots()?.into_iter().chain(custom_hotspots(root)).collect(),
            spells: db.spells()?,
            custom_art: custom && art_requested,
            spell_visuals: db.spell_visuals()?,
            sprite_psi: with_custom(db.sprite_psi()?, custom_fx.0),
            sprite_lights: with_custom(db.sprite_lights()?, custom_fx.1),
            zone_night: db.zone_night_pct()?,
            flipbooks: default(),
            image_sizes: default(),
            scripts: default(),
        })
    }

    /// Loads `scripts/animation/<name>` (a `.sa` flipbook), cached. With `--art custom`, a
    /// replacement in `scripts/override/animation/` wins (custom_assets never overwrites originals).
    pub fn flipbook(&self, name: &str) -> Option<Arc<SpriteAnim>> {
        let mut cache = self.flipbooks.lock().unwrap();
        cache
            .entry(name.to_string())
            .or_insert_with(|| {
                let custom = self.root.join("scripts/override/animation").join(name);
                let path = if self.custom_art && custom.exists() {
                    custom
                } else {
                    self.root.join("scripts/animation").join(name)
                };
                let text = std::fs::read_to_string(path).ok()?;
                SpriteAnim::parse(&text).ok().map(Arc::new)
            })
            .clone()
    }

    /// Asset path (relative to assets root) for a bare original filename.
    pub fn asset_path(&self, name: &str) -> Option<String> {
        self.index.resolve(name).map(str::to_string)
    }

    /// PNG dimensions read from the file header (no decode).
    pub fn image_size(&self, name: &str) -> Option<UVec2> {
        let key = name.to_lowercase();
        let mut cache = self.image_sizes.lock().unwrap();
        *cache.entry(key).or_insert_with(|| {
            let path = self.index.resolve_path(&self.root, name)?;
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
                let path = self.root.join("scripts").join(dir).join(format!("{name}.txt"));
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
