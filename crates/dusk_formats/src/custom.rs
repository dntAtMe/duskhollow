//! Parsers of our own data files (`custom_assets/data`, map sidecars); see `crate::content`
//! for the loaders that merge them with the legacy data.

use crate::db::{NpcModel, NpcSpawn, NpcSpell, NpcTemplate};
use crate::spell::{SpellEffect, SpellTemplate};
use std::path::Path;

/// Prefix of our maps (`custom_assets/maps`), as opposed to legacy ones.
pub const CUSTOM_MAP_PREFIX: &str = "custom_";

/// Parses a `maps/<name>.spawns` sidecar: `entry x y orientation wander_distance` per line,
/// `#` comments. Spawns get synthetic guids from `first_guid` and wander if the distance is > 0.
pub fn parse_spawns(text: &str, map: i64, first_guid: i64) -> Vec<NpcSpawn> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .enumerate()
        .filter_map(|(i, l)| {
            let v: Vec<f32> = l.split_whitespace().map(|s| s.parse().ok()).collect::<Option<_>>()?;
            (v.len() >= 3).then(|| NpcSpawn {
                guid: first_guid + i as i64,
                entry: v[0] as i64,
                map,
                x: v[1],
                y: v[2],
                orientation: v.get(3).copied().unwrap_or(0.0),
                respawn_time: 60,
                movement_type: if v.get(4).copied().unwrap_or(0.0) > 0.0 { 1 } else { 0 },
                wander_distance: v.get(4).copied().unwrap_or(0.0) as i64,
            })
        })
        .collect()
}

/// First `npc_template.entry` (and model id) used by `custom_assets/data/npc_templates.txt`.
pub const CUSTOM_NPC_FIRST: i64 = 50000;

/// Parses `custom_assets/data/npc_templates.txt`: `[entry]` sections of `key=value` lines, `#`
/// comments. Keys are [`NpcTemplate`] field names plus `model` (sprite script name under
/// `scripts/npc/custom/`), `height` (pixels, nameplate offset), `level=MIN[-MAX]`,
/// `resist=frost,fire,shadow,holy` and `spellN=spell,chance,interval_ms,cooldown_ms,target_type`
/// (N = 1..=4). Each template gets its own model with `id = entry`. Unknown keys are ignored.
pub fn parse_npc_templates(text: &str) -> Vec<(NpcTemplate, NpcModel)> {
    let mut out = Vec::new();
    let mut cur: Option<(NpcTemplate, NpcModel)> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if let Some(entry) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.extend(cur.take());
            let Ok(entry) = entry.trim().parse::<i64>() else { continue };
            cur = Some((default_template(entry), NpcModel { id: entry, name: String::new(), height: 0 }));
            continue;
        }
        let (Some((t, m)), Some((k, v))) = (cur.as_mut(), line.split_once('=')) else { continue };
        let (k, v) = (k.trim(), v.trim());
        let n = || v.parse::<i64>().unwrap_or(0);
        match k {
            "name" => t.name = v.into(),
            "subname" => t.subname = v.into(),
            "model" => m.name = v.into(),
            "height" => m.height = n(),
            "level" => {
                let (lo, hi) = v.split_once('-').unwrap_or((v, v));
                t.min_level = lo.trim().parse().unwrap_or(1);
                t.max_level = hi.trim().parse().unwrap_or(t.min_level);
            }
            "faction" => t.faction = n(),
            "model_scale" => t.model_scale = n(),
            "health" => t.health = n(),
            "mana" => t.mana = n(),
            "weapon_value" => t.weapon_value = n(),
            "armor" => t.armor = n(),
            "melee_speed_ms" => t.melee_speed_ms = n(),
            "leash_range" => t.leash_range = n(),
            "ai_type" => t.ai_type = n(),
            "npc_flags" => t.npc_flags = n(),
            "strength" => t.strength = n(),
            "agility" => t.agility = n(),
            "intellect" => t.intellect = n(),
            "willpower" => t.willpower = n(),
            "courage" => t.courage = n(),
            "resist" => {
                for (r, x) in t.resist.iter_mut().zip(v.split(',')) {
                    *r = x.trim().parse().unwrap_or(0);
                }
            }
            "elite" => t.elite = n() != 0,
            "boss" => t.boss = n() != 0,
            "portrait" => t.portrait = v.into(),
            _ => {
                let Some(i) = k.strip_prefix("spell").and_then(|i| i.parse::<usize>().ok()) else { continue };
                let f: Vec<i64> = v.split(',').map(|x| x.trim().parse().unwrap_or(0)).collect();
                if (1..=4).contains(&i) && f.len() == 5 {
                    t.spells[i - 1] =
                        NpcSpell { spell: f[0], chance: f[1], interval_ms: f[2], cooldown_ms: f[3], target_type: f[4] };
                }
            }
        }
    }
    out.extend(cur);
    out
}

