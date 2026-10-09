//! `spell_template` rows and the designer formula language.
//!
//! Formulas (`mana_formula`, `effectN_scale_formula`, `duration_formula`) are
//! arithmetic over `+ - * /` and parentheses with the variables `clvl` (caster level),
//! `splvl` (spell level), `value` (the effect's base value) and the caster's
//! attributes `STR AGI WIL INT CUR` (from the original client's evaluator, which
//! substitutes those tokens before evaluating). See `scripts/text/STF_*.txt`.

use crate::db::GameDb;
use rusqlite::{Row, types::ValueRef};
use std::collections::HashMap;

pub mod effect {
    pub const SCHOOL_DAMAGE: i64 = 1;
    pub const APPLY_AURA: i64 = 3;
    pub const HEAL: i64 = 6;
    pub const WEAPON_DAMAGE: i64 = 14;
    pub const THREAT: i64 = 18;
    pub const HEAL_PCT: i64 = 27;
    pub const RESTORE_MANA_PCT: i64 = 28;
    pub const MELEE_ATK: i64 = 30;
    pub const RANGED_ATK: i64 = 31;
    /// Rush to the target (Charge). Implemented as a dash that stops just short of it.
    pub const CHARGE: i64 = 37;
}

pub mod aura {
    pub const PERIODIC_DAMAGE: i64 = 1;
    pub const PERIODIC_HEAL: i64 = 2;
    pub const INFLICT_MECHANIC: i64 = 3;
    pub const MODIFY_STAT: i64 = 4;
    pub const MODIFY_STAT_PCT: i64 = 5;
    pub const MODIFY_MOVE_SPEED_PCT: i64 = 11;
    pub const MODIFY_DMG_DEALT_PCT: i64 = 14;
    pub const MODIFY_DMG_RECEIVED_PCT: i64 = 15;
    /// Ours (not a legacy aura type): percent change to gaze strain gain (data3, e.g. -50).
    pub const MODIFY_STRAIN_GAIN_PCT: i64 = 100;
}

pub mod mechanic {
    pub const FEAR: i64 = 3;
    pub const ROOT: i64 = 4;
    pub const SILENCE: i64 = 5;
    pub const SLEEP: i64 = 6;
    pub const SNARE: i64 = 7;
    pub const STUN: i64 = 8;
    pub const INCAPACITATED: i64 = 9;
}

pub mod target {
    pub const CASTER: i64 = 1;
    pub const FRIENDLY: i64 = 2;
    pub const AREA_SRC_FRIENDLY: i64 = 3;
    pub const HOSTILE: i64 = 14;
    pub const AREA_SRC_HOSTILE: i64 = 15;
    pub const AREA_DST_HOSTILE: i64 = 16;
    pub const ANY: i64 = 17;
}

#[derive(Debug, Clone, Default)]
pub struct SpellEffect {
    pub kind: i64,
    pub data: [i64; 3],
    pub target: i64,
    pub radius: i64,
    pub positive: bool,
    pub formula: String,
}

#[derive(Debug, Clone, Default)]
pub struct SpellTemplate {
    pub entry: i64,
    pub name: String,
    pub icon: String,
    pub description: String,
    pub aura_description: String,
    pub mana_formula: String,
    pub mana_pct: i64,
    pub effects: Vec<SpellEffect>,
    pub attributes: i64,
    pub cast_time_ms: i64,
    pub cooldown_ms: i64,
    pub cast_interrupt_flags: i64,
    pub school: i64,
    pub duration_ms: i64,
    pub duration_formula: String,
    /// Projectile speed (0 = instant).
    pub speed: i64,
    /// Original units; ~64 per cell.
    pub range: i64,
    pub interval_ms: i64,
    pub required_equipment: i64,
    pub abilities_tab: i64,
}

/// Original range units per map cell (melee 130 ~ 2 cells, spells 610 ~ 9.5 cells).
pub const RANGE_UNITS_PER_CELL: f32 = 64.0;

