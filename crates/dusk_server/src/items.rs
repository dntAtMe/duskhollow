//! Inventories, equipment, item use and NPC loot.
//!
//! Item numbers come from `dusk_formats::item` (shared with the client tooltips). Loot rules are
//! a mix of data (`data/loot.txt`, NPC `junk=` / `loot=` / `loot_chances=` / `gold_ratio=`, the
//! level bands of `data/item_bases.txt`, class `stats=`) and DESIGN. See `docs/items.md`.

use crate::ai::Rng;
use crate::combat::player_stats_msg;
use crate::spells::{CastRequest, CastRequests, Spellbook};
use crate::stats::{Stats, player_stats};
use crate::world::{Dead, GameWorld, Motion, NetId, Npc, OnMap, Outbox, Player, Scope};
use bevy::prelude::*;
use dusk_formats::item::{
    self, Affix, BAG_SLOTS, EQUIP_SLOTS, ItemStats, ItemTemplate, LootRow, NpcLoot, quality, slot,
};
use dusk_protocol::{ClientMsg, CombatStats, EntityId, Item, ServerMsg};
use std::collections::HashMap;
use std::path::Path;

/// Max distance (cells) between a player and a corpse to loot it. DESIGN.
pub const LOOT_RANGE: f32 = 3.0;
/// Corpses with loot stay this long (instead of `combat::CORPSE_SECS`). DESIGN.
const LOOT_CORPSE_SECS: f32 = 60.0;
/// Corpse lingers this long after being emptied. DESIGN.
const LOOTED_CORPSE_SECS: f32 = 3.0;

/// Static item data (`dusk_formats::content::{items, rules, npcs}`).
#[derive(Resource, Default)]
pub struct ItemData {
    pub items: HashMap<i64, ItemTemplate>,
    pub affixes: HashMap<i64, Affix>,
    starting: HashMap<i64, Vec<(i64, i64)>>,
    class_armor: HashMap<i64, Vec<i64>>,
    class_weapons: HashMap<i64, Vec<i64>>,
    desirable_stats: HashMap<i64, Vec<i64>>,
    loot_tables: HashMap<i64, Vec<LootRow>>,
    /// npc model -> junk items
    junk: HashMap<i64, Vec<i64>>,
    npc_loot: HashMap<i64, NpcLoot>,
    /// (quality, required_level) -> generated equippable templates, sorted.
    grid: HashMap<(i64, i64), Vec<i64>>,
}

impl ItemData {
    /// `root`: our content root.
    pub fn load(root: &Path) -> anyhow::Result<Self> {
        use dusk_formats::content;
        let tables = content::items::load(root)?;
        let rules = content::rules::load(root)?;
        let npcs = content::npcs::load(root)?;
        Ok(Self {
            items: tables.items,
            affixes: tables.affixes,
            starting: rules.start_items,
            class_armor: rules.class_armor,
            class_weapons: rules.class_weapons,
            desirable_stats: rules.desirable_stats,
            loot_tables: tables.loot_tables,
            junk: npcs.junk,
            npc_loot: npcs.loot,
            grid: tables.grid,
        })
    }

    fn template(&self, entry: u32) -> Option<&ItemTemplate> {
        self.items.get(&(entry as i64))
    }

    fn can_use(&self, class: i64, t: &ItemTemplate) -> bool {
        item::class_can_use(class, t, &self.class_armor, &self.class_weapons)
    }

    fn max_stack(&self, entry: u32) -> u32 {
        self.template(entry).map(ItemTemplate::max_stack).unwrap_or(1)
    }

    pub fn stats_of(&self, it: &Item) -> ItemStats {
        let Some(t) = self.template(it.entry) else { return ItemStats::default() };
        item::item_stats(t, self.affixes.get(&(it.affix as i64)))
    }

    pub fn name_of(&self, it: &Item) -> String {
        let Some(t) = self.template(it.entry) else { return format!("item #{}", it.entry) };
        item::display_name(t, self.affixes.get(&(it.affix as i64)))
    }
}