/// Loads `<custom_root>/data/npc_templates.txt` (empty if missing).
pub fn load_npc_templates(custom_root: &Path) -> Vec<(NpcTemplate, NpcModel)> {
    std::fs::read_to_string(custom_root.join("data/npc_templates.txt"))
        .map(|t| parse_npc_templates(&t))
        .unwrap_or_default()
}

fn default_template(entry: i64) -> NpcTemplate {
    NpcTemplate {
        entry,
        name: String::new(),
        subname: String::new(),
        model_id: entry,
        min_level: 1,
        max_level: 1,
        faction: crate::db::faction::HOSTILE,
        model_scale: 100,
        health: -1,
        mana: 0,
        weapon_value: -1,
        armor: 0,
        melee_speed_ms: 2000,
        leash_range: 0,
        ai_type: 0,
        npc_flags: 0,
        strength: 0,
        agility: 0,
        intellect: 0,
        willpower: 0,
        courage: 0,
        resist: [0; 4],
        spells: vec![NpcSpell { spell: 0, chance: 0, interval_ms: 0, cooldown_ms: 0, target_type: 0 }; 4],
        elite: false,
        boss: false,
        portrait: String::new(),
    }
}

/// First `spell_template.entry` used by `custom_assets/data/spells.txt`.
pub const CUSTOM_SPELL_FIRST: i64 = 50000;

fn effect_kind(v: &str) -> i64 {
    use crate::spell::effect::*;
    match v {
        "school_damage" => SCHOOL_DAMAGE,
        "apply_aura" => APPLY_AURA,
        "heal" => HEAL,
        "weapon_damage" => WEAPON_DAMAGE,
        "heal_pct" => HEAL_PCT,
        "charge" => CHARGE,
        _ => v.parse().unwrap_or(0),
    }
}

fn target_type(v: &str) -> i64 {
    use crate::spell::target::*;
    match v {
        "caster" => CASTER,
        "friendly" => FRIENDLY,
        "area_src_friendly" => AREA_SRC_FRIENDLY,
        "hostile" => HOSTILE,
        "area_src_hostile" => AREA_SRC_HOSTILE,
        "area_dst_hostile" => AREA_DST_HOSTILE,
        "any" => ANY,
        _ => v.parse().unwrap_or(0),
    }
}

/// Parses `custom_assets/data/spells.txt`: `[entry]` sections of `key=value`, `#` comments.
///
/// Keys mirror `spell_template` columns: `name`, `icon` (image file name), `description` and
/// `aura_description` (tooltip `$` tokens as in legacy spells), `mana` (formula), `mana_pct`,
/// `cast_time`, `cooldown`, `duration`, `interval` (ms), `duration_formula`, `range` (64 per
/// cell), `speed` (projectile, 0 = instant), `school` (1 physical, 2 frost, 3 fire, 4 shadow,
/// 5 holy; default 1), `attributes`, `abilities_tab` (default 1 = Spells).
/// Effects N = 1..=3: `effectN=kind` (number or `school_damage`, `weapon_damage`, `apply_aura`,
/// `heal`, `heal_pct`, `charge`), `effectN_data=d1,d2,d3`, `effectN_target=` (number or `caster`,
/// `hostile`, `friendly`, `area_src_hostile`, ...), `effectN_radius` (cells), `effectN_positive`,
/// `effectN_formula`. Visuals are in `data/spell_visuals.txt` (`crate::content::visuals`).
pub fn parse_spells(text: &str) -> Vec<SpellTemplate> {
    let mut out = Vec::new();
    let mut cur: Option<SpellTemplate> = None;
    for line in text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')) {
        if let Some(entry) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            out.extend(cur.take());
            let Ok(entry) = entry.trim().parse::<i64>() else { continue };
            cur = Some(SpellTemplate { entry, abilities_tab: 1, school: 1, ..Default::default() });
            continue;
        }
        let (Some(t), Some((k, v))) = (cur.as_mut(), line.split_once('=')) else { continue };
        let (k, v) = (k.trim(), v.trim());
        let n = || v.parse::<i64>().unwrap_or(0);
        match k {
            "name" => t.name = v.into(),
            "icon" => t.icon = v.into(),
            "description" => t.description = v.into(),
            "aura_description" => t.aura_description = v.into(),
            "mana" => t.mana_formula = v.into(),
            "mana_pct" => t.mana_pct = n(),
            "cast_time" => t.cast_time_ms = n(),
            "cooldown" => t.cooldown_ms = n(),
            "duration" => t.duration_ms = n(),
            "duration_formula" => t.duration_formula = v.into(),
            "interval" => t.interval_ms = n(),
            "range" => t.range = n(),
            "speed" => t.speed = n(),
            "school" => t.school = n(),
            "attributes" => t.attributes = n(),
            "abilities_tab" => t.abilities_tab = n(),
            _ => {
                let Some(rest) = k.strip_prefix("effect") else { continue };
                let (i, field) = rest.split_once('_').unwrap_or((rest, ""));
                let Some(i) = i.parse::<usize>().ok().filter(|i| (1..=3).contains(i)) else { continue };
                if t.effects.len() < i {
                    t.effects.resize(i, SpellEffect::default());
                }
                let e = &mut t.effects[i - 1];
                match field {
                    "" => e.kind = effect_kind(v),
                    "data" => {
                        for (d, x) in e.data.iter_mut().zip(v.split(',')) {
                            *d = x.trim().parse().unwrap_or(0);
                        }
                    }
                    "target" => e.target = target_type(v),
                    "radius" => e.radius = n(),
                    "positive" => e.positive = n() != 0,
                    "formula" => e.formula = v.into(),
                    _ => {}
                }
            }
        }
    }
    out.extend(cur);
    // Unset effect slots (e.g. only effect2 given) would read as kind 0: drop them.
    for s in &mut out {
        s.effects.retain(|e| e.kind != 0);
    }
    out
}

