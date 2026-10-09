//! Headless test client. Joins, then hunts: walks to the nearest attackable NPC and
//! auto-attacks it until one of them dies, loots the corpse, reporting what the server sent.
//!
//! `cargo run -p dusk_protocol --example bot -- [HOST:PORT] [NAME] [SECONDS]`

use dusk_formats::map::{MapFile, WalkGrid};
use dusk_protocol::{ClientMsg, EntityId, EntityKind, HitResult, PROTOCOL_VERSION, Pos, ServerMsg, net};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

const SPEED: f32 = 3.5;
const MELEE_RANGE: f32 = 1.4;

fn dist(a: Pos, b: Pos) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| format!("127.0.0.1:{}", dusk_protocol::DEFAULT_PORT));
    let name = args.next().unwrap_or_else(|| "Bot".into());
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(40);
    let conn = net::connect(&addr).expect("connect");
    conn.send(ClientMsg::Hello { protocol: PROTOCOL_VERSION, name, class: 1 });

    let mut me: Option<EntityId> = None;
    let mut pos = Pos { x: 0.0, y: 0.0 };
    let mut npcs: HashMap<EntityId, Pos> = HashMap::new();
    let mut rejected: HashSet<EntityId> = HashSet::new();
    let mut target: Option<EntityId> = None;
    let mut stats: HashMap<String, i64> = HashMap::new();
    let mut bump = |k: &str, n: i64| *stats.entry(k.to_string()).or_default() += n;
    let start = Instant::now();
    let mut last_send = Instant::now();
    let mut corrections_logged = 0;
    let mut grid: Option<WalkGrid> = None;
    let mut route: Vec<(f32, f32)> = Vec::new();
    let mut known: Vec<u32> = Vec::new();
    let mut ready_at: HashMap<u32, Instant> = HashMap::new();
    let mut hp = (1, 1);
    let mut errors: HashMap<String, u32> = HashMap::new();
    // (bag stacks, equipped pieces, gold) and visible gear pieces.
    let mut inventory = (0, 0, 0);
    let mut gear_seen = 0;

    while start.elapsed() < Duration::from_secs(secs) {
        while let Ok(msg) = conn.incoming.try_recv() {
            match msg {
                ServerMsg::Welcome { your_id, pos: p, map, .. } => {
                    println!("welcome #{your_id} on {map} at ({:.1}, {:.1})", p.x, p.y);
                    me = Some(your_id);
                    pos = p;
                    let path = dusk_formats::content::maps::map_file(&dusk_formats::assets_root(), &map, "map")
                        .expect("bot needs the map file");
                    grid = Some(MapFile::load(path).expect("bot needs the map file").walk_grid());
                }
                ServerMsg::Rejected { reason } => panic!("rejected: {reason}"),
                ServerMsg::Spawn(info) => {
                    if matches!(info.kind, EntityKind::Npc { .. }) && !info.dead {
                        npcs.insert(info.id, info.pos);
                    }
                }
                ServerMsg::Despawn { id } => {
                    npcs.remove(&id);
                }
                ServerMsg::Moved { id, pos: p, .. } => {
                    if let Some(n) = npcs.get_mut(&id) {
                        *n = p;
                    }
                }
                ServerMsg::Correct { pos: p } => {
                    if corrections_logged < 3 {
                        corrections_logged += 1;
                        println!(
                            "corrected ({:.2}, {:.2}) -> ({:.2}, {:.2}), target {target:?}",
                            pos.x, pos.y, p.x, p.y
                        );
                    }
                    pos = p;
                    route.clear();
                    bump("corrections", 1);
                }
                ServerMsg::Swing { attacker, target: t, result, amount } => {
                    if Some(attacker) == me {
                        bump("swings_out", 1);
                        bump("dmg_out", amount as i64);
                        if result == HitResult::Crit {
                            bump("crits_out", 1);
                        }
                    } else if Some(t) == me {
                        bump("swings_in", 1);
                        bump("dmg_in", amount as i64);
                    }
                }
                ServerMsg::Died { id } => {
                    if Some(id) == me {
                        println!("we died at {:.1}s", start.elapsed().as_secs_f32());
                        bump("deaths", 1);
                    } else if Some(id) == target {
                        println!("killed #{id} at {:.1}s", start.elapsed().as_secs_f32());
                        bump("kills", 1);
                        npcs.remove(&id);
                        target = None;
                    }
                }
                ServerMsg::Revive { id, pos: p, .. } if Some(id) == me => {
                    pos = p;
                    target = None;
                }
                ServerMsg::PlayerStats { level, xp, xp_next, .. } => {
                    println!("stats: level {level}, xp {xp}/{xp_next}");
                }
                ServerMsg::KnownSpells { spells } => {
                    println!("known spells: {spells:?}");
                    known = spells;
                }
                ServerMsg::Cooldown { spell, ms, gcd_ms } => {
                    let now = Instant::now();
                    ready_at.insert(spell, now + Duration::from_millis(ms as u64));
                    ready_at.insert(0, now + Duration::from_millis(gcd_ms as u64));
                }
                ServerMsg::SpellHit { caster, target: t, spell, amount, heal, .. } => {
                    if Some(caster) == me {
                        bump(if heal { "spell_heal" } else { "spell_dmg" }, amount as i64);
                        bump(&format!("spell_{spell}_hits"), 1);
                    } else if Some(t) == me && !heal {
                        bump("spell_dmg_in", amount as i64);
                    }
                }
                ServerMsg::AuraApply { target: t, .. } if Some(t) == target || Some(t) == me => bump("auras", 1),
                ServerMsg::CastError { reason } => *errors.entry(reason).or_default() += 1,
                ServerMsg::Health { id, hp: h, max_hp } if Some(id) == me => hp = (h, max_hp),
                // Loot every corpse we are offered (we are next to it: we just killed it).
                ServerMsg::Lootable { id, lootable: true } => {
                    conn.send(ClientMsg::TakeLoot { corpse: id, index: None });
                }
                ServerMsg::Received { item, gold } => {
                    bump("loot_gold", gold as i64);
                    bump("loot_items", item.map_or(0, |i| i.count as i64));
                }
                ServerMsg::ItemError { reason } => *errors.entry(reason).or_default() += 1,
                ServerMsg::Inventory { bag, equipment, gold } => {
                    inventory = (bag.iter().flatten().count(), equipment.iter().flatten().count(), gold);
                }
                ServerMsg::Appearance { id, gear } if Some(id) == me => {
                    gear_seen = gear.iter().filter(|g| **g != 0).count();
                }
                ServerMsg::TargetLost => {
                    if let Some(t) = target.take() {
                        rejected.insert(t);
                    }
                }
                _ => {}
            }
        }
        if me.is_none() || last_send.elapsed() < Duration::from_millis(100) {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        last_send = Instant::now();

        if target.is_none() {
            target = npcs
                .iter()
                .filter(|(id, _)| !rejected.contains(id))
                .min_by(|a, b| dist(*a.1, pos).total_cmp(&dist(*b.1, pos)))
                .map(|(id, _)| *id);
            if let Some(t) = target {
                conn.send(ClientMsg::Attack { target: t });
            }
            continue;
        }
        let Some(tp) = target.and_then(|t| npcs.get(&t)).copied() else { continue };
        if dist(tp, pos) <= MELEE_RANGE {
            route.clear();
            // Spell rotation: heal when low, else nukes when off cooldown.
            let now = Instant::now();
            let ready =
                |s: u32| ready_at.get(&s).is_none_or(|t| *t <= now) && ready_at.get(&0).is_none_or(|t| *t <= now);
            let plan: &[(u32, bool)] = if hp.0 * 2 < hp.1 { &[(10, true)] } else { &[(9, false), (13, false)] };
            if let Some(&(spell, on_self)) = plan.iter().find(|(s, _)| known.contains(s) && ready(*s)) {
                conn.send(ClientMsg::CastSpell { spell, target: if on_self { me } else { target } });
                ready_at.insert(0, now + Duration::from_millis(1500)); // until the server answers
            }
            continue;
        }
        let g = grid.as_ref().unwrap();
        if route.last().is_none_or(|end| dist(Pos { x: end.0, y: end.1 }, tp) > 1.0) {
            route = g.find_path((pos.x, pos.y), (tp.x, tp.y), 4000).unwrap_or_default();
        }
        // Walk 0.1s worth along the route.
        let mut budget = SPEED * 0.1;
        while budget > 0.0 && !route.is_empty() {
            let w = Pos { x: route[0].0, y: route[0].1 };
            let d = dist(w, pos);
            if d <= budget {
                pos = w;
                budget -= d;
                route.remove(0);
            } else {
                pos.x += (w.x - pos.x) / d * budget;
                pos.y += (w.y - pos.y) / d * budget;
                budget = 0.0;
            }
        }
        conn.send(ClientMsg::Move { pos, orientation: (tp.y - pos.y).atan2(tp.x - pos.x), moving: true });
    }
    println!("summary: {stats:?}");
    println!("cast/item errors: {errors:?}");
    println!("inventory: {} bag stacks, {} equipped, {} gold", inventory.0, inventory.1, inventory.2);
    assert!(inventory.1 > 0 && gear_seen == inventory.1, "expected starting gear to be equipped and visible");
    println!("non-attackable targets skipped: {}", rejected.len());
    assert!(stats.get("swings_out").copied().unwrap_or(0) > 0, "expected to swing at something");
}