/// A player's bags, equipment and purse.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct Inventory {
    pub bag: Vec<Option<Item>>,
    pub equipment: Vec<Option<Item>>,
    pub gold: u32,
}

impl Default for Inventory {
    fn default() -> Self {
        Self { bag: vec![None; BAG_SLOTS], equipment: vec![None; EQUIP_SLOTS], gold: 0 }
    }
}

impl Inventory {
    /// Stacks onto matching stacks first, then fills free slots. Returns what did not fit.
    pub fn add(&mut self, mut item: Item, max_stack: u32) -> Option<Item> {
        let max = max_stack.max(1);
        for s in self.bag.iter_mut().flatten() {
            if s.entry == item.entry && s.affix == item.affix && s.count < max {
                let n = item.count.min(max - s.count);
                s.count += n;
                item.count -= n;
                if item.count == 0 {
                    return None;
                }
            }
        }
        for s in self.bag.iter_mut().filter(|s| s.is_none()) {
            let n = item.count.min(max);
            *s = Some(Item { count: n, ..item });
            item.count -= n;
            if item.count == 0 {
                return None;
            }
        }
        Some(item)
    }

    /// Whether `add` would take all of `item`.
    pub fn fits(&self, item: &Item, max_stack: u32) -> bool {
        self.clone().add(*item, max_stack).is_none()
    }

    /// Equipped `item_template.entry` per slot (0 = empty), for `ServerMsg::Appearance`.
    pub fn appearance(&self) -> Vec<u32> {
        self.equipment.iter().map(|s| s.map_or(0, |i| i.entry)).collect()
    }

    /// Moves bag slot `bag_slot` into its equipment slot, swapping out what was there.
    pub fn equip(&mut self, bag_slot: usize, data: &ItemData, class: i64, level: u32) -> Result<(), String> {
        let it = self.bag.get(bag_slot).copied().flatten().ok_or("No item there")?;
        let t = data.template(it.entry).ok_or("Unknown item")?;
        let slots = item::slots_for(t.equip_type);
        if slots.is_empty() {
            return Err("That can't be equipped".into());
        }
        if t.required_level > level as i64 {
            return Err(format!("Requires level {}", t.required_level));
        }
        if !data.can_use(class, t) {
            return Err("Your class can't use that".into());
        }
        // Rings: first free ring slot, else replace the first.
        let target = slots.iter().copied().find(|s| self.equipment[*s].is_none()).unwrap_or(slots[0]);
        self.bag[bag_slot] = self.equipment[target].take();
        self.equipment[target] = Some(it);
        Ok(())
    }

    pub fn unequip(&mut self, slot: usize) -> Result<(), String> {
        let it = self.equipment.get(slot).copied().flatten().ok_or("Nothing equipped there")?;
        let free = self.bag.iter().position(Option::is_none).ok_or("Inventory is full")?;
        self.bag[free] = Some(it);
        self.equipment[slot] = None;
        Ok(())
    }
}

/// Sum of everything equipped. The main-hand weapon sets weapon value and swing speed;
/// DESIGN: a bow only does so when no melee weapon is equipped (auto attacks are melee).
pub fn gear_stats(inv: &Inventory, data: &ItemData) -> ItemStats {
    let mut total = ItemStats::default();
    for (i, it) in inv.equipment.iter().enumerate() {
        let Some(it) = it else { continue };
        let s = data.stats_of(it);
        total.armor += s.armor;
        total.block += s.block;
        for (st, v) in s.bonuses {
            match total.bonuses.iter_mut().find(|b| b.0 == st) {
                Some(b) => b.1 += v,
                None => total.bonuses.push((st, v)),
            }
        }
        let main = i == slot::WEAPON || (i == slot::RANGED && inv.equipment[slot::WEAPON].is_none());
        if main && s.weapon_value > 0 {
            total.weapon_value = s.weapon_value;
            total.speed_ms = s.speed_ms;
        }
    }
    total
}

/// Cached [`gear_stats`] of a player (level-ups rebuild `Stats` from it).
#[derive(Component, Default, Clone)]
pub struct GearStats(pub ItemStats);