/// Loads `<custom_root>/data/spells.txt` (empty if missing).
pub fn load_spells(custom_root: &Path) -> Vec<SpellTemplate> {
    std::fs::read_to_string(custom_root.join("data/spells.txt")).map(|t| parse_spells(&t)).unwrap_or_default()
}

/// Parses `custom_assets/data/class_spells.txt`: `class spell` per line, `#` comments.
pub fn parse_class_spells(text: &str) -> Vec<(i64, i64)> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace().map(|s| s.parse::<i64>().ok());
            Some((it.next()??, it.next()??))
        })
        .collect()
}

/// Loads `<custom_root>/data/class_spells.txt` (empty if missing).
pub fn load_class_spells(custom_root: &Path) -> Vec<(i64, i64)> {
    std::fs::read_to_string(custom_root.join("data/class_spells.txt"))
        .map(|t| parse_class_spells(&t))
        .unwrap_or_default()
}

/// A named point of a custom map (`maps/<name>.markers`: `name x y [radius]` per line, cells).
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    pub name: String,
    pub x: f32,
    pub y: f32,
    pub radius: f32,
}

pub fn parse_markers(text: &str) -> Vec<Marker> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let name = it.next()?.to_string();
            let v: Vec<f32> = it.map(|s| s.parse().ok()).collect::<Option<_>>()?;
            (v.len() >= 2).then(|| Marker { name, x: v[0], y: v[1], radius: v.get(2).copied().unwrap_or(3.0) })
        })
        .collect()
}

/// Cover of one cell (`maps/<name>.cover`, docs/demo-plan.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cover {
    #[default]
    Open,
    Shade,
    Shelter,
    Cairn,
}

/// Per-cell cover grid: first line `W H`, then H rows of W chars (`.` open, `s` shade,
/// `S` deep shelter, `C` rest cairn). Cells outside the grid are open sky.
#[derive(Debug, Clone, Default)]
pub struct CoverGrid {
    pub width: usize,
    pub height: usize,
    pub cells: Vec<Cover>,
}

impl CoverGrid {
    pub fn parse(text: &str) -> Option<Self> {
        let mut lines = text.lines().map(str::trim_end).filter(|l| !l.starts_with('#'));
        let mut dims = lines.next()?.split_whitespace().map(|s| s.parse::<usize>());
        let (width, height) = (dims.next()?.ok()?, dims.next()?.ok()?);
        let mut cells = vec![Cover::Open; width * height];
        for (y, row) in lines.take(height).enumerate() {
            for (x, c) in row.chars().take(width).enumerate() {
                cells[y * width + x] = match c {
                    's' => Cover::Shade,
                    'S' => Cover::Shelter,
                    'C' => Cover::Cairn,
                    _ => Cover::Open,
                };
            }
        }
        Some(Self { width, height, cells })
    }

