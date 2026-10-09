//! Spell templates by entry (player skills, auto attacks, item spells, NPC spells) from
//! `data/spells.txt`.
//!
//! One `[entry]` section per spell. Keys mirror [`SpellTemplate`]: `name`, `icon` (image file
//! name), `description` and `aura_description` (tooltip `$` tokens: `$E1min $E1max $E1D3 $DUR
//! $INVL`), `mana` (formula), `mana_pct`, `cast_time`, `cooldown`, `duration`, `interval` (ms),
//! `duration_formula`, `range` (64 per cell), `speed` (projectile, 0 = instant), `school` (1
//! physical, 2 frost, 3 fire, 4 shadow, 5 holy; default 1), `attributes`, `abilities_tab` (1 =
//! Spells, 0 = Actions; default 1).
//! Effects N = 1..=3: `effectN=kind` (see [`effect_kind`]), `effectN_data=d1,d2,d3`,
//! `effectN_target=` (see [`target_type`]), `effectN_radius` (cells), `effectN_positive`,
//! `effectN_formula`. Visuals live in `data/spell_visuals.txt` ([`super::visuals`]).

use super::sections::{self, Section};
use crate::spell::{SpellEffect, SpellTemplate};
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;

/// Our player skills start here (`50001..`); auto attacks `50100`/`50101`, item spells
/// `50110..`, NPC spells `51001..`.
pub const FIRST_SPELL: i64 = 50000;
/// Melee auto attack (the server drives it from `Attack`, never as a cast).
pub const ATTACK: i64 = 50100;
/// Ranged auto attack.
pub const SHOOT: i64 = 50101;

const BASE_KEYS: &[&str] = &[
    "name",
    "icon",
    "description",
    "aura_description",
    "mana",
    "mana_pct",
    "cast_time",
    "cooldown",
    "duration",
    "duration_formula",
    "interval",
    "range",
    "speed",
    "school",
    "attributes",
    "abilities_tab",
];
const EFFECT_FIELDS: &[&str] = &["", "_data", "_target", "_radius", "_positive", "_formula"];

/// Every key a `[entry]` section of `data/spells.txt` may use.
pub fn keys() -> Vec<String> {
    let mut k: Vec<String> = BASE_KEYS.iter().map(|s| s.to_string()).collect();
    for n in 1..=3 {
        k.extend(EFFECT_FIELDS.iter().map(|f| format!("effect{n}{f}")));
    }
    k
}

/// `effectN=`: a number or `school_damage`, `apply_aura`, `heal`, `weapon_damage`, `heal_pct`,
/// `restore_mana_pct`, `melee_atk`, `ranged_atk`, `charge`.
pub fn effect_kind(v: &str) -> anyhow::Result<i64> {
    use crate::spell::effect::*;
    Ok(match v {
        "school_damage" => SCHOOL_DAMAGE,
        "apply_aura" => APPLY_AURA,
        "heal" => HEAL,
        "weapon_damage" => WEAPON_DAMAGE,
        "heal_pct" => HEAL_PCT,
        "restore_mana_pct" => RESTORE_MANA_PCT,
        "melee_atk" => MELEE_ATK,
        "ranged_atk" => RANGED_ATK,
        "charge" => CHARGE,
        _ => v.parse().with_context(|| format!("unknown effect kind {v:?}"))?,
    })
}

/// `effectN_target=`: a number or `caster`, `friendly`, `area_src_friendly`, `hostile`,
/// `area_src_hostile`, `area_dst_hostile`, `any`.
pub fn target_type(v: &str) -> anyhow::Result<i64> {
    use crate::spell::target::*;
    Ok(match v {
        "caster" => CASTER,
        "friendly" => FRIENDLY,
        "area_src_friendly" => AREA_SRC_FRIENDLY,
        "hostile" => HOSTILE,
        "area_src_hostile" => AREA_SRC_HOSTILE,
        "area_dst_hostile" => AREA_DST_HOSTILE,
        "any" => ANY,
        _ => v.parse().with_context(|| format!("unknown target type {v:?}"))?,
    })
}

fn int(s: &Section, k: &str, default: i64) -> anyhow::Result<i64> {
    match s.get(k) {
        Some(v) => v.parse().with_context(|| format!("[{}] {k}={v:?}: not a number", s.id)),
        None => Ok(default),
    }
}

