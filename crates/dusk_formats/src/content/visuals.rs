//! Spell visuals: named visual kits (flipbooks, particles, sound, glows) and which kit plays when,
//! from `data/spell_visuals.txt`:
//!
//! ```text
//! [kit heavy_slash]
//! anim=slash_002c.sa      # .sa flipbook (scripts/animation)
//! anim_x=47               # canvas left edge relative to the unit's feet (px, + = left)
//! anim_y=23               # canvas bottom relative to the feet (px, + = down); may use `height`
//! anim_color=ff00007f     # optional tint, rrggbbaa
//! anim_blend=0            # optional: 0 draws over everything when the unit has no depth
//! anim2=...               # optional second flipbook (anim2_x, anim2_y, anim2_color, anim2_blend)
//! particles=fire_cast     # optional particle system of data/particles.txt
//! particles_x=0           # emitter offset from the anchor (px, y down); may use `height`
//! particles_y=-height
//! sound=spell_heavy_slash # optional sound (bare name)
//! unit_glow=e0642a7f      # optional rrggbbaa: colour of a projectile without flipbook/particles
//! ground_glow=e0642a3f    # optional rrggbbaa (reserved)
//!
//! [spell 50001]
//! impact=heavy_slash      # traveling | impact | casting | go | aura = <kit name>
//! go_anim=swing           # unit animation on release: swing | cast | shoot | cast_alt | block | hit
//! cast_anim=cast          # unit animation while casting
//! ```

