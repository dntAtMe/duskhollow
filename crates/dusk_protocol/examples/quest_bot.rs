//! Headless run of the "First Gaze" quest flow: talks to Ysolde, takes The Red Fields, kills
//! glarewolves, turns in, takes The Warden at the Glare Gate, walks into the `glare_gate`
//! marker, kills Corvin, turns in and waits for the end of the run. Exits non-zero on failure.
//!
//! `cargo run -p dusk_protocol --example quest_bot -- [HOST:PORT] [SECONDS]`
//! (the server must run a map with Ysolde, glarewolves and a `glare_gate` marker).

use dusk_formats::map::{MapFile, WalkGrid};
use dusk_protocol::{ClientMsg, EntityId, EntityKind, PROTOCOL_VERSION, Pos, QuestStatus, ServerMsg, net};
use std::collections::HashMap;
use std::time::{Duration, Instant};

const SPEED: f32 = 3.5;
const MELEE_RANGE: f32 = 1.4;
const TALK_RANGE: f32 = 2.0;
const YSOLDE: i64 = 50010;
const WOLVES: [i64; 2] = [50001, 50002];
const RISEN: [i64; 2] = [50003, 50004];
/// Choices the bot picks when offered (everything that moves the run forward).
const PICK: [&str; 6] = [
    "You look like you need a sword.",
    "I'll see to the wolves.",
    "Four of them. It's done.",
    "Go on.",
    "I'll go to the gate.",
    "Corvin's at rest.",
];

fn dist(a: Pos, b: Pos) -> f32 {
    ((a.x - b.x).powi(2) + (a.y - b.y).powi(2)).sqrt()
}

struct Npc {
    entry: i64,
    pos: Pos,
    dead: bool,
}