    pub fn get(&self, x: i32, y: i32) -> Cover {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return Cover::Open;
        }
        self.cells[y as usize * self.width + x as usize]
    }

    /// Cell centres of every `C`.
    pub fn cairns(&self) -> Vec<(f32, f32)> {
        (0..self.cells.len())
            .filter(|&i| self.cells[i] == Cover::Cairn)
            .map(|i| ((i % self.width) as f32 + 0.5, (i / self.width) as f32 + 0.5))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spawns_sidecar() {
        let s = parse_spawns("# comment\n2 10.5 12 1.5 4\n\n13 3 4\nbad line\n", 10000, 1);
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].entry, s[0].x, s[0].wander_distance, s[0].movement_type), (2, 10.5, 4, 1));
        assert_eq!((s[1].entry, s[1].movement_type, s[1].guid), (13, 0, 2));
    }

    #[test]
    fn npc_templates_file() {
        let t = parse_npc_templates(
            "# c\n[50001]\nname=Glarewolf\nmodel=glarewolf\nheight=40\nlevel=2-3\nboss=1\nspell2=12,50,4000,8000,14\n\n[50010]\nname=Ysolde\nfaction=1\n",
        );
        assert_eq!(t.len(), 2);
        let (w, m) = &t[0];
        assert_eq!((w.entry, w.model_id, w.min_level, w.max_level, w.boss), (50001, 50001, 2, 3, true));
        assert_eq!((m.name.as_str(), m.height, w.spells[1].spell, w.spells[1].target_type), ("glarewolf", 40, 12, 14));
        assert_eq!(t[1].0.faction, 1);
    }

    #[test]
    fn spells_file() {
        let s = parse_spells(
            "# c\n[50001]\nname=Open Vein\nicon=skill_open_vein.png\ndescription=Bleeds $E2max over $DUR.\n\
             mana=2+clvl\ncooldown=8000\nrange=130\nduration=9000\ninterval=3000\n\
             effect1=weapon_damage\neffect1_data=1,60,0\neffect1_target=hostile\n\
             effect2=apply_aura\neffect2_data=1,1,0\neffect2_target=14\neffect2_formula=4+clvl*3\n\
             [50002]\nname=Veil\nabilities_tab=0\neffect2=apply_aura\neffect2_target=caster\neffect2_positive=1\n",
        );
        assert_eq!(s.len(), 2);
        let t = &s[0];
        assert_eq!((t.entry, t.name.as_str(), t.icon.as_str()), (50001, "Open Vein", "skill_open_vein.png"));
        assert_eq!(
            (t.mana_formula.as_str(), t.cooldown_ms, t.range, t.duration_ms, t.interval_ms),
            ("2+clvl", 8000, 130, 9000, 3000)
        );
        assert_eq!((t.abilities_tab, t.school), (1, 1));
        assert_eq!(t.effects.len(), 2);
        assert_eq!((t.effects[0].kind, t.effects[0].data, t.effects[0].target), (14, [1, 60, 0], 14));
        assert_eq!((t.effects[1].kind, t.effects[1].formula.as_str()), (3, "4+clvl*3"));
        // A lone effect2 becomes the only effect.
        let veil = &s[1];
        assert_eq!(
            (veil.abilities_tab, veil.effects.len(), veil.effects[0].target, veil.effects[0].positive),
            (0, 1, 1, true)
        );
    }

    #[test]
    fn class_spells_file() {
        assert_eq!(parse_class_spells("# x\n1 50001\n\n4 50010 trailing\nbad\n"), vec![(1, 50001), (4, 50010)]);
    }

    #[test]
    fn shipped_spells_are_valid() {
        let root = crate::content_root();
        let spells = load_spells(&root);
        for t in &spells {
            assert!(t.entry >= CUSTOM_SPELL_FIRST && !t.name.is_empty() && !t.icon.is_empty(), "{t:?}");
            assert!(!t.effects.is_empty(), "{} has no effects", t.name);
            assert!(!t.description.contains("$E4"), "{}", t.name);
            for e in &t.effects {
                assert!(e.target != 0, "{}: effect without target", t.name);
            }
            // Every formula evaluates.
            let vars = crate::spell::FormulaVars { clvl: 3.0, splvl: 1.0, value: 10.0, ..Default::default() };
            let formulas =
                [&t.mana_formula, &t.duration_formula].into_iter().chain(t.effects.iter().map(|e| &e.formula));
            for f in formulas {
                assert!(crate::spell::eval_formula(f, &vars).is_ok(), "{}: bad formula {f}", t.name);
            }
        }
        for (class, spell) in load_class_spells(&root) {
            assert!((1..=4).contains(&class));
            assert!(spell < CUSTOM_SPELL_FIRST || spells.iter().any(|s| s.entry == spell), "{spell}");
        }
    }

    #[test]
    fn cover_and_markers() {
        let g = CoverGrid::parse("3 2\n.sS\nC..\n").unwrap();
        assert_eq!(
            (g.get(1, 0), g.get(2, 0), g.get(0, 1), g.get(5, 5)),
            (Cover::Shade, Cover::Shelter, Cover::Cairn, Cover::Open)
        );
        assert_eq!(g.cairns(), vec![(0.5, 1.5)]);
        let m = parse_markers("# x\nglare_gate 30 8 6\nspawn 31.5 60\n");
        assert_eq!(m[0], Marker { name: "glare_gate".into(), x: 30.0, y: 8.0, radius: 6.0 });
        assert_eq!(m[1].radius, 3.0);
    }
}
