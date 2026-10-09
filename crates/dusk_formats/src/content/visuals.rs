//! Spell visuals: visual kits (flipbooks, particles, sound, glows) and which kit plays when.
//!
//! Stream 0: kits and the spell -> kit table come from the legacy data; our spells pick legacy
//! kits by id in `data/spell_visuals.txt` (`[spell N]` sections, [`SPELL_VISUAL_KEYS`]).

use super::sections::{self, Section};
use crate::spell::SpellTemplate;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One sprite animation of a visual kit (`spranim` / `spranim_2`).
#[derive(Debug, Clone, Default)]
pub struct KitAnim {
    /// `.sa` script name, e.g. `fire_001.sa`.
    pub sa: String,
    /// Canvas left edge relative to the unit's feet (px, positive = left).
    pub x: i64,
    /// Canvas bottom relative to the feet (px, screen-down positive); may reference `height`.
    pub y: String,
    /// Packed RGBA tint (-1 / 0 = none).
    pub color: i64,
    pub blend: i64,
}

#[derive(Debug, Clone, Default)]
pub struct VisualKit {
    pub id: i64,
    pub anims: Vec<KitAnim>,
    /// Particle system (`.psi`; other names such as `0` or a stray `.sa` are ignored).
    pub psystem: String,
    /// Emitter offset from the kit's anchor (px, y down); `psystem_y` may reference `height`.
    pub psystem_x: String,
    pub psystem_y: String,
    pub sound: String,
    pub unit_glow: i64,
    pub ground_glow: i64,
}

/// `spell_visual` row with its kits resolved.
#[derive(Debug, Clone, Default)]
pub struct SpellVisual {
    pub traveling: Option<VisualKit>,
    pub impact: Option<VisualKit>,
    pub casting: Option<VisualKit>,
    pub go: Option<VisualKit>,
    /// Shown on a unit while the spell's aura is on it.
    pub aura_ontop: Option<VisualKit>,
    /// Unit animation ids (enum: 4 Die, 5 CritDie, 6 Cast, 7 Swing, 8 Hit, 9 Block, 10 CastAlt).
    pub unit_go_animation: i64,
    pub unit_cast_animation: i64,
}

/// Keys of a `[spell N]` section of `data/spell_visuals.txt`.
pub const SPELL_VISUAL_KEYS: &[&str] =
    &["visual", "traveling_kit", "impact_kit", "casting_kit", "go_kit", "aura_kit", "go_anim", "cast_anim"];

/// Visual of one of our spells: a legacy spell visual to start from (`visual=<spell entry>`)
/// with per-kit overrides by kit id (0 = none) and unit animations.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CustomVisual {
    pub base: Option<i64>,
    pub traveling: Option<i64>,
    pub impact: Option<i64>,
    pub casting: Option<i64>,
    pub go: Option<i64>,
    pub aura: Option<i64>,
    pub go_anim: Option<i64>,
    pub cast_anim: Option<i64>,
}

impl CustomVisual {
    /// Resolves kit ids against the legacy tables. Unknown kit ids (and 0) mean no kit.
    pub fn resolve(&self, visuals: &HashMap<i64, SpellVisual>, kits: &HashMap<i64, VisualKit>) -> SpellVisual {
        let mut v = self.base.and_then(|b| visuals.get(&b)).cloned().unwrap_or_default();
        let kit = |id: Option<i64>, slot: &mut Option<VisualKit>| {
            if let Some(id) = id {
                *slot = kits.get(&id).cloned();
            }
        };
        kit(self.traveling, &mut v.traveling);
        kit(self.impact, &mut v.impact);
        kit(self.casting, &mut v.casting);
        kit(self.go, &mut v.go);
        kit(self.aura, &mut v.aura_ontop);
        if let Some(a) = self.go_anim {
            v.unit_go_animation = a;
        }
        if let Some(a) = self.cast_anim {
            v.unit_cast_animation = a;
        }
        v
    }
}

