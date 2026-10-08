//! Unpacks a legacy data pack into `assets/` so Bevy can load it.
//!
//! Usage: `cargo run -p dusk_extract -- GAME_DIR [OUT_DIR]`
//!
//! Layout produced:
//! - `game.db`, `maps/`, `scripts/` copied verbatim
//! - `content/<dir>/<zip stem>/...` for every zip under `content/`
//! - loose files under `content/` (fonts, icon.png, ...) copied verbatim
//! - `file_index.txt`: `basename<TAB>relative/path` — the original game resolves
//!   images by bare filename across all zips, so we keep the same lookup.

use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let game_dir = PathBuf::from(args.next().context("usage: dusk_extract GAME_DIR [OUT_DIR]")?);
    let out_dir = PathBuf::from(args.next().unwrap_or_else(|| "assets".into()));

    anyhow::ensure!(
        game_dir.join("game.db").exists() && game_dir.join("content").is_dir(),
        "{} does not look like a data pack (game.db + content/)",
        game_dir.display()
    );
    fs::create_dir_all(&out_dir)?;

    fs::copy(game_dir.join("game.db"), out_dir.join("game.db")).context("copy game.db")?;
    copy_tree(&game_dir.join("maps"), &out_dir.join("maps"))?;
    copy_tree(&game_dir.join("scripts"), &out_dir.join("scripts"))?;

    let content_src = game_dir.join("content");
    let content_out = out_dir.join("content");
    let mut zips = 0;
    walk(&content_src, &mut |path| {
        let rel = path.strip_prefix(&content_src).unwrap();
        if rel.components().any(|c| c.as_os_str() == "7z") {
            return Ok(()); // bundled 7-zip binaries for the map editor
        }
        match path.extension().and_then(|e| e.to_str()) {
            // add_textures_here.zip is an empty placeholder for the map editor
            Some("zip") if fs::metadata(path)?.len() == 0 => {}
            Some("zip") => {
                let dest = content_out.join(rel.with_extension(""));
                extract_zip(path, &dest).with_context(|| format!("extract {}", path.display()))?;
                zips += 1;
            }
            Some("bat") | Some("txt") => {}
            _ => {
                let dest = content_out.join(rel);
                fs::create_dir_all(dest.parent().unwrap())?;
                fs::copy(path, dest)?;
            }
        }
        Ok(())
    })?;
    println!("extracted {zips} zip archives");

    write_index(&out_dir)?;
    println!("done -> {}", out_dir.display());
    Ok(())
}

fn walk(dir: &Path, f: &mut dyn FnMut(&Path) -> Result<()>) -> Result<()> {
    let mut entries: Vec<_> = fs::read_dir(dir)?.collect::<io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        if e.file_type()?.is_dir() {
            walk(&p, f)?;
        } else {
            f(&p)?;
        }
    }
    Ok(())
}

fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    walk(src, &mut |p| {
        let d = dst.join(p.strip_prefix(src).unwrap());
        fs::create_dir_all(d.parent().unwrap())?;
        fs::copy(p, d)?;
        Ok(())
    })
}

fn extract_zip(zip_path: &Path, dest: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(fs::File::open(zip_path)?)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let Some(rel) = entry.enclosed_name() else { continue };
        let out = dest.join(rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)?;
            continue;
        }
        fs::create_dir_all(out.parent().unwrap())?;
        io::copy(&mut entry, &mut fs::File::create(&out)?)?;
    }
    Ok(())
}

/// Maps lowercase basename -> path relative to `out_dir` (forward slashes).
fn write_index(out_dir: &Path) -> Result<()> {
    let mut index: BTreeMap<String, String> = BTreeMap::new();
    let mut dupes = 0;
    walk(&out_dir.join("content"), &mut |p| {
        let name = p.file_name().unwrap().to_string_lossy().to_lowercase();
        let rel = p.strip_prefix(out_dir).unwrap().to_string_lossy().replace('\\', "/");
        if index.contains_key(&name) {
            dupes += 1;
        } else {
            index.insert(name, rel);
        }
        Ok(())
    })?;
    let body: String = index.iter().map(|(k, v)| format!("{k}\t{v}\n")).collect();
    fs::write(out_dir.join("file_index.txt"), body)?;
    println!("indexed {} files ({dupes} duplicate basenames, first kept)", index.len());
    Ok(())
}