/// Item/loot requests from `net::handle_players`.
#[derive(Resource, Default)]
pub struct ItemRequests(pub Vec<(Entity, ClientMsg)>);

/// NPC kills credited to a player this tick: (npc, killer), from `combat::npc_deaths`.
#[derive(Resource, Default)]
pub struct Kills(pub Vec<(Entity, Entity)>);

/// Unlooted contents of a corpse; only `owner` (the killer) may take them.
#[derive(Component)]
pub struct Loot {
    pub owner: Entity,
    pub gold: u32,
    pub items: Vec<Item>,
}

fn fail(outbox: &mut Outbox, e: Entity, reason: impl Into<String>) {
    outbox.push(Scope::To(e), ServerMsg::ItemError { reason: reason.into() });
}

fn combat_stats(s: &Stats) -> CombatStats {
    CombatStats {
        weapon_value: s.weapon_value,
        melee_speed_ms: s.melee_speed_ms,
        armor: s.armor,
        melee_crit: s.melee_crit,
        spell_crit: s.spell_crit,
        dodge: s.dodge,
        parry: s.parry,
        block: s.block,
        resist: s.resist,
    }
}

fn inventory_msg(inv: &Inventory) -> ServerMsg {
    ServerMsg::Inventory { bag: inv.bag.clone(), equipment: inv.equipment.clone(), gold: inv.gold }
}

/// Everything a player entity carries that the item systems touch.
type PlayerItems<'a> = (
    Entity,
    &'a NetId,
    &'a OnMap,
    &'a Motion,
    &'a Player,
    &'a mut Inventory,
    &'a mut Stats,
    &'a mut GearStats,
    Has<Dead>,
);

/// Recomputes stats from gear (keeping current health/mana, clamped) and tells everyone.
#[allow(clippy::too_many_arguments)]
fn refresh(
    e: Entity,
    id: EntityId,
    map: i64,
    pos: Vec2,
    p: &Player,
    inv: &Inventory,
    stats: &mut Stats,
    gear: &mut GearStats,
    world: &GameWorld,
    data: &ItemData,
    outbox: &mut Outbox,
) {
    let new_gear = gear_stats(inv, data);
    let appearance_changed = gear.0 != new_gear;
    if let Some(cs) = world.class_stats(p.class, stats.level) {
        let mut s = player_stats(cs, &new_gear);
        s.hp = stats.hp.min(s.max_hp);
        s.mana = stats.mana.min(s.max_mana);
        *stats = s;
    }
    gear.0 = new_gear;
    outbox.push(Scope::To(e), inventory_msg(inv));
    outbox.push(Scope::To(e), ServerMsg::CombatStats(combat_stats(stats)));
    outbox.push(Scope::To(e), player_stats_msg(world, p, stats));
    outbox.push(Scope::Near(map, pos), ServerMsg::Health { id, hp: stats.hp, max_hp: stats.max_hp });
    if appearance_changed {
        outbox.push(Scope::Map(map), ServerMsg::Appearance { id, gear: inv.appearance() });
    }
}