/// Unit animation by name (`swing`, `cast`, `shoot`, `cast_alt`, `block`, `hit`) or number
/// (enum: 2 Shoot, 6 Cast, 7 Swing, 8 Hit, 9 Block, 10 CastAlt).
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

/// The `[spell N]` sections (other kinds are left to later readers).
pub fn parse_spell_visuals(sections: &[Section]) -> anyhow::Result<HashMap<i64, CustomVisual>> {
    let mut out = HashMap::new();
    for s in sections.iter().filter(|s| s.kind.as_deref() == Some("spell")) {
        let Some(entry) = s.id_int() else { anyhow::bail!("line {}: bad spell id {:?}", s.line, s.id) };
        let id = |k: &str| s.get(k).map(|v| v.parse::<i64>().unwrap_or(0));
        out.insert(
            entry,
            CustomVisual {
                base: id("visual"),
                traveling: id("traveling_kit"),
                impact: id("impact_kit"),
                casting: id("casting_kit"),
                go: id("go_kit"),
                aura: id("aura_kit"),
                go_anim: s.get("go_anim").map(unit_anim_id),
                cast_anim: s.get("cast_anim").map(unit_anim_id),
            },
        );
    }
    Ok(out)
}

/// Spell entry -> visual, for every spell in `spells`.
pub fn load(root: &Path, spells: &HashMap<i64, SpellTemplate>) -> anyhow::Result<HashMap<i64, SpellVisual>> {
    let db = super::legacy_db()?;
    crate::legacy_note("game.db", "spell_visual, spell_visual_kit");
    let mut visuals = db.spell_visuals()?;
    let kits = db.spell_visual_kits()?;
    let path = root.join("data/spell_visuals.txt");
    if path.exists() {
        for (entry, v) in parse_spell_visuals(&sections::load(&path)?)? {
            visuals.insert(entry, v.resolve(&visuals, &kits));
        }
    }
    visuals.retain(|entry, _| spells.contains_key(entry));
    Ok(visuals)
}

/// `.sa` flipbook script by name: ours (`scripts/override/animation/`) first, else legacy.
pub fn flipbook_path(root: &Path, name: &str) -> Option<PathBuf> {
    let own = root.join("scripts/override/animation").join(name);
    if own.exists() {
        return Some(own);
    }
    crate::find_file(root, &format!("scripts/animation/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spell_sections_resolve_against_kits() {
        let s =
            sections::parse("[spell 50001]\nvisual=229\nimpact_kit=17\ngo_anim=swing\n[kit x]\nanim=a.sa\n").unwrap();
        let v = &parse_spell_visuals(&s).unwrap()[&50001];
        assert_eq!((v.base, v.impact, v.go_anim, v.traveling), (Some(229), Some(17), Some(7), None));
        let kit = |id| VisualKit { id, ..Default::default() };
        let visuals =
            HashMap::from([(229, SpellVisual { impact: Some(kit(179)), unit_go_animation: 6, ..Default::default() })]);
        let kits = HashMap::from([(17, kit(17)), (179, kit(179))]);
        let r = v.resolve(&visuals, &kits);
        assert_eq!((r.impact.map(|k| k.id), r.unit_go_animation, r.traveling.is_none()), (Some(17), 7, true));
    }

    #[test]
    fn shipped_spell_visuals_are_valid() {
        let root = crate::content_root();
        let s = sections::load(&root.join("data/spell_visuals.txt")).unwrap();
        let spell: Vec<Section> = s.iter().filter(|s| s.kind.as_deref() == Some("spell")).cloned().collect();
        assert!(sections::unknown_keys(&spell, SPELL_VISUAL_KEYS).is_empty());
        let visuals = parse_spell_visuals(&s).unwrap();
        let spells = crate::custom::load_spells(&root);
        assert_eq!(visuals.len(), spells.len());
        assert!(spells.iter().all(|t| visuals.contains_key(&t.entry)));
    }
}
