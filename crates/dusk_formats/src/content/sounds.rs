//! Sound tables: looping sounds next to map sprites (`data/sprite_sounds.txt`) and the
//! soundtrack file list, plus [`referenced`], every sound our data and the client name.
//! Our files only.

use super::sections;
use crate::FileIndex;
use crate::sound::{MUSIC_DIR, SoundTables, VOICE_EVENTS, builtin, parse_sprite_sounds, split_playlist, voice_lines};
use std::path::Path;

pub fn load(root: &Path) -> anyhow::Result<SoundTables> {
    let path = root.join("data/sprite_sounds.txt");
    let sprite_sounds = match std::fs::read_to_string(&path) {
        Ok(text) => parse_sprite_sounds(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => anyhow::bail!("{}: {e}", path.display()),
    };
    let mut soundtrack: Vec<String> = std::fs::read_dir(root.join(MUSIC_DIR))
        .map(|d| d.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| is_sound(n)).collect())
        .unwrap_or_default();
    soundtrack.sort();
    Ok(SoundTables { sprite_sounds, soundtrack })
}

fn is_sound(name: &str) -> bool {
    let n = name.to_lowercase();
    [".wav", ".ogg", ".mp3"].iter().any(|ext| n.ends_with(ext))
}

/// Index of the sound files under `<root>/content` by bare name (paths relative to `root`), the
/// way the client's file index sees them.
pub fn index(root: &Path) -> FileIndex {
    let mut index = FileIndex::default();
    let mut stack = vec![root.join("content")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else if let Ok(rel) = path.strip_prefix(root)
                && is_sound(&e.file_name().to_string_lossy())
            {
                index.insert(&e.file_name().to_string_lossy(), &rel.to_string_lossy());
            }
        }
    }
    index
}

/// Every sound our data and the client name, as `(where, name)`: the [`builtin`] event sounds
/// and cues, kit sounds of `data/spell_visuals.txt`, `data/sprite_sounds.txt`, map music and
/// ambience of `data/maps.txt`, the soundtrack, and the voice lines of every model of
/// `data/npc_templates.txt` (all of [`VOICE_EVENTS`] for models that are not friendly, plus any
/// greet). A voice event without lines is listed as `npc_<model>_<event>` (which does not
/// resolve), so callers report it as missing.
pub fn referenced(root: &Path, index: &FileIndex) -> anyhow::Result<Vec<(String, String)>> {
    let mut out: Vec<(String, String)> = Vec::new();
    out.extend(builtin::all().into_iter().map(|n| ("builtin".to_string(), n.to_string())));
    out.extend(builtin::CUES.iter().map(|n| ("builtin cue".to_string(), n.to_string())));
    let data = root.join("data");
    let optional = |name: &str| -> anyhow::Result<Vec<sections::Section>> {
        let path = data.join(name);
        if path.exists() { sections::load(&path) } else { Ok(Vec::new()) }
    };
    for s in optional("spell_visuals.txt")? {
        let at =
            format!("spell_visuals.txt [{}{}]", s.kind.as_deref().map_or(String::new(), |k| format!("{k} ")), s.id);
        out.extend(s.all("sound").flat_map(split_playlist).map(|n| (at.clone(), n)));
    }
    for s in optional("maps.txt")? {
        for key in ["music", "ambience"] {
            out.extend(s.all(key).flat_map(split_playlist).map(|n| (format!("maps.txt [{}] {key}", s.id), n)));
        }
    }
    let tables = load(root)?;
    out.extend(tables.sprite_sounds.iter().map(|s| ("sprite_sounds.txt".to_string(), s.sound.clone())));
    out.extend(tables.soundtrack.iter().map(|t| ("soundtrack".to_string(), t.clone())));
    let mut models: Vec<(String, bool)> = optional("npc_templates.txt")?
        .iter()
        .filter_map(|s| Some((s.get("model")?.to_lowercase(), s.int("faction", 0) == 1)))
        .collect();
    models.sort();
    models.dedup_by(|a, b| {
        a.0 == b.0 && {
            b.1 &= a.1; // a model used by any non-friendly template needs combat voices
            true
        }
    });
    for (model, friendly) in models {
        let combat: &[&str] = if friendly { &[] } else { &VOICE_EVENTS };
        for event in combat.iter().chain(&["greet"]) {
            let lines = voice_lines(index, &model, event);
            let at = format!("npc_templates.txt model {model}");
            if lines.is_empty() && *event != "greet" {
                out.push((at.clone(), format!("npc_{model}_{event}")));
            }
            out.extend(lines.into_iter().map(|l| (at.clone(), l)));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