/// One spell from its section.
pub fn parse_one(s: &Section) -> anyhow::Result<SpellTemplate> {
    let Some(entry) = s.id_int() else { bail!("line {}: bad spell id {:?}", s.line, s.id) };
    let text = |k: &str| s.get(k).unwrap_or_default().to_string();
    let mut t = SpellTemplate {
        entry,
        name: text("name"),
        icon: text("icon"),
        description: text("description"),
        aura_description: text("aura_description"),
        mana_formula: text("mana"),
        mana_pct: int(s, "mana_pct", 0)?,
        cast_time_ms: int(s, "cast_time", 0)?,
        cooldown_ms: int(s, "cooldown", 0)?,
        duration_ms: int(s, "duration", 0)?,
        duration_formula: text("duration_formula"),
        interval_ms: int(s, "interval", 0)?,
        range: int(s, "range", 0)?,
        speed: int(s, "speed", 0)?,
        school: int(s, "school", 1)?,
        attributes: int(s, "attributes", 0)?,
        abilities_tab: int(s, "abilities_tab", 1)?,
        ..Default::default()
    };
    for n in 1..=3 {
        let key = format!("effect{n}");
        let Some(kind) = s.get(&key) else { continue };
        let mut e = SpellEffect { kind: effect_kind(kind)?, ..Default::default() };
        if let Some(d) = s.get(&format!("{key}_data")) {
            for (slot, x) in e.data.iter_mut().zip(d.split(',')) {
                *slot = x.trim().parse().with_context(|| format!("[{entry}] {key}_data={d:?}"))?;
            }
        }
        if let Some(v) = s.get(&format!("{key}_target")) {
            e.target = target_type(v)?;
        }
        e.radius = int(s, &format!("{key}_radius"), 0)?;
        e.positive = int(s, &format!("{key}_positive"), 0)? != 0;
        e.formula = text(&format!("{key}_formula"));
        t.effects.push(e);
    }
    Ok(t)
}

/// Every plain `[entry]` section.
pub fn parse(sections: &[Section]) -> anyhow::Result<Vec<SpellTemplate>> {
    sections.iter().filter(|s| s.kind.is_none()).map(parse_one).collect()
}

pub fn load(root: &Path) -> anyhow::Result<HashMap<i64, SpellTemplate>> {
    let spells = parse(&sections::load(&root.join("data/spells.txt"))?)?;
    Ok(spells.into_iter().map(|s| (s.entry, s)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spell::{FormulaVars, effect, eval_formula, target};

    #[test]
    fn parses_a_spell() {
        let s = sections::parse(
            "# c\n[50001]\nname=Open Vein\nicon=skill_open_vein.png\ndescription=Bleeds $E2max over $DUR.\n\
             mana=2+clvl\ncooldown=8000\nrange=130\nduration=9000\ninterval=3000\n\
             effect1=weapon_damage\neffect1_data=1,60,0\neffect1_target=hostile\n\
             effect2=apply_aura\neffect2_data=1,1,0\neffect2_target=14\neffect2_formula=4+clvl*3\n\
             [50002]\nname=Veil\nabilities_tab=0\neffect1=apply_aura\neffect1_target=caster\neffect1_positive=1\n",
        )
        .unwrap();
        let v = parse(&s).unwrap();
        let t = &v[0];
        assert_eq!((t.entry, t.name.as_str(), t.icon.as_str()), (50001, "Open Vein", "skill_open_vein.png"));
        assert_eq!(
            (t.mana_formula.as_str(), t.cooldown_ms, t.range, t.duration_ms, t.interval_ms),
            ("2+clvl", 8000, 130, 9000, 3000)
        );
        assert_eq!((t.abilities_tab, t.school), (1, 1));
        assert_eq!((t.effects[0].kind, t.effects[0].data, t.effects[0].target), (14, [1, 60, 0], 14));
        assert_eq!((t.effects[1].kind, t.effects[1].formula.as_str()), (3, "4+clvl*3"));
        assert_eq!((v[1].abilities_tab, v[1].effects[0].target, v[1].effects[0].positive), (0, 1, true));
        assert!(parse(&sections::parse("[1]\neffect1=bogus\n").unwrap()).is_err());
        assert!(parse(&sections::parse("[1]\ncooldown=soon\n").unwrap()).is_err());
    }

    #[test]
    fn shipped_spells_are_valid() {
        let root = crate::assets_root();
        let s = sections::load(&root.join("data/spells.txt")).unwrap();
        let keys = keys();
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        assert!(sections::unknown_keys(&s, &keys).is_empty(), "{:?}", sections::unknown_keys(&s, &keys));
        let spells = load(&root).unwrap();
        for t in spells.values() {
            assert!(t.entry > FIRST_SPELL && !t.name.is_empty() && !t.icon.is_empty(), "{t:?}");
            assert!(!t.effects.is_empty(), "{} has no effects", t.name);
            assert!(!t.description.contains("$E4"), "{}", t.name);
            for e in &t.effects {
                assert!(e.target != 0, "{}: effect without target", t.name);
            }
            let vars = FormulaVars { clvl: 3.0, splvl: 1.0, value: 10.0, ..Default::default() };
            let formulas =
                [&t.mana_formula, &t.duration_formula].into_iter().chain(t.effects.iter().map(|e| &e.formula));
            for f in formulas {
                assert!(eval_formula(f, &vars).is_ok(), "{}: bad formula {f}", t.name);
            }
        }
        // The auto attacks the server drives from `Attack` and the client keeps off the bar.
        assert_eq!(spells[&ATTACK].effects[0].kind, effect::MELEE_ATK);
        assert_eq!(spells[&SHOOT].effects[0].kind, effect::RANGED_ATK);
        assert!(spells[&ATTACK].is_passive_or_auto() && spells[&SHOOT].is_passive_or_auto());
        assert_eq!(spells[&ATTACK].effects[0].target, target::HOSTILE);
    }
}