use super::sections::{self, Section};
use crate::spell::SpellTemplate;
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One flipbook of a visual kit (`anim` / `anim2`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KitAnim {
    /// `.sa` script name, e.g. `slash_001.sa`.
    pub sa: String,
    /// Canvas left edge relative to the unit's feet (px, positive = left).
    pub x: i64,
    /// Canvas bottom relative to the feet (px, screen-down positive); may reference `height`.
    pub y: String,
    /// Packed RGBA tint (-1 = none).
    pub color: i64,
    pub blend: i64,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VisualKit {
    /// `heavy_slash` in `[kit heavy_slash]`.
    pub name: String,
    pub anims: Vec<KitAnim>,
    /// Particle system of `data/particles.txt` (empty = none).
    pub particles: String,
    /// Emitter offset from the kit's anchor (px, y down); may reference `height`.
    pub particles_x: String,
    pub particles_y: String,
    /// Sound name (empty = none).
    pub sound: String,
    /// Packed RGBA (-1 = none).
    pub unit_glow: i64,
    pub ground_glow: i64,
}

/// Which kits a spell plays, resolved.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SpellVisual {
    pub traveling: Option<VisualKit>,
    pub impact: Option<VisualKit>,
    pub casting: Option<VisualKit>,
    pub go: Option<VisualKit>,
    /// Shown on a unit while the spell's aura is on it.
    pub aura_ontop: Option<VisualKit>,
    /// Unit animation ids (2 Shoot, 6 Cast, 7 Swing, 8 Hit, 9 Block, 10 CastAlt; 0 = none).
    pub unit_go_animation: i64,
    pub unit_cast_animation: i64,
}

/// Keys of a `[kit name]` section.
pub const KIT_KEYS: &[&str] = &[
    "anim",
    "anim_x",
    "anim_y",
    "anim_color",
    "anim_blend",
    "anim2",
    "anim2_x",
    "anim2_y",
    "anim2_color",
    "anim2_blend",
    "particles",
    "particles_x",
    "particles_y",
    "sound",
    "unit_glow",
    "ground_glow",
];

/// Keys of a `[spell N]` section.
pub const SPELL_VISUAL_KEYS: &[&str] = &["traveling", "impact", "casting", "go", "aura", "go_anim", "cast_anim"];

/// Unit animation by name (`swing`, `cast`, `shoot`, `cast_alt`, `block`, `hit`) or number
/// (2 Shoot, 6 Cast, 7 Swing, 8 Hit, 9 Block, 10 CastAlt).
pub fn unit_anim_id(v: &str) -> i64 {
    match v {
        "shoot" => 2,
        "cast" => 6,
        "swing" => 7,
        "hit" => 8,
        "block" => 9,
        "cast_alt" => 10,
        _ => v.parse().unwrap_or(0),
    }
}

/// `rrggbbaa` hex -> packed RGBA.
fn color(s: &Section, k: &str) -> anyhow::Result<i64> {
    match s.get(k) {
        None => Ok(-1),
        Some(v) => Ok(u32::from_str_radix(v, 16).with_context(|| format!("{k}: {v:?} is not rrggbbaa"))? as i64),
    }
}

fn int(s: &Section, k: &str) -> anyhow::Result<i64> {
    s.get(k).map_or(Ok(0), |v| v.parse().with_context(|| format!("{k}: {v:?}")))
}

fn parse_kit(s: &Section) -> anyhow::Result<VisualKit> {
    let mut anims = Vec::new();
    for p in ["anim", "anim2"] {
        let Some(sa) = s.get(p) else { continue };
        anims.push(KitAnim {
            sa: sa.to_string(),
            x: int(s, &format!("{p}_x"))?,
            y: s.get(&format!("{p}_y")).unwrap_or("0").to_string(),
            color: color(s, &format!("{p}_color"))?,
            blend: s.get(&format!("{p}_blend")).map_or(Ok(-1), |_| int(s, &format!("{p}_blend")))?,
        });
    }
    let text = |k: &str, d: &str| s.get(k).unwrap_or(d).to_string();
    Ok(VisualKit {
        name: s.id.clone(),
        anims,
        particles: text("particles", ""),
        particles_x: text("particles_x", "0"),
        particles_y: text("particles_y", "0"),
        sound: text("sound", ""),
        unit_glow: color(s, "unit_glow")?,
        ground_glow: color(s, "ground_glow")?,
    })
}

/// The `[kit name]` sections by name.
pub fn parse_kits(sections: &[Section]) -> anyhow::Result<HashMap<String, VisualKit>> {
    let mut out = HashMap::new();
    for s in sections.iter().filter(|s| s.kind.as_deref() == Some("kit")) {
        let kit = parse_kit(s).with_context(|| format!("[kit {}] (line {})", s.id, s.line))?;
        out.insert(s.id.clone(), kit);
    }
    Ok(out)
}

/// The `[spell N]` sections, kit names resolved against `kits` (an unknown name is an error).
pub fn parse_spell_visuals(
    sections: &[Section],
    kits: &HashMap<String, VisualKit>,
) -> anyhow::Result<HashMap<i64, SpellVisual>> {
    let mut out = HashMap::new();
    for s in sections {
        match s.kind.as_deref() {
            Some("kit") => continue,
            Some("spell") => {}
            _ => bail!("line {}: expected [kit <name>] or [spell <entry>], got [{}]", s.line, s.id),
        }
        let Some(entry) = s.id_int() else { bail!("line {}: bad spell id {:?}", s.line, s.id) };
        let kit = |k: &str| -> anyhow::Result<Option<VisualKit>> {
            let Some(name) = s.get(k) else { return Ok(None) };
            match kits.get(name) {
                Some(kit) => Ok(Some(kit.clone())),
                None => bail!("[spell {entry}] (line {}): {k}: unknown kit {name:?}", s.line),
            }
        };
        out.insert(
            entry,
            SpellVisual {
                traveling: kit("traveling")?,
                impact: kit("impact")?,
                casting: kit("casting")?,
                go: kit("go")?,
                aura_ontop: kit("aura")?,
                unit_go_animation: s.get("go_anim").map_or(0, unit_anim_id),
                unit_cast_animation: s.get("cast_anim").map_or(0, unit_anim_id),
            },
        );
    }
    Ok(out)
}

/// Spell entry -> visual, for the spells of `spells` that have a `[spell N]` section.
pub fn load(root: &Path, spells: &HashMap<i64, SpellTemplate>) -> anyhow::Result<HashMap<i64, SpellVisual>> {
    let s = sections::load(&root.join("data/spell_visuals.txt"))?;
    let kits = parse_kits(&s)?;
    let mut visuals = parse_spell_visuals(&s, &kits).context("data/spell_visuals.txt")?;
    visuals.retain(|entry, _| spells.contains_key(entry));
    Ok(visuals)
}

/// `.sa` flipbook script by name (`scripts/animation/`).
pub fn flipbook_path(root: &Path, name: &str) -> Option<PathBuf> {
    let own = root.join("scripts/animation").join(name);
    own.exists().then_some(own)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kits_and_spells_parse() {
        let s = sections::parse(
            "[kit cut]\nanim=slash_001.sa\nanim_x=47\nanim_y=-height+20\nanim2=b.sa\nanim2_blend=3\n\
             unit_glow=ff00007f\nsound=spell_cut\n[kit fire]\nparticles=fire_cast\nparticles_y=-height\n\
             [spell 50001]\nimpact=cut\ncasting=fire\ngo_anim=swing\ncast_anim=cast\n",
        )
        .unwrap();
        let kits = parse_kits(&s).unwrap();
        let cut = &kits["cut"];
        assert_eq!((cut.anims.len(), cut.anims[0].x, cut.anims[0].y.as_str()), (2, 47, "-height+20"));
        assert_eq!((cut.anims[0].blend, cut.anims[0].color, cut.anims[1].blend), (-1, -1, 3));
        assert_eq!((cut.unit_glow, cut.ground_glow, cut.sound.as_str()), (0xff00007f, -1, "spell_cut"));
        assert_eq!((kits["fire"].particles.as_str(), kits["fire"].particles_x.as_str()), ("fire_cast", "0"));
        let v = &parse_spell_visuals(&s, &kits).unwrap()[&50001];
        assert_eq!(v.impact.as_ref().map(|k| k.name.as_str()), Some("cut"));
        assert_eq!((v.unit_go_animation, v.unit_cast_animation, v.traveling.is_none()), (7, 6, true));
    }

    #[test]
    fn unknown_kits_are_errors() {
        let s = sections::parse("[spell 1]\nimpact=nope\n").unwrap();
        assert!(parse_spell_visuals(&s, &HashMap::new()).is_err());
        let s = sections::parse("[kit a]\nunit_glow=red\n").unwrap();
        assert!(parse_kits(&s).is_err());
        let s = sections::parse("[1]\nimpact=a\n").unwrap();
        assert!(parse_spell_visuals(&s, &HashMap::new()).is_err());
    }

    /// Every kit references an existing flipbook, particle system and (once generated) sound;
    /// every spell we ship has a visual.
    #[test]
    fn shipped_spell_visuals_resolve() {
        let root = crate::assets_root();
        let s = sections::load(&root.join("data/spell_visuals.txt")).unwrap();
        let (kit_sections, spell_sections): (Vec<Section>, Vec<Section>) =
            s.iter().cloned().partition(|s| s.kind.as_deref() == Some("kit"));
        assert_eq!(sections::unknown_keys(&kit_sections, KIT_KEYS), Vec::<String>::new());
        assert_eq!(sections::unknown_keys(&spell_sections, SPELL_VISUAL_KEYS), Vec::<String>::new());
        let kits = parse_kits(&s).unwrap();
        let particles = super::super::particles::load(&root).unwrap();
        for k in kits.values() {
            for a in &k.anims {
                let path = flipbook_path(&root, &a.sa).unwrap_or_else(|| panic!("kit {}: no {}", k.name, a.sa));
                let anim = crate::sprite_anim::SpriteAnim::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
                for (n, _, _) in &anim.frames {
                    let frame = anim.frame_file(*n);
                    assert!(
                        root.join("content/spellfx").join(&frame).exists(),
                        "kit {}: {} frame {frame}",
                        k.name,
                        a.sa
                    );
                }
            }
            if !k.particles.is_empty() {
                assert!(particles.contains_key(&k.particles), "kit {}: particles {}", k.name, k.particles);
            }
            assert!(!k.sound.contains('.'), "kit {}: sounds are bare names", k.name);
        }
        // Kit sounds are generated by tools/sfxgen; check them once any exists.
        let sfx = root.join("content/sfx");
        if sfx.join("spell_heavy_slash.wav").exists() {
            for k in kits.values().filter(|k| !k.sound.is_empty()) {
                assert!(sfx.join(format!("{}.wav", k.sound)).exists(), "kit {}: sound {}", k.name, k.sound);
            }
        }
        let visuals = parse_spell_visuals(&s, &kits).unwrap();
        let spells = sections::load(&root.join("data/spells.txt")).unwrap();
        for entry in spells.iter().filter_map(Section::id_int) {
            assert!(visuals.contains_key(&entry), "spell {entry} has no [spell {entry}] section");
        }
    }
}