/// New players: starting items (class `start_item=`) go on if they can, else into the bags.
#[allow(clippy::too_many_arguments)]
pub fn init_inventories(
    mut commands: Commands,
    world: Res<GameWorld>,
    data: Res<ItemData>,
    mut outbox: ResMut<Outbox>,
    mut joined: Query<(Entity, &NetId, &OnMap, &Motion, &Player, &mut Stats), Added<Player>>,
    others: Query<(&NetId, &OnMap, &Inventory), With<Player>>,
) {
    for (e, id, map, m, p, mut stats) in &mut joined {
        let mut inv = Inventory::default();
        for &(entry, count) in data.starting.get(&p.class).into_iter().flatten() {
            let Some(t) = data.items.get(&entry) else { continue };
            let it = Item { entry: entry as u32, affix: 0, count: count as u32 };
            if inv.add(it, t.max_stack()).is_some() {
                warn!("no room for starting item {entry}");
                continue;
            }
            if t.is_equippable() && item::slots_for(t.equip_type).iter().any(|s| inv.equipment[*s].is_none()) {
                let bag_slot = inv.bag.iter().rposition(|s| s.is_some_and(|s| s.entry == it.entry)).unwrap();
                if let Err(why) = inv.equip(bag_slot, &data, p.class, stats.level) {
                    debug!("starting item {} stays in the bag: {why}", t.name);
                }
            }
        }
        // Existing players' gear for the newcomer (their `Spawn`s were sent directly by `accept`).
        for (oid, omap, oinv) in &others {
            if omap == map && oid != id {
                outbox.push(Scope::To(e), ServerMsg::Appearance { id: oid.0, gear: oinv.appearance() });
            }
        }
        let mut gear = GearStats(ItemStats { weapon_value: -1, ..default() }); // force an Appearance
        let full = (stats.hp >= stats.max_hp, stats.mana >= stats.max_mana);
        refresh(e, id.0, map.0, m.pos, p, &inv, &mut stats, &mut gear, &world, &data, &mut outbox);
        // Gear bonuses on a fresh character start topped up.
        if full.0 {
            stats.hp = stats.max_hp;
        }
        if full.1 {
            stats.mana = stats.max_mana;
        }
        commands.entity(e).insert((inv, gear));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn handle_item_requests(
    mut commands: Commands,
    world: Res<GameWorld>,
    data: Res<ItemData>,
    mut requests: ResMut<ItemRequests>,
    mut outbox: ResMut<Outbox>,
    mut casts: ResMut<CastRequests>,
    mut players: Query<PlayerItems, Without<Npc>>,
    books: Query<&Spellbook>,
    mut corpses: Query<(&NetId, &OnMap, &Motion, &mut Loot, &mut Dead), (With<Npc>, Without<Player>)>,
    index: Res<crate::world::NetIndex>,
) {
    for (e, msg) in std::mem::take(&mut requests.0) {
        let Ok((e, id, map, m, p, mut inv, mut stats, mut gear, dead)) = players.get_mut(e) else { continue };
        let before = inv.clone();
        match msg {
            ClientMsg::EquipItem { .. } | ClientMsg::UnequipItem { .. } | ClientMsg::UseItem { .. } if dead => {
                fail(&mut outbox, e, "You are dead");
            }
            ClientMsg::EquipItem { bag_slot } => {
                if let Err(why) = inv.equip(bag_slot as usize, &data, p.class, stats.level) {
                    fail(&mut outbox, e, why);
                }
            }
            ClientMsg::UnequipItem { slot } => {
                if let Err(why) = inv.unequip(slot as usize) {
                    fail(&mut outbox, e, why);
                }
            }
            ClientMsg::DestroyItem { bag_slot } => {
                if let Some(s) = inv.bag.get_mut(bag_slot as usize) {
                    *s = None;
                }
            }
            ClientMsg::UseItem { bag_slot } => {
                let Some(it) = inv.bag.get(bag_slot as usize).copied().flatten() else { continue };
                let Some(t) = data.template(it.entry) else { continue };
                let Some(&spell) = t.spells.first() else {
                    fail(&mut outbox, e, "That can't be used");
                    continue;
                };
                if t.required_level > stats.level as i64 {
                    fail(&mut outbox, e, format!("Requires level {}", t.required_level));
                } else if books.get(e).is_ok_and(|b| !b.ready(spell as u32)) {
                    fail(&mut outbox, e, "That isn't ready yet");
                } else {
                    // Consumed on use, before the cast resolves (DESIGN).
                    let s = inv.bag[bag_slot as usize].as_mut().unwrap();
                    s.count -= 1;
                    if s.count == 0 {
                        inv.bag[bag_slot as usize] = None;
                    }
                    casts.0.push(CastRequest { caster: e, spell: spell as u32, target: Some(e), from_item: true });
                }
            }
            ClientMsg::OpenLoot { corpse } | ClientMsg::TakeLoot { corpse, .. } => {
                let Some(ce) = index.0.get(&corpse).copied() else { continue };
                let Ok((cid, cmap, cm, mut loot, mut cdead)) = corpses.get_mut(ce) else {
                    outbox.push(Scope::To(e), ServerMsg::Lootable { id: corpse, lootable: false });
                    continue;
                };
                if loot.owner != e {
                    fail(&mut outbox, e, "You can't loot that");
                    continue;
                }
                if cmap != map || cm.pos.distance(m.pos) > LOOT_RANGE {
                    fail(&mut outbox, e, "Too far away");
                    continue;
                }
                if let ClientMsg::TakeLoot { index: which, .. } = msg {
                    take_loot(&mut inv, &mut loot, which.map(|i| i as usize), &data, &mut outbox, e);
                }
                outbox.push(
                    Scope::To(e),
                    ServerMsg::LootWindow { corpse: cid.0, gold: loot.gold, items: loot.items.clone() },
                );
                if loot.gold == 0 && loot.items.is_empty() {
                    commands.entity(ce).remove::<Loot>();
                    cdead.timer = cdead.timer.min(LOOTED_CORPSE_SECS);
                    outbox.push(Scope::To(e), ServerMsg::Lootable { id: cid.0, lootable: false });
                }
            }
            _ => {}
        }
        if *inv != before {
            refresh(e, id.0, map.0, m.pos, p, &inv, &mut stats, &mut gear, &world, &data, &mut outbox);
        }
    }
}

/// Moves loot entry `index` (or everything, gold included) into the bags.
fn take_loot(
    inv: &mut Inventory,
    loot: &mut Loot,
    index: Option<usize>,
    data: &ItemData,
    outbox: &mut Outbox,
    e: Entity,
) {
    if index.is_none() && loot.gold > 0 {
        inv.gold = inv.gold.saturating_add(loot.gold);
        outbox.push(Scope::To(e), ServerMsg::Received { item: None, gold: loot.gold });
        loot.gold = 0;
    }
    let picks: Vec<usize> = match index {
        Some(i) if i < loot.items.len() => vec![i],
        Some(_) => return,
        None => (0..loot.items.len()).collect(),
    };
    let mut full = false;
    // Back to front so earlier indices stay valid while removing.
    for i in picks.into_iter().rev() {
        let it = loot.items[i];
        match inv.add(it, data.max_stack(it.entry)) {
            None => {
                loot.items.remove(i);
                outbox.push(Scope::To(e), ServerMsg::Received { item: Some(it), gold: 0 });
            }
            Some(rest) => {
                if rest.count < it.count {
                    outbox.push(
                        Scope::To(e),
                        ServerMsg::Received { item: Some(Item { count: it.count - rest.count, ..it }), gold: 0 },
                    );
                }
                loot.items[i] = rest;
                full = true;
            }
        }
    }
    if full {
        outbox.push(Scope::To(e), ServerMsg::ItemError { reason: "Inventory is full".into() });
    }
}

/// DESIGN default drop chances (percent) for green / blue / gold / purple items when an NPC's
/// `loot_chances` are -1, plus plain (quality 2) gear.
const DEFAULT_QUALITY_CHANCES: [f32; 4] = [6.0, 1.5, 0.4, 0.1];
const COMMON_GEAR_CHANCE: f32 = 8.0;
/// DESIGN: chance for coins / a junk item on any kill.
const GOLD_CHANCE: f32 = 0.6;
const JUNK_CHANCE: f32 = 0.4;
/// DESIGN: share of equipment drops filtered to what the killer's class can use.
const CLASS_BIAS: f32 = 0.75;

/// What an NPC drops. `elite`/`boss` multiply the default quality chances by 2 / 5. DESIGN.
#[allow(clippy::too_many_arguments)]
pub fn roll_npc_loot(
    data: &ItemData,
    npc_entry: i64,
    npc_model: i64,
    level: u32,
    rank_mult: f32,
    killer_class: i64,
    rng: &mut Rng,
) -> (u32, Vec<Item>) {
    let level = level.max(1) as i64;
    let cfg = data.npc_loot.get(&npc_entry).copied();
    let mut items = Vec::new();

    // Coins: level x U(1, 3), scaled by the NPC's `gold_ratio` percent.
    let ratio = cfg.map(|c| c.gold_ratio).filter(|r| *r >= 0).unwrap_or(100) as f32 / 100.0;
    let gold = if rng.next_f32() < GOLD_CHANCE {
        (level as f32 * rng.range(1.0, 3.0) * ratio * rank_mult).round() as u32
    } else {
        0
    };

    // Junk of the npc model, closest `item_level` to the npc's level.
    if rng.next_f32() < JUNK_CHANCE
        && let Some(list) = data.junk.get(&npc_model)
    {
        let lvl_of = |e: &i64| data.items.get(e).map(|t| t.item_level).unwrap_or(0);
        let best = list.iter().filter(|e| data.items.contains_key(e)).map(|e| (lvl_of(e) - level).abs()).min();
        let pool: Vec<i64> = list.iter().copied().filter(|e| Some((lvl_of(e) - level).abs()) == best).collect();
        if let Some(&e) = pick(&pool, rng) {
            items.push(Item { entry: e as u32, affix: 0, count: 1 });
        }
    }

    // Its hand-made table (`loot=` -> `data/loot.txt`).
    if let Some(rows) = cfg.filter(|c| c.custom_loot > 0).and_then(|c| data.loot_tables.get(&c.custom_loot)) {
        for r in rows.iter().filter(|r| !r.conditional && data.items.contains_key(&r.item)) {
            if rng.next_f32() * 100.0 < r.chance {
                let n = r.count_min + (rng.next_f32() * (r.count_max - r.count_min + 1) as f32) as i64;
                items.push(Item { entry: r.item as u32, affix: 0, count: n.clamp(1, r.count_max) as u32 });
            }
        }
    }

    // At most one random piece of gear; best quality first.
    let chances = cfg.map(|c| c.chances).unwrap_or([-1.0; 4]);
    let mut q = None;
    for (i, &c) in chances.iter().enumerate().rev() {
        let c = if c >= 0.0 { c } else { DEFAULT_QUALITY_CHANCES[i] * rank_mult };
        if rng.next_f32() * 100.0 < c {
            q = Some(quality::GREEN + i as i64);
            break;
        }
    }
    if q.is_none() && rng.next_f32() * 100.0 < COMMON_GEAR_CHANCE * rank_mult {
        q = Some(quality::COMMON);
    }
    if let Some(it) = q.and_then(|q| random_gear(data, q, level.min(25), killer_class, rng)) {
        items.push(it);
    }
    (gold, items)
}

fn pick<'a, T>(v: &'a [T], rng: &mut Rng) -> Option<&'a T> {
    if v.is_empty() { None } else { v.get((rng.next_f32() * v.len() as f32) as usize % v.len()) }
}

