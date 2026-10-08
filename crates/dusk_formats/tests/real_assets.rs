//! Parses every original file in `assets/`. Skipped when assets are not extracted.

use dusk_formats::{FileIndex, db::GameDb, map::MapFile, sprite_anim::SpriteAnim, sprite_script::SpriteScript};
use std::path::{Path, PathBuf};

fn assets() -> Option<PathBuf> {
    let p = dusk_formats::assets_root();
    p.join("game.db").exists().then_some(p)
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(files(&p, ext));
        } else if p.extension().is_some_and(|e| e == ext) {
            out.push(p);
        }
    }
    out
}

#[test]
fn all_maps_parse_and_textures_resolve() {
    let Some(root) = assets() else { return };
    let index = FileIndex::load(root.join("file_index.txt")).unwrap();
    // Our own maps (mirrored from custom_assets) are checked by `custom_maps_parse_and_resolve`.
    let originals = files(&root.join("maps"), "map")
        .into_iter()
        .filter(|f| !f.file_stem().unwrap().to_string_lossy().starts_with(dusk_formats::custom::CUSTOM_MAP_PREFIX));
    for f in originals {
        let m = MapFile::load(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let missing: Vec<_> = m.textures.iter().filter(|t| index.resolve(t).is_none()).collect();
        // .psi particle systems are not images; tolerate those.
        let missing: Vec<_> = missing.into_iter().filter(|t| !t.ends_with(".psi")).collect();
        assert!(missing.len() <= 2, "{}: unresolved textures {missing:?}", f.display());
    }
}

#[test]
fn all_sprite_scripts_parse() {
    let Some(root) = assets() else { return };
    let index = FileIndex::load(root.join("file_index.txt")).unwrap();
    let (mut total, mut missing) = (0, Vec::new());
    for dir in ["scripts/npc", "scripts/player"] {
        // `custom/` holds our own scripts mirrored in by `custom::install`; their sheets aren't indexed.
        for f in
            files(&root.join(dir), "txt").into_iter().filter(|f| !f.components().any(|c| c.as_os_str() == "custom"))
        {
            let s = SpriteScript::parse(&std::fs::read_to_string(&f).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            total += 1;
            // A few scripts reference sheets not shipped with the game (cut content).
            if !s.image.is_empty() && index.resolve(&s.image).is_none() {
                missing.push(s.image);
            }
        }
    }
    assert!(missing.len() * 10 < total, "too many missing sheets: {missing:?}");
}

#[test]
fn all_sprite_anims_parse() {
    let Some(root) = assets() else { return };
    for f in files(&root.join("scripts/animation"), "sa") {
        SpriteAnim::parse(&std::fs::read_to_string(&f).unwrap()).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
    }
}

#[test]
fn db_loads() {
    let Some(root) = assets() else { return };
    let db = GameDb::open(root.join("game.db")).unwrap();
    assert!(db.maps().unwrap().iter().any(|m| m.name == "fanadin"));
    assert!(!db.npc_templates().unwrap().is_empty());
    assert!(!db.npc_spawns(1).unwrap().is_empty());
    assert!(!db.sprite_hotspots().unwrap().is_empty());
}

#[test]
fn all_spell_formulas_evaluate() {
    use dusk_formats::spell::{FormulaVars, eval_formula};
    let Some(root) = assets() else { return };
    let db = GameDb::open(root.join("game.db")).unwrap();
    let spells = db.spells().unwrap();
    assert!(spells.len() > 300);
    let v = FormulaVars { clvl: 5.0, splvl: 1.0, value: 10.0, str: 10.0, agi: 10.0, wil: 10.0, int: 10.0, cur: 10.0 };
    let mut bad = Vec::new();
    for s in spells.values() {
        let formulas = [&s.mana_formula, &s.duration_formula].into_iter().chain(s.effects.iter().map(|e| &e.formula));
        for f in formulas {
            if let Err(e) = eval_formula(f, &v) {
                bad.push(format!("{} ({}): {f:?}: {e}", s.name, s.entry));
            }
        }
    }
    assert!(bad.is_empty(), "unparseable formulas:\n{}", bad.join("\n"));
    assert!(db.class_spells().unwrap()[&1].contains(&9));
}

#[test]
fn spell_visuals_load_and_animations_exist() {
    let Some(root) = assets() else { return };
    let db = GameDb::open(root.join("game.db")).unwrap();
    let visuals = db.spell_visuals().unwrap();
    let fireball = &visuals[&29];
    assert_eq!(fireball.impact.as_ref().unwrap().anims[0].sa, "fire_001.sa");
    let missing: Vec<_> = visuals
        .values()
        .flat_map(|v| [&v.traveling, &v.impact, &v.casting, &v.go])
        .flatten()
        .flat_map(|k| &k.anims)
        .filter(|a| !root.join("scripts/animation").join(&a.sa).exists())
        .map(|a| a.sa.clone())
        .collect();
    assert!(missing.len() < 10, "missing .sa scripts: {missing:?}");
}

#[test]
fn custom_maps_parse_and_resolve() {
    use dusk_formats::custom::{CUSTOM_MAP_PREFIX, parse_spawns};
    let dir = dusk_formats::custom_assets_root().join("maps");
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    let mut n = 0;
    for p in entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "map")) {
        assert!(p.file_stem().unwrap().to_string_lossy().starts_with(CUSTOM_MAP_PREFIX));
        let m = MapFile::load(&p).unwrap();
        assert_eq!(m.cells.len() as u32, m.size * m.size);
        // Every texture must exist in custom_assets/content (`.psi` entries are original particle systems).
        for t in m.textures.iter().filter(|t| !t.ends_with(".psi")) {
            let found = files(&dusk_formats::custom_assets_root().join("content"), "png")
                .iter()
                .any(|f| f.file_name().unwrap() == t.as_str());
            assert!(found, "{}: missing texture {t}", p.display());
        }
        let spawns = std::fs::read_to_string(p.with_extension("spawns")).unwrap_or_default();
        assert!(!parse_spawns(&spawns, 1, 0).is_empty(), "{}: no spawns", p.display());
        n += 1;
    }
    assert!(n > 0);
}