impl SpellTemplate {
    pub fn range_cells(&self) -> f32 {
        self.range as f32 / RANGE_UNITS_PER_CELL
    }

    pub fn is_passive_or_auto(&self) -> bool {
        self.effects.iter().any(|e| e.kind == effect::MELEE_ATK || e.kind == effect::RANGED_ATK)
    }
}

fn int(row: &Row, col: &str) -> i64 {
    match row.get_ref(col) {
        Ok(ValueRef::Integer(i)) => i,
        Ok(ValueRef::Real(f)) => f as i64,
        Ok(ValueRef::Text(t)) => std::str::from_utf8(t).ok().and_then(|s| s.trim().parse().ok()).unwrap_or(0),
        _ => 0,
    }
}

fn text(row: &Row, col: &str) -> String {
    match row.get_ref(col) {
        Ok(ValueRef::Text(t)) => String::from_utf8_lossy(t).trim().to_string(),
        Ok(ValueRef::Integer(i)) => i.to_string(),
        _ => String::new(),
    }
}

impl GameDb {
    pub fn spells(&self) -> rusqlite::Result<HashMap<i64, SpellTemplate>> {
        let mut stmt = self.conn().prepare("SELECT * FROM spell_template")?;
        let rows = stmt.query_map([], |r| {
            let effects = (1..=3)
                .filter_map(|i| {
                    let kind = int(r, &format!("effect{i}"));
                    (kind != 0).then(|| SpellEffect {
                        kind,
                        data: [1, 2, 3].map(|d| int(r, &format!("effect{i}_data{d}"))),
                        target: int(r, &format!("effect{i}_targetType")),
                        radius: int(r, &format!("effect{i}_radius")),
                        positive: int(r, &format!("effect{i}_positive")) != 0,
                        formula: text(r, &format!("effect{i}_scale_formula")),
                    })
                })
                .collect();
            Ok(SpellTemplate {
                entry: int(r, "entry"),
                name: text(r, "name"),
                icon: text(r, "icon"),
                description: text(r, "description"),
                aura_description: text(r, "aura_description"),
                mana_formula: text(r, "mana_formula"),
                mana_pct: int(r, "mana_pct"),
                effects,
                attributes: int(r, "attributes"),
                cast_time_ms: int(r, "cast_time"),
                cooldown_ms: int(r, "cooldown"),
                cast_interrupt_flags: int(r, "cast_interrupt_flags"),
                school: int(r, "cast_school"),
                duration_ms: int(r, "duration"),
                duration_formula: text(r, "duration_formula"),
                speed: int(r, "speed"),
                range: int(r, "range"),
                interval_ms: int(r, "interval"),
                required_equipment: int(r, "required_equipment"),
                abilities_tab: int(r, "abilities_tab"),
            })
        })?;
        rows.map(|r| r.map(|s| (s.entry, s))).collect()
    }

    /// `player_create_spell`: class -> starting spells.
    pub fn class_spells(&self) -> rusqlite::Result<HashMap<i64, Vec<i64>>> {
        let mut stmt = self.conn().prepare("SELECT class, spell FROM player_create_spell ORDER BY class, spell")?;
        let mut out: HashMap<i64, Vec<i64>> = HashMap::new();
        let rows = stmt.query_map([], |r| Ok((int(r, "class"), int(r, "spell"))))?;
        for row in rows {
            let (c, s) = row?;
            out.entry(c).or_default().push(s);
        }
        Ok(out)
    }
}

pub use crate::content::visuals::{KitAnim, SpellVisual, VisualKit};

/// Inputs for formula evaluation.
#[derive(Debug, Clone, Copy, Default)]
pub struct FormulaVars {
    pub clvl: f64,
    pub splvl: f64,
    pub value: f64,
    pub str: f64,
    pub agi: f64,
    pub wil: f64,
    pub int: f64,
    pub cur: f64,
}

