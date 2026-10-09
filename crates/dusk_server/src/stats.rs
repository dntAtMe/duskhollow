//! Unit stats and the melee formula.
//!
//! Rules follow the in-game stat descriptions (tooltips) wherever they say something concrete;
//! everything marked DESIGN is a balance choice.

use bevy::prelude::*;
use dusk_formats::content::types::{ClassStats, NpcTemplate};
use dusk_formats::item::{ItemStats, stat};
use dusk_protocol::{Attributes, HitResult};

#[derive(Component, Debug, Clone)]
pub struct Stats {
    pub level: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub mana: i32,
    pub max_mana: i32,
    /// "The amount of damage that you deal with a basic melee attack."
    pub weapon_value: i32,
    pub armor: i32,
    pub melee_speed_ms: u32,
    pub melee_crit: i32,
    pub dodge: i32,
    pub parry: i32,
    pub block: i32,
    pub spell_crit: i32,
    /// Frost, fire, shadow, holy (`ResistX`: chance = value / attacker level % to halve damage).
    pub resist: [i32; 4],
    pub attrs: Attributes,
}

/// DESIGN: the db leaves most NPC health/weapon values at -1 ("derive"); these curves are
/// tuned so a level-1 warrior kills a level-1 NPC in ~4 swings and survives ~10.
pub fn npc_stats(t: &NpcTemplate, level: u32) -> Stats {
    let lvl = level.max(1) as i32;
    let (hp_mul, dmg_mul) = match (t.boss, t.elite) {
        (true, _) => (6.0, 2.0),
        (_, true) => (2.5, 1.5),
        _ => (1.0, 1.0),
    };
    let hp = if t.health > 0 { t.health as i32 } else { ((20 + 30 * lvl) as f32 * hp_mul) as i32 };
    let wv = if t.weapon_value > 0 { t.weapon_value as i32 } else { ((3 + 2 * lvl) as f32 * dmg_mul) as i32 };
    let mana = t.mana.max(0) as i32;
    Stats {
        level: lvl as u32,
        hp,
        max_hp: hp,
        mana,
        max_mana: mana,
        weapon_value: wv,
        armor: t.armor.max(0) as i32,
        melee_speed_ms: if t.melee_speed_ms > 0 { t.melee_speed_ms as u32 } else { 2000 },
        melee_crit: 5 * lvl + t.agility as i32 / 2,
        dodge: 3 * lvl + t.agility as i32 / 2,
        parry: 0,
        block: 0,
        spell_crit: 3 * lvl + t.intellect as i32 / 2,
        resist: t.resist.map(|r| r.max(0) as i32),
        attrs: Attributes {
            strength: t.strength as i32,
            agility: t.agility as i32,
            willpower: t.willpower as i32,
            intelligence: t.intellect as i32,
            courage: t.courage as i32,
        },
    }
}

/// Player stats from `player_class_stats` plus everything equipped (`gear`: the sum of the
/// equipped items' [`ItemStats`], see `items::gear_stats`).
pub fn player_stats(c: &ClassStats, gear: &ItemStats) -> Stats {
    let lvl = c.level.max(1) as i32;
    let bonus = |s: i64| gear.bonuses.iter().filter(|b| b.0 == s).map(|b| b.1).sum::<i32>();
    let attrs = Attributes {
        strength: c.strength as i32 + bonus(stat::STRENGTH),
        agility: c.agility as i32 + bonus(stat::AGILITY),
        willpower: c.willpower as i32 + bonus(stat::WILLPOWER),
        intelligence: c.intelligence as i32 + bonus(stat::INTELLIGENCE),
        courage: c.courage as i32 + bonus(stat::COURAGE),
    };
    let max_hp = c.hp as i32 + bonus(stat::HEALTH);
    let max_mana = c.mana as i32 + bonus(stat::MANA);
    // DESIGN: bare hands hit like a level-scaled 2 s weapon of half the usual value.
    let weapon = if gear.weapon_value > 0 { gear.weapon_value } else { 1 + lvl };
    Stats {
        level: lvl as u32,
        hp: max_hp,
        max_hp,
        mana: max_mana,
        max_mana,
        // TEXT: "Half of your total strength is applied to total Weapon Value."
        weapon_value: weapon + bonus(stat::WEAPON_VALUE) + attrs.strength / 2,
        armor: gear.armor + bonus(stat::ARMOR_VALUE),
        melee_speed_ms: if gear.speed_ms > 0 { gear.speed_ms } else { 2000 },
        // DESIGN: Agility/Courage "increase the value of Combat/Skill stats".
        melee_crit: (attrs.agility + attrs.courage) / 2 + bonus(stat::MELEE_CRITICAL),
        dodge: attrs.agility / 2 + bonus(stat::DODGE_RATING),
        parry: attrs.strength / 3,
        block: gear.block + bonus(stat::BLOCK_RATING),
        // DESIGN: Intelligence "improves the effectiveness of certain magical abilities".
        spell_crit: (attrs.intelligence + attrs.courage) / 2 + bonus(stat::SPELL_CRITICAL),
        resist: [stat::RESIST_FROST, stat::RESIST_FIRE, stat::RESIST_SHADOW, stat::RESIST_HOLY].map(bonus),
        attrs,
    }
}