/// A generated item of quality `q` and required level `level`: slot type uniform, then a base
/// whose level band covers `level` (the band is its tier), affix from the level band for green
/// and better.
pub fn random_gear(data: &ItemData, q: i64, level: i64, class: i64, rng: &mut Rng) -> Option<Item> {
    let all = data.grid.get(&(q, level))?;
    let usable: Vec<i64> = all.iter().copied().filter(|e| data.can_use(class, &data.items[e])).collect();
    let pool = if !usable.is_empty() && rng.next_f32() < CLASS_BIAS { &usable } else { all };
    let mut types: Vec<i64> = pool.iter().map(|e| data.items[e].equip_type).collect();
    types.sort_unstable();
    types.dedup();
    let ty = *pick(&types, rng)?;
    let cands: Vec<i64> = pool.iter().copied().filter(|e| data.items[e].equip_type == ty).collect();
    let entry = *pick(&cands, rng)?;
    let affix = if q >= quality::GREEN { random_affix(data, level, class, rng) } else { 0 };
    Some(Item { entry: entry as u32, affix, count: 1 })
}

/// DESIGN: 75% of affixes only carry stats the killer's class favours (class `stats=`).
fn random_affix(data: &ItemData, level: i64, class: i64, rng: &mut Rng) -> u32 {
    let mut band: Vec<&Affix> =
        data.affixes.values().filter(|a| a.min_level <= level && level <= a.max_level).collect();
    band.sort_unstable_by_key(|a| a.entry);
    let wanted = data.desirable_stats.get(&class);
    let fitting: Vec<&Affix> =
        band.iter().copied().filter(|a| wanted.is_some_and(|w| a.stats.iter().all(|(s, _)| w.contains(s)))).collect();
    let pool = if !fitting.is_empty() && rng.next_f32() < CLASS_BIAS { &fitting } else { &band };
    pick(pool, rng).map_or(0, |a| a.entry as u32)
}

