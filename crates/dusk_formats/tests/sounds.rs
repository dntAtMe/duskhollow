//! Every sound our data and the client name (`content::sounds::referenced`: builtin event
//! sounds and cues, spell kit sounds, sprite_sounds.txt, map music, the soundtrack, NPC voices)
//! is one of our files with a valid container.

use dusk_formats::{content::sounds, assets_root, sound::resolve_sound};
use std::io::Read;
use std::path::Path;

/// Checks the container header: a PCM RIFF/WAVE, `OggS` + a Vorbis identification packet, or
/// MP3 (ID3 tag or a frame sync).
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
    if head.starts_with(b"ID3") || (head.len() > 1 && head[0] == 0xFF && head[1] & 0xE0 == 0xE0) {
        return Ok(());
    }
    Err("unknown container".into())
}

#[test]
fn referenced_sounds_exist_and_are_valid() {
    let root = assets_root();
    let index = sounds::index(&root);
    let refs = sounds::referenced(&root, &index).unwrap();
    let mut problems = Vec::new();
    for (source, name) in &refs {
        match resolve_sound(&index, name) {
            None => problems.push(format!("{source}: {name} missing")),
            Some(rel) => {
                if let Err(e) = check_header(&root.join(rel)) {
                    problems.push(format!("{source}: {rel}: {e}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
    let sources = |prefix: &str| refs.iter().filter(|(s, _)| s.starts_with(prefix)).count();
    assert_eq!(sources("builtin"), 27 + dusk_formats::sound::builtin::CUES.len());
    assert!(sources("soundtrack") > 0, "no soundtrack");
    assert!(sources("sprite_sounds.txt") > 0);
    assert!(sources("npc_templates.txt") >= 12, "NPC voices: {}", sources("npc_templates.txt"));
}

/// The generated effects stay within the size budget of `tools/sfxgen` (16 MB).
#[test]
fn sfx_size_budget() {
    let dir = assets_root().join("content/sfx");
    let total: u64 = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "wav"))
        .map(|e| e.metadata().unwrap().len())
        .sum();
    assert!(total <= 16 * 1024 * 1024, "{} has {:.2} MB of effects", dir.display(), total as f64 / 1048576.0);
}

#[test]
fn sprite_sounds_parse() {
    let tables = sounds::load(&assets_root()).unwrap();
    assert!(tables.sprite_sounds.iter().any(|s| s.pattern == "cg_campfire*" && s.sound == "loop_cairn_fire.wav"));
    assert!(tables.sprite_sounds.iter().all(|s| s.radius > 0.0));
}