#[test]
fn all_particle_systems_parse_and_simulate() {
    use dusk_formats::psi::{MAX_PARTICLES, ParticleSystem, ParticleSystemInfo};
    let Some(root) = assets() else { return };
    let all = files(&root.join("scripts/particles"), "psi");
    assert!(all.len() >= 40);
    for f in all {
        let info = ParticleSystemInfo::load(&f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let (tx, ty) = info.texture_origin();
        assert!(tx < 128 && ty < 128, "{}: frame outside particles.png", f.display());
        assert!(info.emission > 0 && info.emission <= 1000, "{}", f.display());
        let mut s = ParticleSystem::new(info, 7);
        for i in 0..300 {
            s.set_position(i as f32, 0.0, i % 2 == 0);
            s.update(1.0 / 60.0);
            assert!(s.particles.len() <= MAX_PARTICLES);
        }
        assert!(!s.particles.is_empty(), "{}: nothing emitted", f.display());
        assert!(s.particles.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()));
    }
}

#[test]
fn sprite_effects_reference_existing_files() {
    let Some(root) = assets() else { return };
    let db = GameDb::open(root.join("game.db")).unwrap();
    let psi = db.sprite_psi().unwrap();
    assert_eq!(psi["campfire_01.png"][0].psi, "campfire.psi");
    assert_eq!(psi["medieval-tavern_10000.png"].len(), 2);
    for v in psi.values().flatten() {
        assert!(root.join("scripts/particles").join(&v.psi).exists(), "{}", v.psi);
    }
    let lights = db.sprite_lights().unwrap();
    let fire = &lights["campfire_01.png"][0];
    assert_eq!((fire.color, fire.apply_ground, fire.apply_top, fire.scale), (0xe25822c8, true, false, 1.0));
    assert!(db.zone_night_pct().unwrap()[&49] > 0.4);
    let visuals = db.spell_visuals().unwrap();
    let kits = visuals.values().flat_map(|v| [&v.traveling, &v.casting, &v.aura_ontop]).flatten();
    for k in kits.filter(|k| k.psystem.ends_with(".psi")) {
        assert!(root.join("scripts/particles").join(&k.psystem).exists(), "{}", k.psystem);
    }
}

#[test]
fn items_and_loot_load() {
    use dusk_formats::item::{self, equip, quality};
    let Some(root) = assets() else { return };
    let db = GameDb::open(root.join("game.db")).unwrap();
    let items = db.items().unwrap();
    assert!(items.len() > 17000, "{}", items.len());
    let blade = &items[&18];
    assert_eq!((blade.name.as_str(), blade.equip_type, blade.model.as_str()), ("Blade", equip::WEAPON, "shortsword"));
    assert_eq!(blade.quality, quality::COMMON);
    // Generated grid: 5 qualities x 25 levels for every base item.
    let generated = items.values().filter(|t| t.generated).count();
    assert!(generated >= 16000, "{generated}");
    // Every starting item exists, every model has a paper-doll script.
    let start = db.starting_items().unwrap();
    assert_eq!(start.len(), 4);
    for (item, _) in start.values().flatten() {
        let t = &items[item];
        if t.has_model() {
            assert!(root.join(format!("scripts/player/male/{}.txt", t.model)).exists(), "{}", t.model);
        }
    }
    // Equippable items resolve to a slot and an icon; weapons get a value and a speed.
    let index = FileIndex::load(root.join("file_index.txt")).unwrap();
    let mut missing_icons = 0;
    for t in items.values().filter(|t| t.is_equippable()) {
        if index.resolve(&t.icon).is_none() {
            missing_icons += 1;
        }
        if t.has_model() && !root.join(format!("scripts/player/male/{}.txt", t.model)).exists() {
            panic!("no paper-doll script for {} ({})", t.name, t.model);
        }
        let s = item::item_stats(t, None);
        if matches!(t.equip_type, equip::WEAPON | equip::RANGED) {
            assert!(s.weapon_value > 0 && s.speed_ms > 0, "{}", t.name);
        }
    }
    assert!(missing_icons < 20, "{missing_icons} icons missing");
    let affixes = db.affixes().unwrap();
    assert!(affixes.len() > 7000);
    assert!(affixes.values().all(|a| !a.stats.is_empty() && a.min_level <= a.max_level));
    let loot = db.loot_tables().unwrap();
    // A few rows point at deleted items; the server skips those.
    let dangling = loot.values().flatten().filter(|r| !items.contains_key(&r.item)).count();
    assert!(dangling < 10, "{dangling} loot rows without an item");
    let junk = db.junk_loot().unwrap();
    assert!(junk.values().flatten().filter(|i| items.contains_key(i)).count() > 800);
    let npc_loot = db.npc_loot().unwrap();
    assert!(npc_loot.values().any(|l| l.custom_loot > 0));
    assert_eq!(db.class_armor().unwrap().len(), 4);
    assert!(!db.material_chances().unwrap().is_empty());
}