/// Uniform sample in [0, 1).
pub trait Roll {
    fn roll(&mut self) -> f32;
}

/// Resolves one melee swing. Rules from the stat texts:
/// - dodge chance = defender DodgeRating / attacker level (%), avoids entirely
/// - parry halves damage, block (shield) removes two thirds
/// - crit chance = attacker MeleeCritical / defender level (%)
/// - armour: "reduces physical damage up to half for 300 AV"
pub fn resolve_melee(att: &Stats, def: &Stats, rng: &mut impl Roll) -> (HitResult, i32) {
    let pct = |v: f32| (v / 100.0).clamp(0.0, 1.0);
    let att_lvl = att.level.max(1) as f32;
    let def_lvl = def.level.max(1) as f32;

    // DESIGN: 5% base miss, +2% per level the defender is above the attacker.
    let miss = 0.05 + 0.02 * (def_lvl - att_lvl).max(0.0);
    if rng.roll() < miss {
        return (HitResult::Miss, 0);
    }
    if rng.roll() < pct(def.dodge as f32 / att_lvl).min(0.3) {
        return (HitResult::Dodge, 0);
    }

    let base = att.weapon_value.max(1) as f32 * (0.8 + 0.4 * rng.roll());
    let armor = 1.0 - 0.5 * (def.armor.max(0) as f32 / 300.0).min(1.0);
    let mut dmg = base * armor;
    let mut result = HitResult::Hit;
    if rng.roll() < pct(def.parry as f32 / att_lvl).min(0.25) {
        dmg *= 0.5;
        result = HitResult::Parry;
    } else if rng.roll() < pct(def.block as f32 / att_lvl).min(0.5) {
        dmg /= 3.0;
        result = HitResult::Block;
    } else if rng.roll() < pct(att.melee_crit as f32 / def_lvl).min(0.5) {
        dmg *= 1.5; // DESIGN: crit multiplier
        result = HitResult::Crit;
    }
    (result, dmg.round().max(1.0) as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(Vec<f32>);
    impl Roll for Fixed {
        fn roll(&mut self) -> f32 {
            if self.0.is_empty() { 0.99 } else { self.0.remove(0) }
        }
    }

    fn unit(level: u32, wv: i32, armor: i32) -> Stats {
        Stats {
            level,
            hp: 100,
            max_hp: 100,
            mana: 0,
            max_mana: 0,
            weapon_value: wv,
            armor,
            melee_speed_ms: 2000,
            melee_crit: 0,
            dodge: 0,
            parry: 0,
            block: 0,
            spell_crit: 0,
            resist: [0; 4],
            attrs: Attributes::default(),
        }
    }

    #[test]
    fn armor_caps_at_half() {
        // rolls: no miss, no dodge, damage roll 0.5 (x1.0), no parry/block/crit
        let (r, d) = resolve_melee(&unit(1, 100, 0), &unit(1, 0, 600), &mut Fixed(vec![0.9, 0.9, 0.5]));
        assert_eq!((r, d), (HitResult::Hit, 50));
        let (_, d) = resolve_melee(&unit(1, 100, 0), &unit(1, 0, 150), &mut Fixed(vec![0.9, 0.9, 0.5]));
        assert_eq!(d, 75);
    }

    #[test]
    fn dodge_scales_with_attacker_level() {
        let mut def = unit(1, 0, 0);
        def.dodge = 20; // 20% vs a level-1 attacker, 10% vs level 2
        let (r, _) = resolve_melee(&unit(1, 10, 0), &def, &mut Fixed(vec![0.9, 0.15]));
        assert_eq!(r, HitResult::Dodge);
        let (r, _) = resolve_melee(&unit(2, 10, 0), &def, &mut Fixed(vec![0.9, 0.15]));
        assert_ne!(r, HitResult::Dodge);
    }

    #[test]
    fn crit_and_parry() {
        let mut att = unit(1, 100, 0);
        att.melee_crit = 100;
        let (r, d) = resolve_melee(&att, &unit(1, 0, 0), &mut Fixed(vec![0.9, 0.9, 0.5, 0.9, 0.9, 0.0]));
        assert_eq!((r, d), (HitResult::Crit, 150));
        let mut def = unit(1, 0, 0);
        def.parry = 100;
        let (r, d) = resolve_melee(&unit(1, 100, 0), &def, &mut Fixed(vec![0.9, 0.9, 0.5, 0.0]));
        assert_eq!((r, d), (HitResult::Parry, 50));
    }
}