/// Turns this tick's kills into lootable corpses.
pub fn roll_loot(
    mut commands: Commands,
    world: Res<GameWorld>,
    data: Res<ItemData>,
    mut kills: ResMut<Kills>,
    mut outbox: ResMut<Outbox>,
    mut rng: Local<Rng>,
    npcs: Query<(&NetId, &Npc, &Stats)>,
    players: Query<&Player>,
) {
    for (npc, killer) in kills.0.drain(..) {
        let (Ok((nid, n, s)), Ok(p)) = (npcs.get(npc), players.get(killer)) else { continue };
        let Some(t) = world.npc_templates.get(&n.entry) else { continue };
        let rank = if t.boss {
            5.0
        } else if t.elite {
            2.0
        } else {
            1.0
        };
        let (gold, items) = roll_npc_loot(&data, n.entry, t.model_id, s.level, rank, p.class, &mut rng);
        if gold == 0 && items.is_empty() {
            continue;
        }
        commands.entity(npc).insert((Loot { owner: killer, gold, items }, Dead { timer: LOOT_CORPSE_SECS }));
        outbox.push(Scope::To(killer), ServerMsg::Lootable { id: nid.0, lootable: true });
    }
}

/// Corpses that disappeared (or respawned) lose their loot.
pub fn expire_loot(
    mut commands: Commands,
    mut outbox: ResMut<Outbox>,
    gone: Query<(Entity, &NetId, &Loot), Without<Dead>>,
) {
    for (e, id, loot) in &gone {
        commands.entity(e).remove::<Loot>();
        outbox.push(Scope::To(loot.owner), ServerMsg::Lootable { id: id.0, lootable: false });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dusk_formats::item::equip;

    fn data() -> ItemData {
        let mut d = ItemData::default();
        let mk = |entry, equip_type, weapon_type, armor_type, level, stack| ItemTemplate {
            entry,
            name: format!("item{entry}"),
            equip_type,
            weapon_type,
            armor_type,
            required_level: level,
            quality: quality::COMMON,
            stack_count: stack,
            ..default()
        };
        for t in [
            mk(1, 0, 0, 0, 1, 5),              // potion
            mk(18, equip::WEAPON, 4, 0, 1, 1), // sword
            mk(19, equip::RANGED, 2, 0, 1, 1), // bow
            mk(30, equip::RING, 0, 0, 1, 1),   // ring
            mk(40, equip::CHEST, 0, 9, 10, 1), // level-10 plate
            mk(41, equip::CHEST, 0, 1, 1, 1),  // cloth shirt
            mk(50, equip::WEAPON, 5, 0, 1, 1), // staff
        ] {
            d.items.insert(t.entry, t);
        }
        d.class_armor.insert(1, vec![2, 3, 4, 5, 6, 7, 8, 9, 10, 11]);
        d.class_weapons.insert(1, vec![1, 3, 4, 6, 10]);
        d
    }

    fn it(entry: u32, count: u32) -> Item {
        Item { entry, affix: 0, count }
    }

    #[test]
    fn stacking_fills_existing_stacks_first() {
        let mut inv = Inventory::default();
        assert_eq!(inv.add(it(1, 3), 5), None);
        assert_eq!(inv.add(it(1, 4), 5), None);
        assert_eq!(inv.bag[0], Some(it(1, 5)));
        assert_eq!(inv.bag[1], Some(it(1, 2)));
        // Full bag: the rest comes back.
        for s in inv.bag.iter_mut().skip(2) {
            *s = Some(it(18, 1));
        }
        assert_eq!(inv.add(it(1, 5), 5), Some(it(1, 2)));
        assert!(!inv.fits(&it(18, 1), 1));
    }

    #[test]
    fn equip_validates_and_swaps() {
        let d = data();
        let mut inv = Inventory::default();
        inv.add(it(40, 1), 1);
        inv.add(it(18, 1), 1);
        inv.add(it(50, 1), 1);
        assert_eq!(inv.equip(0, &d, 1, 5).unwrap_err(), "Requires level 10");
        assert!(inv.equip(0, &d, 1, 10).is_ok());
        assert!(inv.equip(2, &d, 1, 10).is_err(), "paladins can't use staves");
        assert!(inv.equip(1, &d, 1, 1).is_ok());
        assert_eq!(inv.equipment[slot::WEAPON], Some(it(18, 1)));
        // Equipping a shirt swaps the plate back into the bag slot.
        inv.bag[1] = Some(it(41, 1));
        inv.equip(1, &d, 1, 10).unwrap();
        assert_eq!(inv.bag[1], Some(it(40, 1)));
        assert!(inv.unequip(slot::CHEST).is_ok());
        assert!(inv.unequip(slot::CHEST).is_err());
    }

    #[test]
    fn rings_fill_both_slots() {
        let d = data();
        let mut inv = Inventory::default();
        inv.add(it(30, 1), 1);
        inv.add(it(30, 1), 1);
        inv.equip(0, &d, 1, 1).unwrap();
        inv.equip(1, &d, 1, 1).unwrap();
        assert!(inv.equipment[slot::RING1].is_some() && inv.equipment[slot::RING2].is_some());
    }

    #[test]
    fn bow_is_the_weapon_only_without_a_melee_weapon() {
        let d = data();
        let mut inv = Inventory::default();
        inv.equipment[slot::RANGED] = Some(it(19, 1));
        let bow = gear_stats(&inv, &d);
        assert_eq!(bow.speed_ms, 2400);
        inv.equipment[slot::WEAPON] = Some(it(18, 1));
        assert_eq!(gear_stats(&inv, &d).speed_ms, 2000);
    }

    #[test]
    fn loot_rolls_are_sane_on_real_data() {
        let d = ItemData::load(&dusk_formats::content_root()).unwrap();
        let mut rng = Rng::default();
        let (mut gold, mut drops) = (0, 0);
        for i in 0..2000 {
            let (g, items) = roll_npc_loot(&d, 50001, 50001, 1 + i % 25, 1.0, 1 + (i as i64 % 4), &mut rng);
            gold += g;
            drops += items.len();
            for it in items {
                let t = &d.items[&(it.entry as i64)];
                assert!(it.count >= 1 && it.count <= t.max_stack().max(it.count));
                if t.generated && t.quality >= quality::GREEN {
                    assert!(d.affixes.contains_key(&(it.affix as i64)), "{} without affix", t.name);
                }
            }
        }
        assert!(gold > 0 && drops > 100, "gold {gold}, drops {drops}");
        // Corvin's table and the glarewolves' junk come through.
        let (mut named, mut junk) = (0, 0);
        for _ in 0..300 {
            let (_, items) = roll_npc_loot(&d, 50004, 50004, 4, 5.0, 1, &mut rng);
            named += items.iter().filter(|i| (1001..=1003).contains(&i.entry)).count();
            let (_, items) = roll_npc_loot(&d, 50001, 50001, 1, 1.0, 1, &mut rng);
            junk += items.iter().filter(|i| (100..=102).contains(&i.entry)).count();
        }
        assert!(named > 100 && junk > 60, "named {named}, junk {junk}");
        // Every class gets gear it can use most of the time.
        for class in 1..=4 {
            let usable = (0..200)
                .filter_map(|_| random_gear(&d, quality::GREEN, 5, class, &mut rng))
                .filter(|it| d.can_use(class, &d.items[&(it.entry as i64)]))
                .count();
            assert!(usable > 120, "class {class}: {usable}/200 usable");
        }
    }
}