fn main() {
    let mut args = std::env::args().skip(1);
    let addr = args.next().unwrap_or_else(|| format!("127.0.0.1:{}", dusk_protocol::DEFAULT_PORT));
    let secs: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(300);
    let conn = net::connect(&addr).expect("connect");
    conn.send(ClientMsg::Hello { protocol: PROTOCOL_VERSION, name: "Questbot".into(), class: 1 });

    let mut me: Option<EntityId> = None;
    let mut pos = Pos { x: 0.0, y: 0.0 };
    let mut npcs: HashMap<EntityId, Npc> = HashMap::new();
    let mut grid: Option<WalkGrid> = None;
    let mut gate: Option<Pos> = None;
    let mut quests: HashMap<u32, QuestStatus> = HashMap::new();
    let mut target: Option<EntityId> = None;
    let mut boss: Option<EntityId> = None;
    let mut route: Vec<(f32, f32)> = Vec::new();
    let mut talked = Instant::now() - Duration::from_secs(10);
    let mut log: Vec<String> = Vec::new();
    let mut ended: Option<(u32, u32)> = None;
    let start = Instant::now();
    let mut last_send = Instant::now();
    let mut say = |s: String| {
        println!("[{:5.1}s] {s}", start.elapsed().as_secs_f32());
        log.push(s);
    };

    while start.elapsed() < Duration::from_secs(secs) && ended.is_none() {
        while let Ok(msg) = conn.incoming.try_recv() {
            match msg {
                ServerMsg::Welcome { your_id, pos: p, map, .. } => {
                    say(format!("welcome #{your_id} on {map} at ({:.1}, {:.1})", p.x, p.y));
                    me = Some(your_id);
                    pos = p;
                    let root = dusk_formats::assets_root().join("maps");
                    grid = Some(MapFile::load(root.join(format!("{map}.map"))).expect("needs assets").walk_grid());
                    let markers = std::fs::read_to_string(root.join(format!("{map}.markers"))).unwrap_or_default();
                    gate = dusk_formats::custom::parse_markers(&markers)
                        .into_iter()
                        .find(|m| m.name == "glare_gate")
                        .map(|m| Pos { x: m.x, y: m.y });
                }
                ServerMsg::Rejected { reason } => panic!("rejected: {reason}"),
                ServerMsg::Spawn(info) => {
                    if let EntityKind::Npc { entry } = info.kind {
                        npcs.insert(info.id, Npc { entry, pos: info.pos, dead: info.dead });
                    }
                }
                ServerMsg::Despawn { id } => {
                    npcs.remove(&id);
                }
                ServerMsg::Moved { id, pos: p, .. } => {
                    if let Some(n) = npcs.get_mut(&id) {
                        n.pos = p;
                    }
                }
                ServerMsg::Correct { pos: p } => {
                    pos = p;
                    route.clear();
                }
                ServerMsg::Died { id } => {
                    if Some(id) == me {
                        say("we died".into());
                        target = None;
                    }
                    if let Some(n) = npcs.get_mut(&id) {
                        n.dead = true;
                    }
                    if Some(id) == target {
                        target = None;
                    }
                }
                ServerMsg::Revive { id, pos: p, .. } if Some(id) == me => {
                    pos = p;
                    route.clear();
                }
                ServerMsg::TargetLost => target = None,
                ServerMsg::Swing { attacker, target: t, amount, .. }
                    if Some(t) == me && std::env::var_os("QB_DMG").is_some() =>
                {
                    say(format!("swing from #{attacker}: {amount}"))
                }
                ServerMsg::SpellHit { caster, target: t, spell, amount, heal: false, .. }
                    if Some(t) == me && std::env::var_os("QB_DMG").is_some() =>
                {
                    say(format!("spell {spell:?} from #{caster}: {amount}"))
                }
                ServerMsg::Health { id, hp, .. } if Some(id) == me && std::env::var_os("QB_DMG").is_some() => {
                    say(format!("hp {hp}"))
                }
                ServerMsg::Health { id, hp, max_hp } if Some(id) == boss && std::env::var_os("QB_DMG").is_some() => {
                    say(format!("boss hp {hp}/{max_hp}"))
                }
                ServerMsg::PlayerStats { level, .. } if std::env::var_os("QB_DMG").is_some() => {
                    say(format!("level {level}"))
                }
                ServerMsg::Dialogue { speaker, text, choices } => {
                    say(format!("dialogue: {:?} {choices:?}", text.lines().next().unwrap_or("")));
                    if let Some(i) = choices.iter().position(|c| PICK.contains(&c.as_str())) {
                        say(format!("  -> {}", choices[i]));
                        conn.send(ClientMsg::DialogueChoice { speaker, index: i as u8 });
                    }
                }
                ServerMsg::Quest(q) => {
                    say(format!("quest {} '{}': {:?} {} {}/{}", q.id, q.title, q.status, q.objective, q.count, q.need));
                    quests.insert(q.id, q.status);
                }
                ServerMsg::QuestMarker { npc, marker } => say(format!("marker on #{npc}: {marker:?}")),
                ServerMsg::BossBar { boss: b } => {
                    say(format!("boss bar: {b:?}"));
                    boss = b;
                }
                ServerMsg::NpcSay { id, text } => {
                    let entry = npcs.get(&id).map_or(0, |n| n.entry);
                    if RISEN.contains(&entry) || entry == YSOLDE {
                        say(format!("{entry} says: {text}"));
                    }
                }
                ServerMsg::Received { item, gold } => say(format!("received {item:?} {gold} gold")),
                ServerMsg::DemoEnd { secs, deaths } => {
                    say(format!("END: {secs}s, {deaths} deaths"));
                    ended = Some((secs, deaths));
                }
                ServerMsg::Lootable { id, lootable: true } => {
                    conn.send(ClientMsg::TakeLoot { corpse: id, index: None });
                }
                _ => {}
            }
        }
        if me.is_none() || last_send.elapsed() < Duration::from_millis(100) {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        last_send = Instant::now();

        let q1 = quests.get(&1).copied();
        let q2 = quests.get(&2).copied();
        let talk = q1.is_none()
            || q1 == Some(QuestStatus::Ready)
            || (q1 == Some(QuestStatus::Done) && q2.is_none())
            || q2 == Some(QuestStatus::Ready);
        let alive = |n: &&Npc| !n.dead;
        // Where to go and what to do there.
        let goal: Option<(Pos, f32)> = if talk {
            let y = npcs.iter().find(|(_, n)| n.entry == YSOLDE).map(|(id, n)| (*id, n.pos));
            match y {
                Some((id, p)) if dist(p, pos) <= TALK_RANGE => {
                    if talked.elapsed() > Duration::from_secs(3) {
                        conn.send(ClientMsg::Interact { target: id });
                        talked = Instant::now();
                    }
                    None
                }
                Some((_, p)) => Some((p, TALK_RANGE * 0.8)),
                None => panic!("no Ysolde on this map"),
            }
        } else {
            let wanted: &[i64] = if q1 == Some(QuestStatus::Active) { &WOLVES } else { &RISEN };
            if target.is_none_or(|t| !npcs.get(&t).is_some_and(|n| !n.dead)) {
                // Corvin first when he is up, else the nearest wanted unit.
                target = boss.filter(|b| npcs.get(b).is_some_and(|n| !n.dead)).or_else(|| {
                    npcs.iter()
                        .filter(|(_, n)| wanted.contains(&n.entry) && alive(n))
                        .min_by(|a, b| dist(a.1.pos, pos).total_cmp(&dist(b.1.pos, pos)))
                        .map(|(id, _)| *id)
                        .filter(|_| q1 == Some(QuestStatus::Active) || boss.is_some())
                });
                if let Some(t) = target {
                    conn.send(ClientMsg::Attack { target: t });
                }
            }
            match target.and_then(|t| npcs.get(&t)) {
                Some(n) if dist(n.pos, pos) <= MELEE_RANGE => None,
                Some(n) => Some((n.pos, MELEE_RANGE * 0.8)),
                // Nothing to fight yet: walk into the gate to wake it.
                None => gate.map(|g| (g, 0.5)),
            }
        };
        let Some((tp, near)) = goal else { continue };
        if dist(tp, pos) <= near {
            continue;
        }
        let g = grid.as_ref().unwrap();
        if route.last().is_none_or(|end| dist(Pos { x: end.0, y: end.1 }, tp) > 1.0) {
            route = g.find_path((pos.x, pos.y), (tp.x, tp.y), 6000).unwrap_or_default();
        }
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
    let Some((secs, deaths)) = ended else {
        eprintln!("quest flow did not finish: quests {quests:?}");
        std::process::exit(1);
    };
    assert_eq!(quests.get(&1), Some(&QuestStatus::Done));
    assert_eq!(quests.get(&2), Some(&QuestStatus::Done));
    assert!(log.iter().any(|l| l.starts_with("boss bar: Some")), "the Warden fight never started");
    assert!(log.iter().any(|l| l.contains("received Some")), "no ember draughts");
    println!("quest flow OK in {secs}s with {deaths} deaths");
}
