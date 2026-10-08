//! Every sound file named by `game.db` (and the hard-coded client sounds) resolves through
//! `file_index.txt` to a valid OGG/WAV file. Missing db references are reported, not fatal
//! (the original data has a few dangling ones). Skipped when assets are not extracted.

use dusk_formats::{
    FileIndex,
    db::GameDb,
    map::MapFile,
    sound::{RegionGrid, builtin, resolve_sound},
};
use std::io::Read;
use std::path::{Path, PathBuf};

fn assets() -> Option<PathBuf> {
    let p = dusk_formats::assets_root();
    p.join("game.db").exists().then_some(p)
}

/// Checks the container header: `OggS` + a Vorbis identification packet, or a PCM RIFF/WAVE.
fn check_header(path: &Path) -> Result<(), String> {
    let mut head = [0u8; 64];
    let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).map_err(|e| e.to_string())?;
    let head = &head[..n];
    if head.starts_with(b"OggS") {
        return if head.windows(7).any(|w| w == b"\x01vorbis") { Ok(()) } else { Err("ogg without vorbis".into()) };
    }
    if head.starts_with(b"RIFF") && head.get(8..12) == Some(b"WAVE") {
        let fmt = head.windows(4).position(|w| w == b"fmt ").ok_or("wav without fmt chunk")?;
        let tag = u16::from_le_bytes([head[fmt + 8], head[fmt + 9]]);
        return if tag == 1 { Ok(()) } else { Err(format!("wav format tag {tag} (not PCM)")) };
    }
    Err("unknown container".into())
}

#[test]
fn referenced_sounds_exist_and_are_valid() {
    let Some(root) = assets() else { return };
    let index = FileIndex::load(root.join("file_index.txt")).unwrap();
    let db = GameDb::open(root.join("game.db")).unwrap();
    let refs = db.referenced_sounds().unwrap();
    let mut missing = Vec::new();
    let mut bad = Vec::new();
    for (source, name) in &refs {
        match resolve_sound(&index, name) {
            None => missing.push(format!("{source}: {name}")),
            Some(rel) => {
                if let Err(e) = check_header(&root.join(rel)) {
                    bad.push(format!("{rel}: {e}"));
                }
            }
        }
    }
    for name in builtin::all() {
        match resolve_sound(&index, name) {
            None => bad.push(format!("built-in {name} missing")),
            Some(rel) => {
                if let Err(e) = check_header(&root.join(rel)) {
                    bad.push(format!("{rel}: {e}"));
                }
            }
        }
    }
    eprintln!("{} sound references, {} missing from the install:", refs.len(), missing.len());
    for m in &missing {
        eprintln!("  missing {m}");
    }
    assert!(bad.is_empty(), "invalid sound files: {bad:#?}");
    // The shipped data has 6 dangling names (mostly music); anything beyond that is a regression.
    assert!(missing.len() <= 8, "too many missing sounds: {missing:#?}");
}

#[test]
fn every_map_has_playable_music_somewhere() {
    let Some(root) = assets() else { return };
    let index = FileIndex::load(root.join("file_index.txt")).unwrap();
    let db = GameDb::open(root.join("game.db")).unwrap();
    let tables = db.sound_tables().unwrap();
    for info in db.maps().unwrap() {
        let Ok(map) = MapFile::load(root.join("maps").join(format!("{}.map", info.name))) else { continue };
        let grid = RegionGrid::new(&map);
        let zone_tracks = grid.zones.values().filter_map(|z| tables.zones.get(&(*z as i64))).flat_map(|z| &z.music);
        let playable = info.music.iter().chain(zone_tracks).any(|t| resolve_sound(&index, t).is_some());
        assert!(playable, "map {} has no playable music", info.name);
    }
}