/// Evaluates a designer formula. Empty formulas evaluate to `value`.
/// Division by zero yields 0 rather than an error.
pub fn eval_formula(expr: &str, v: &FormulaVars) -> Result<f64, String> {
    let expr: Vec<char> = expr.chars().filter(|c| !c.is_whitespace()).collect();
    if expr.is_empty() {
        return Ok(v.value);
    }
    let mut p = Parser { s: &expr, i: 0, v };
    let r = p.expr()?;
    if p.i != expr.len() {
        return Err(format!("unexpected '{}' at {}", expr[p.i], p.i));
    }
    Ok(r)
}

struct Parser<'a> {
    s: &'a [char],
    i: usize,
    v: &'a FormulaVars,
}

impl Parser<'_> {
    fn peek(&self) -> Option<char> {
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Result<f64, String> {
        let mut acc = self.term()?;
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.i += 1;
            let rhs = self.term()?;
            acc = if op == '+' { acc + rhs } else { acc - rhs };
        }
        Ok(acc)
    }

    fn term(&mut self) -> Result<f64, String> {
        let mut acc = self.factor()?;
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.i += 1;
            let rhs = self.factor()?;
            acc = if op == '*' {
                acc * rhs
            } else if rhs == 0.0 {
                0.0
            } else {
                acc / rhs
            };
        }
        Ok(acc)
    }

    fn factor(&mut self) -> Result<f64, String> {
        match self.peek() {
            Some('-') => {
                self.i += 1;
                Ok(-self.factor()?)
            }
            Some('(') => {
                self.i += 1;
                let r = self.expr()?;
                match self.peek() {
                    Some(')') => self.i += 1,
                    // Unclosed at end of input: tolerated (Greater Heal's mana formula ships like that).
                    None => {}
                    Some(c) => return Err(format!("expected ')' but found '{c}' at {}", self.i)),
                }
                Ok(r)
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_digit() || c == '.') {
                    self.i += 1;
                }
                let s: String = self.s[start..self.i].iter().collect();
                s.parse().map_err(|_| format!("bad number {s}"))
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let start = self.i;
                while self.peek().is_some_and(|c| c.is_ascii_alphanumeric() || c == '_') {
                    self.i += 1;
                }
                let name: String = self.s[start..self.i].iter().collect();
                let v = self.v;
                Ok(match name.as_str() {
                    "clvl" => v.clvl,
                    "splvl" => v.splvl,
                    "value" => v.value,
                    "STR" => v.str,
                    "AGI" => v.agi,
                    "WIL" => v.wil,
                    "INT" => v.int,
                    "CUR" => v.cur,
                    other => return Err(format!("unknown variable {other}")),
                })
            }
            other => Err(format!("unexpected {other:?} at {}", self.i)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars() -> FormulaVars {
        FormulaVars { clvl: 1.0, splvl: 1.0, value: 9.0, str: 15.0, agi: 10.0, wil: 5.0, int: 5.0, cur: 15.0 }
    }

    #[test]
    fn evaluates_real_formulas() {
        let v = vars();
        assert_eq!(eval_formula("value+splvl", &v).unwrap(), 10.0); // Mark Target: 10%
        assert_eq!(eval_formula("2+((clvl*20)/20)", &v).unwrap(), 3.0);
        // Holy Wrath: splvl+(((CUR+INT)*115)/(105-(splvl*5))) = 1 + 2300/100
        assert_eq!(eval_formula("splvl+(((CUR+INT)*115)/(105-(splvl*5)))", &v).unwrap(), 24.0);
        assert_eq!(eval_formula("", &v).unwrap(), 9.0);
        assert_eq!(eval_formula("10/0", &v).unwrap(), 0.0);
        assert_eq!(eval_formula("-3+clvl", &v).unwrap(), -2.0);
        assert_eq!(eval_formula("2+((clvl*75)/10", &v).unwrap(), 9.5);
        assert!(eval_formula("2+", &v).is_err());
        assert!(eval_formula("foo", &v).is_err());
    }
}
