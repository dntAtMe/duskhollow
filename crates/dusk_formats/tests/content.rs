//! Every reference in our content resolves: map textures, spawns, NPC and item models (sprite
//! scripts and their sheets), portraits, icons, spell visual kits (flipbooks, frames, particles,
//! sounds), sprite effects, sprite sounds, fonts, music and the help text. All names go through
//! the same bare-name index the client uses (`FileIndex::scan`).

use dusk_formats::{
    FileIndex, assets_root,
    content::{self, sidecars::parse_spawns, types::faction},
    map::MapFile,
    sound::resolve_sound,
    sprite_anim::SpriteAnim,
    sprite_script::SpriteScript,
};
use std::collections::BTreeSet;
use std::path::Path;

struct Check {
    root: std::path::PathBuf,
    index: FileIndex,
    problems: BTreeSet<String>,
}

impl Check {
    fn new() -> Self {
        let root = assets_root();
        let index = FileIndex::scan(&root).expect("content index");
        Self { root, index, problems: BTreeSet::new() }
    }

    fn file(&mut self, at: &str, name: &str) -> bool {
        let ok = self.index.resolve(name).is_some();
        if !ok {
            self.problems.insert(format!("{at}: missing {name}"));
        }
        ok
    }

    /// `scripts/<dir>/<name>.txt` parses and its sheet resolves.
    fn script(&mut self, at: &str, dir: &str, name: &str) {
        let path = self.root.join(format!("scripts/{dir}/{name}.txt"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            self.problems.insert(format!("{at}: missing scripts/{dir}/{name}.txt"));
            return;
        };
        match SpriteScript::parse(&text) {
            Ok(s) if s.image.is_empty() => {
                self.problems.insert(format!("scripts/{dir}/{name}.txt: no image="));
            }
            Ok(s) => {
                self.file(&format!("scripts/{dir}/{name}.txt"), &s.image);
            }
            Err(e) => {
                self.problems.insert(format!("scripts/{dir}/{name}.txt: {e}"));
            }
        }
    }

    fn sound(&mut self, at: &str, name: &str) {
        if resolve_sound(&self.index, name).is_none() {
            self.problems.insert(format!("{at}: missing sound {name}"));
        }
    }

    fn finish(self) {
        assert!(self.problems.is_empty(), "{} problems:\n{}", self.problems.len(), self.problems.into_iter().collect::<Vec<_>>().join("\n"));
    }
}

fn files(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut out: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == ext))
        .collect();
    out.sort();
    out
}

#[test]
fn maps_resolve() {
    let mut c = Check::new();
    let maps = content::maps::load(&c.root).unwrap();
    let npcs = content::npcs::load(&c.root).unwrap();
    let fx = content::sprite_fx::load(&c.root).unwrap();
    let names: BTreeSet<String> = maps.iter().map(|m| m.name.clone()).collect();
    // Every map file is listed in data/maps.txt and the other way round.
    let on_disk: BTreeSet<String> = files(&c.root.join("maps"), "map")
        .iter()
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, on_disk, "data/maps.txt vs maps/*.map");
    for m in &maps {
        let at = format!("maps/{}.map", m.name);
        let file = MapFile::load(c.root.join(&at)).unwrap();
        assert_eq!(file.cells.len() as u32, file.size * file.size, "{at}");
        for t in file.textures.iter().chain(&file.terrain_textures) {
            // `.psi` entries are invisible sprites that only carry effects.
            if t.ends_with(".psi") {
                if !fx.psi.contains_key(&t.to_lowercase()) {
                    c.problems.insert(format!("{at}: effect sprite {t} has no sprite_fx.txt line"));
                }
            } else {
                c.file(&at, t);
            }
        }
        let spawns = std::fs::read_to_string(c.root.join(format!("maps/{}.spawns", m.name))).unwrap_or_default();
        for s in parse_spawns(&spawns, m.id, 0) {
            if !npcs.templates.contains_key(&s.entry) {
                c.problems.insert(format!("maps/{}.spawns: unknown NPC {}", m.name, s.entry));
            }
        }
        for track in m.music.iter().chain(std::iter::once(&m.ambience)).filter(|t| !t.is_empty()) {
            c.sound(&format!("data/maps.txt [{}]", m.name), track);
        }
    }
    c.finish();
}

#[test]
fn npcs_resolve() {
    let mut c = Check::new();
    let npcs = content::npcs::load(&c.root).unwrap();
    let mut templates: Vec<_> = npcs.templates.values().collect();
    templates.sort_by_key(|t| t.entry);
    for f in ["portrait_friendly.png", "portrait_hostile.png", "portrait_grey.png", "portrait_custom_adventurer.png"] {
        c.file("HUD", f);
    }
    for t in templates {
        let at = format!("data/npc_templates.txt [{}]", t.entry);
        let model = &npcs.models[&t.model_id].name;
        c.script(&at, "npc", model);
        if !t.portrait.is_empty() {
            c.file(&at, &format!("portrait_{}.png", t.portrait));
        } else if t.faction != faction::FRIENDLY && t.faction != faction::NEUTRAL {
            // Enemies show a close-up of their model in the target frame.
            c.file(&at, &format!("portrait_custom_{model}.png"));
        }
    }
    c.finish();
}

#[test]
fn every_sprite_script_parses() {
    let mut c = Check::new();
    for dir in ["npc", "player"] {
        for p in files(&c.root.join("scripts").join(dir), "txt") {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            c.script("scripts", dir, &name);
        }
    }
    c.finish();
}

#[test]
fn items_resolve() {
    let mut c = Check::new();
    let tables = content::items::load(&c.root).unwrap();
    let mut items: Vec<_> = tables.items.values().collect();
    items.sort_by_key(|t| t.entry);
    let mut models = BTreeSet::new();
    for t in items {
        let at = format!("item {} ({})", t.entry, t.name);
        c.file(&at, &t.icon);
        if t.has_model() {
            models.insert(t.model.clone());
        }
    }
    for m in models {
        c.script("item model", "player", &m);
    }
    c.finish();
}

#[test]
fn spells_and_visuals_resolve() {
    let mut c = Check::new();
    let spells = content::spells::load(&c.root).unwrap();
    let visuals = content::visuals::load(&c.root, &spells).unwrap();
    let particles = content::particles::load(&c.root).unwrap();
    let mut entries: Vec<_> = spells.keys().copied().collect();
    entries.sort();
    for e in entries {
        let s = &spells[&e];
        let at = format!("spell {e} ({})", s.name);
        c.file(&at, &s.icon);
        let Some(v) = visuals.get(&e) else {
            c.problems.insert(format!("{at}: no visual"));
            continue;
        };
        for kit in [&v.traveling, &v.impact, &v.casting, &v.go, &v.aura_ontop].into_iter().flatten() {
            let at = format!("kit {}", kit.name);
            for a in &kit.anims {
                let Some(path) = content::visuals::flipbook_path(&c.root, &a.sa) else {
                    c.problems.insert(format!("{at}: missing scripts/animation/{}", a.sa));
                    continue;
                };
                let anim = SpriteAnim::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
                for (n, _, _) in &anim.frames {
                    c.file(&format!("{at} {}", a.sa), &anim.frame_file(*n));
                }
            }
            if !kit.particles.is_empty() && !particles.contains_key(&content::particles::key(&kit.particles)) {
                c.problems.insert(format!("{at}: unknown particles {}", kit.particles));
            }
            if !kit.sound.is_empty() {
                c.sound(&at, &kit.sound);
            }
        }
    }
    // Every flipbook parses and has all its frames, used by a kit or not.
    for p in files(&c.root.join("scripts/animation"), "sa") {
        let at = p.file_name().unwrap().to_string_lossy().into_owned();
        match SpriteAnim::parse(&std::fs::read_to_string(&p).unwrap()) {
            Ok(anim) => {
                for (n, _, _) in &anim.frames {
                    c.file(&at, &anim.frame_file(*n));
                }
            }
            Err(e) => {
                c.problems.insert(format!("{at}: {e}"));
            }
        }
    }
    c.finish();
}

#[test]
fn sprite_fx_and_sounds_resolve() {
    let mut c = Check::new();
    let fx = content::sprite_fx::load(&c.root).unwrap();
    let particles = content::particles::load(&c.root).unwrap();
    let sprites: BTreeSet<&String> = fx.psi.keys().chain(fx.lights.keys()).chain(fx.hotspots.keys()).collect();
    for s in sprites.into_iter().filter(|s| !s.ends_with(".psi")) {
        c.file("sprite_fx.txt / hotspots.txt", s);
    }
    for (sprite, list) in &fx.psi {
        for p in list {
            if !particles.contains_key(&content::particles::key(&p.psi)) {
                c.problems.insert(format!("sprite_fx.txt {sprite}: unknown particles {}", p.psi));
            }
        }
    }
    // Every sprite sound pattern matches some map texture, and its sound exists.
    let mut textures = BTreeSet::new();
    for p in files(&c.root.join("maps"), "map") {
        textures.extend(MapFile::load(&p).unwrap().textures);
    }
    let tables = content::sounds::load(&c.root).unwrap();
    for s in &tables.sprite_sounds {
        if !textures.iter().any(|t| s.matches(t)) {
            c.problems.insert(format!("sprite_sounds.txt: {} matches no map texture", s.pattern));
        }
        c.sound("sprite_sounds.txt", &s.sound);
    }
    assert!(!tables.soundtrack.is_empty(), "no soundtrack");
    for t in &tables.soundtrack {
        c.sound("soundtrack", t);
    }
    c.finish();
}

#[test]
fn fonts_and_text_exist() {
    let mut c = Check::new();
    c.file("UI font", content::UI_FONT);
    c.file("UI font", content::UI_FONT_BOLD);
    let help = std::fs::read_to_string(c.root.join("data/help.txt")).expect("data/help.txt");
    assert!(help.lines().any(|l| !l.trim().is_empty()));
    c.finish();
}
