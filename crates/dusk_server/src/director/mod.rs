//! The demo director: talking to NPCs, the two quests of "First Gaze", the scripted Warden fight
//! at the Glare Gate (which forces the Eye open via [`EyeCommand`]), villager barks and the end
//! of the run. The run itself is data in [`script`].
//!
//! Tolerant by design: without the demo's NPC templates nothing here triggers, and without a
//! `glare_gate` marker (`maps/<map>.markers`) the Warden fight simply never starts.
//!
//! Debug: `DUSK_QUEST_TEST=<stage>` starts every new player at that point of the run
//! (see [`script::Progress::parse_test`]).

pub mod script;

use crate::ai::Rng;
use crate::combat::Attacking;
use crate::eye::EyeCommand;
use crate::items::{Inventory, ItemData, Kills};
use crate::spells::{NpcSpellState, NpcSpells, Spellbook};
use crate::stats::{Stats, npc_stats};
use crate::world::{
    Dead, Faction, GameWorld, Hidden, Home, Motion, NetId, NetIndex, Npc, OnMap, Outbox, Player, Scope, entity_info,
};
use bevy::prelude::*;
use dusk_formats::content::sidecars::{Marker, parse_markers};
use dusk_formats::content::types::faction;
use dusk_protocol::{ClientMsg, EntityId, Item, QuestMarker, ServerMsg};
use script::{Action, Outcome, Progress, entry};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Max distance (cells) between a player and the NPC they talk to.
pub const INTERACT_RANGE: f32 = 3.5;
/// Stooped that rise around the gate with Corvin.
const STOOPED_RISEN: usize = 2;
/// How long the Eye is forced open at most (it settles earlier when Corvin falls).
const EYE_OPEN_SECS: f32 = 300.0;
/// The fight resets after this long without anyone on the quest near the gate.
const RESET_SECS: f32 = 45.0;
/// Extra cells around the gate radius that still count as "at the fight".
const FIGHT_MARGIN: f32 = 25.0;
/// Villagers bark when a player is this close (cells).
const BARK_RADIUS: f32 = 12.0;

pub fn plugin(app: &mut App, assets: &Path) {
    app.add_message::<EyeCommand>()
        .insert_resource(Director { root: assets.to_path_buf(), ..default() })
        .init_resource::<DirectorRequests>()
        .add_systems(Startup, setup.after(crate::world::spawn_npcs))
        .add_systems(
            Update,
            (
                (join, handle_requests).chain().after(crate::net::handle_players),
                count_kills.after(crate::combat::npc_deaths).before(crate::items::roll_loot),
                (count_deaths, encounters, keep_corpses, cleanup_risen, barks, sync_markers)
                    .chain()
                    .after(crate::items::expire_loot)
                    .before(crate::net::broadcast_motion),
            ),
        );
}

/// `Interact` / `DialogueChoice` messages, queued by `net::handle_players`.
#[derive(Resource, Default)]
pub struct DirectorRequests(pub Vec<(Entity, ClientMsg)>);

#[derive(Resource, Default)]
pub struct Director {
    root: PathBuf,
    /// Map id -> named points from `maps/<name>.markers`.
    markers: HashMap<i64, Vec<Marker>>,
    encounters: HashMap<i64, Encounter>,
    test_stage: Option<Progress>,
}

impl Director {
    fn marker(&self, map: i64, name: &str) -> Option<&Marker> {
        self.markers.get(&map)?.iter().find(|m| m.name == name)
    }
}

#[derive(Default)]
enum Encounter {
    #[default]
    Dormant,
    Active {
        boss: Entity,
        risen: Vec<Entity>,
        /// Seconds without anyone on the quest near the gate.
        empty_for: f32,
    },
    Won,
}

/// A player's place in the run, plus the dialogue they have open.
#[derive(Component)]
pub struct Quester {
    pub progress: Progress,
    /// `Time::elapsed_secs` when they joined.
    joined: f32,
    deaths: u32,
    dialogue: Option<OpenDialogue>,
    /// Last quest-giver marker sent.
    marker: Option<QuestMarker>,
}

struct OpenDialogue {
    speaker: EntityId,
    choices: Vec<Action>,
}

/// Units the director raised (Corvin, the Stooped): they never respawn.
#[derive(Component)]
struct Risen;

/// A corpse that stays (Corvin keeps his post).
#[derive(Component)]
struct KeepCorpse;

fn setup(mut director: ResMut<Director>, mut world: ResMut<GameWorld>) {
    let maps: Vec<(i64, String)> = world.maps.iter().map(|(id, m)| (*id, m.name.clone())).collect();
    for (id, name) in maps {
        let path = director.root.join("maps").join(format!("{name}.markers"));
        if let Ok(text) = std::fs::read_to_string(&path) {
            let markers = parse_markers(&text);
            info!("director: {} markers on {name}", markers.len());
            director.markers.insert(id, markers);
        }
    }
    // New characters arrive at the `arrival` marker of the start map, if it has one.
    let start_map = world.start.0;
    if let Some(m) = director.marker(start_map, "arrival") {
        let want = (m.x, m.y);
        let (x, y) = world.maps.get(&start_map).and_then(|g| g.grid.nearest_floor(want)).unwrap_or(want);
        world.start.1 = Vec2::new(x, y);
    }
    director.test_stage = std::env::var("DUSK_QUEST_TEST").ok().and_then(|s| Progress::parse_test(&s));
}

fn join(
    mut commands: Commands,
    time: Res<Time>,
    director: Res<Director>,
    mut outbox: ResMut<Outbox>,
    joined: Query<Entity, Added<Player>>,
) {
    for e in &joined {
        let progress = director.test_stage.unwrap_or_default();
        for q in progress.open_quests() {
            outbox.push(Scope::To(e), ServerMsg::Quest(q));
        }
        commands.entity(e).insert(Quester {
            progress,
            joined: time.elapsed_secs(),
            deaths: 0,
            dialogue: None,
            marker: None,
        });
    }
}

type NpcView<'a> = (&'a NetId, &'a Npc, &'a OnMap, &'a Faction, &'a mut Motion, Has<Dead>);

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn handle_requests(
    time: Res<Time>,
    index: Res<NetIndex>,
    data: Res<ItemData>,
    mut requests: ResMut<DirectorRequests>,
    mut outbox: ResMut<Outbox>,
    mut rng: Local<Rng>,
    mut players: Query<
        (
            (&OnMap, &Motion, &mut Quester, Option<&mut Inventory>, Has<Dead>),
            (&NetId, &mut Player, &mut Stats, Option<&crate::items::GearStats>),
        ),
        Without<Npc>,
    >,
    mut npcs: Query<NpcView, (Without<Player>, Without<Hidden>)>,
    world: Res<GameWorld>,
) {
    for (pe, msg) in requests.0.drain(..) {
        let Ok(((pmap, pm, mut quester, mut inv, dead), (pid, mut player, mut stats, gear))) = players.get_mut(pe)
        else {
            continue;
        };
        let (target, choice) = match msg {
            ClientMsg::Interact { target } => (target, None),
            ClientMsg::DialogueChoice { speaker, index } => (speaker, Some(index)),
            _ => continue,
        };
        let npc = index.0.get(&target).and_then(|e| npcs.get_mut(*e).ok());
        let Some((nid, npc, nmap, nfaction, mut nm, ndead)) = npc else {
            quester.dialogue = None;
            continue;
        };
        if dead || ndead || nmap != pmap || nfaction.0 != faction::FRIENDLY {
            quester.dialogue = None;
            continue;
        }
        if nm.pos.distance(pm.pos) > INTERACT_RANGE + if choice.is_some() { 2.0 } else { 0.0 } {
            quester.dialogue = None;
            outbox.push(Scope::To(pe), ServerMsg::CastError { reason: "Too far away".into() });
            continue;
        }

        let action = match choice {
            None => {
                // Turn to face whoever is talking.
                nm.orientation = crate::ai::orientation(pm.pos - nm.pos);
                nm.dirty = true;
                if npc.entry == entry::YSOLDE {
                    Action::Goto(script::ysolde_entry(&quester.progress))
                } else {
                    let lines = script::talk_lines(npc.entry);
                    let line = lines[(rng.next_f32() * lines.len() as f32) as usize % lines.len()];
                    show(&mut outbox, pe, &mut quester, nid.0, line, vec![("Goodbye.", Action::Close)]);
                    continue;
                }
            }
            Some(i) => {
                let open = quester.dialogue.take().filter(|d| d.speaker == nid.0);
                match open.and_then(|d| d.choices.get(i as usize).copied()) {
                    Some(a) => a,
                    None => continue,
                }
            }
        };
        let next = match action {
            Action::Close => None,
            Action::Goto(n) => Some(n),
            Action::Accept(q) | Action::TurnIn(q) => {
                let outcomes = quester.progress.apply(action);
                if outcomes.is_empty() {
                    None
                } else {
                    let now = time.elapsed_secs();
                    for o in outcomes {
                        if let Outcome::Reward(_) = o {
                            // DESIGN: each demo quest is worth one level at the player's current level.
                            let xp = world.xp_to_next(stats.level);
                            let who = (pid, pmap, pm);
                            crate::combat::grant_xp(&world, &mut outbox, pe, who, &mut player, &mut stats, gear, xp);
                        }
                        handle_outcome(&mut outbox, &data, pe, &mut quester, inv.as_deref_mut(), o, now);
                    }
                    Some(if matches!(action, Action::Accept(_)) {
                        script::after_accept(q)
                    } else {
                        script::after_turn_in(q)
                    })
                }
            }
        };
        if let Some(node) = next.filter(|_| npc.entry == entry::YSOLDE) {
            let page = script::ysolde(node);
            show(&mut outbox, pe, &mut quester, nid.0, page.text, page.choices);
        }
    }
}

fn show(
    outbox: &mut Outbox,
    player: Entity,
    quester: &mut Quester,
    speaker: EntityId,
    text: &str,
    choices: Vec<(&str, Action)>,
) {
    outbox.push(
        Scope::To(player),
        ServerMsg::Dialogue {
            speaker,
            text: text.to_string(),
            choices: choices.iter().map(|(l, _)| l.to_string()).collect(),
        },
    );
    quester.dialogue = Some(OpenDialogue { speaker, choices: choices.into_iter().map(|(_, a)| a).collect() });
}

fn handle_outcome(
    outbox: &mut Outbox,
    data: &ItemData,
    player: Entity,
    quester: &mut Quester,
    inv: Option<&mut Inventory>,
    outcome: Outcome,
    now: f32,
) {
    match outcome {
        Outcome::Quest(q) => outbox.push(Scope::To(player), ServerMsg::Quest(q)),
        Outcome::Reward(quest) => {
            let Some(inv) = inv else { return };
            if quest == script::RED_FIELDS {
                let item = Item { entry: script::EMBER_DRAUGHT, affix: 0, count: script::EMBER_DRAUGHTS };
                let Some(t) = data.items.get(&(item.entry as i64)) else { return };
                let rest = inv.add(item, t.max_stack()).map_or(0, |r| r.count);
                if rest < item.count {
                    let got = Item { count: item.count - rest, ..item };
                    outbox.push(Scope::To(player), ServerMsg::Received { item: Some(got), gold: 0 });
                }
                if rest > 0 {
                    outbox.push(Scope::To(player), ServerMsg::ItemError { reason: "Inventory is full".into() });
                }
            } else {
                inv.gold += script::WARDEN_GOLD;
                outbox.push(Scope::To(player), ServerMsg::Received { item: None, gold: script::WARDEN_GOLD });
            }
            outbox.push(
                Scope::To(player),
                ServerMsg::Inventory { bag: inv.bag.clone(), equipment: inv.equipment.clone(), gold: inv.gold },
            );
        }
        Outcome::End => outbox.push(
            Scope::To(player),
            ServerMsg::DemoEnd { secs: (now - quester.joined).max(0.0) as u32, deaths: quester.deaths },
        ),
    }
}

/// Glarewolf kills for The Red Fields (reads this tick's kills before `roll_loot` drains them).
pub fn count_kills(
    kills: Res<Kills>,
    mut outbox: ResMut<Outbox>,
    npcs: Query<&Npc>,
    mut questers: Query<&mut Quester>,
) {
    for &(npc, killer) in &kills.0 {
        let (Ok(n), Ok(mut q)) = (npcs.get(npc), questers.get_mut(killer)) else { continue };
        if let Some(info) = q.progress.on_kill(n.entry) {
            outbox.push(Scope::To(killer), ServerMsg::Quest(info));
        }
    }
}

fn count_deaths(mut died: Query<&mut Quester, (Added<Dead>, With<Player>)>) {
    for mut q in &mut died {
        q.deaths += 1;
    }
}

/// Spawns an NPC from its template the way `world::spawn_npcs` does, minus wandering and
/// respawning; announces it to the map.
fn raise(
    commands: &mut Commands,
    world: &mut GameWorld,
    index: &mut NetIndex,
    outbox: &mut Outbox,
    npc_entry: i64,
    map: i64,
    pos: Vec2,
    orientation: f32,
) -> Option<(Entity, EntityId)> {
    let t = world.npc_templates.get(&npc_entry)?.clone();
    let id = world.alloc_id();
    let level = t.max_level.max(t.min_level).max(1) as u32;
    let stats = npc_stats(&t, level);
    let motion = Motion { pos, orientation, moving: false, dirty: false };
    let leash = if t.leash_range > 0 { t.leash_range as f32 } else { crate::world::DEFAULT_LEASH };
    let npc = Npc { entry: npc_entry };
    outbox.push(Scope::Map(map), ServerMsg::Spawn(entity_info(&NetId(id), &motion, &stats, Some(&npc), None, false)));
    let mut e = commands.spawn((
        NetId(id),
        OnMap(map),
        npc,
        Faction(t.faction),
        stats,
        motion,
        Home { pos, orientation, leash, respawn_secs: f32::MAX },
        Risen,
    ));
    let spells: Vec<_> = t.spells.iter().filter(|s| s.spell > 0).collect();
    if !spells.is_empty() {
        e.insert(NpcSpells(
            spells
                .iter()
                .map(|sp| NpcSpellState {
                    spell: sp.spell as u32,
                    chance: sp.chance.max(1) as f32,
                    interval: (sp.interval_ms.max(500) as f32) / 1000.0,
                    cooldown: sp.cooldown_ms.max(0) as f32 / 1000.0,
                    timer: (sp.interval_ms.max(500) as f32) / 1000.0,
                    cd: 0.0,
                    on_self: matches!(sp.target_type, 1 | 2),
                })
                .collect(),
        ));
        e.insert(Spellbook::new(spells.iter().map(|sp| sp.spell as u32).collect()));
    }
    let e = e.id();
    index.0.insert(id, e);
    Some((e, id))
}

fn say(outbox: &mut Outbox, map: i64, pos: Vec2, id: EntityId, text: &str) {
    outbox.push(Scope::Near(map, pos), ServerMsg::NpcSay { id, text: text.to_string() });
}

/// The Warden at the Glare Gate: a player on the quest walks into the `glare_gate` marker ->
/// Corvin and the Stooped rise, the Eye opens; Corvin falls -> credit, the Eye settles.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn encounters(
    mut commands: Commands,
    time: Res<Time>,
    mut director: ResMut<Director>,
    mut world: ResMut<GameWorld>,
    mut index: ResMut<NetIndex>,
    mut outbox: ResMut<Outbox>,
    mut eye: MessageWriter<EyeCommand>,
    mut players: Query<(Entity, &OnMap, &Motion, &mut Quester, Has<Dead>), (With<Player>, Without<Npc>)>,
    units: Query<(Entity, &NetId, &Npc, &OnMap, &Motion, &Stats, Has<Dead>, Has<Hidden>, Has<Risen>), Without<Player>>,
) {
    let maps: Vec<(i64, Marker)> = director
        .markers
        .iter()
        .filter_map(|(map, _)| Some((*map, director.marker(*map, "glare_gate")?.clone())))
        .collect();
    for (map, gate) in maps {
        let centre = Vec2::new(gate.x, gate.y);
        let near = |m: &Motion, r: f32| m.pos.distance(centre) <= r;
        // A living player on the quest, inside the gate.
        let trigger = players
            .iter()
            .find(|(_, on, m, q, dead)| {
                on.0 == map && !dead && q.progress.stage == script::Stage::Warden && near(m, gate.radius)
            })
            .map(|(e, ..)| e);
        let spot = director.marker(map, "corvin").map(|m| Vec2::new(m.x, m.y)).unwrap_or(centre);
        let state = director.encounters.entry(map).or_default();
        match state {
            Encounter::Dormant => {
                let Some(player) = trigger else { continue };
                if !world.npc_templates.contains_key(&entry::CORVIN) {
                    continue;
                }
                // Use a Corvin the map already placed, else raise one.
                let placed = units.iter().find(|u| {
                    u.2.entry == entry::CORVIN
                        && u.3.0 == map
                        && !u.6
                        && !u.7
                        && u.4.pos.distance(centre) < gate.radius * 2.0
                });
                let boss = match placed {
                    Some(u) => Some((u.0, u.1.0)),
                    None => raise(&mut commands, &mut world, &mut index, &mut outbox, entry::CORVIN, map, spot, 0.0),
                };
                let Some((boss, boss_id)) = boss else { continue };
                commands.entity(boss).insert(crate::ai::KeepsWounds);
                let mut risen = Vec::new();
                for i in 0..STOOPED_RISEN {
                    let a = i as f32 / STOOPED_RISEN as f32 * std::f32::consts::TAU + 0.4;
                    let want = centre + Vec2::from_angle(a) * (gate.radius * 0.7).max(2.0);
                    let pos = if world.is_walkable(map, want) {
                        want
                    } else {
                        let g = world.maps.get(&map).and_then(|m| m.grid.nearest_floor((want.x, want.y)));
                        g.map(|(x, y)| Vec2::new(x, y)).unwrap_or(want)
                    };
                    let facing = crate::ai::orientation(centre - pos);
                    if let Some((e, _)) =
                        raise(&mut commands, &mut world, &mut index, &mut outbox, entry::STOOPED, map, pos, facing)
                    {
                        commands.entity(e).insert(Attacking::new(player));
                        risen.push(e);
                    }
                }
                commands.entity(boss).insert(Attacking::new(player));
                say(&mut outbox, map, spot, boss_id, script::CORVIN_AGGRO);
                eye.write(EyeCommand::Open { map, secs: EYE_OPEN_SECS });
                outbox.push(Scope::Map(map), ServerMsg::BossBar { boss: Some(boss_id) });
                info!("director: the Glare Gate wakes on map {map}");
                *state = Encounter::Active { boss, risen, empty_for: 0.0 };
            }
            Encounter::Active { boss, risen, empty_for } => {
                let fallen = units.get(*boss).map_or(true, |u| u.5.hp <= 0 || u.6 || u.7);
                if fallen {
                    if let Ok(u) = units.get(*boss) {
                        say(&mut outbox, map, u.4.pos, u.1.0, script::CORVIN_DEATH);
                        commands.entity(*boss).insert(KeepCorpse);
                    }
                    for (pe, on, m, mut q, _) in &mut players {
                        if on.0 == map && near(m, gate.radius + FIGHT_MARGIN * 2.0) {
                            if let Some(info) = q.progress.on_warden_down() {
                                outbox.push(Scope::To(pe), ServerMsg::Quest(info));
                            }
                        }
                    }
                    eye.write(EyeCommand::Settle { map });
                    outbox.push(Scope::Map(map), ServerMsg::BossBar { boss: None });
                    info!("director: Corvin fell on map {map}");
                    *state = Encounter::Won;
                    continue;
                }
                let anyone = players.iter().any(|(_, on, m, q, dead)| {
                    on.0 == map
                        && !dead
                        && q.progress.stage == script::Stage::Warden
                        && near(m, gate.radius + FIGHT_MARGIN)
                });
                *empty_for = if anyone { 0.0 } else { *empty_for + time.delta_secs() };
                if *empty_for < RESET_SECS {
                    continue;
                }
                // Everyone left or died: put the gate back to sleep for the next attempt.
                for e in std::iter::once(*boss).chain(risen.iter().copied()) {
                    // A Corvin the map placed itself stays (it walks home on its own).
                    let Some(u) = units.get(e).ok().filter(|u| u.8) else { continue };
                    if !u.7 {
                        outbox.push(Scope::Map(map), ServerMsg::Despawn { id: u.1.0 });
                    }
                    index.0.remove(&u.1.0);
                    commands.entity(e).despawn();
                }
                eye.write(EyeCommand::Settle { map });
                outbox.push(Scope::Map(map), ServerMsg::BossBar { boss: None });
                info!("director: the Glare Gate sleeps again on map {map}");
                *state = Encounter::Dormant;
            }
            Encounter::Won => {
                // Latecomers on the quest find Corvin already down.
                for (pe, on, m, mut q, dead) in &mut players {
                    if on.0 == map && !dead && near(m, gate.radius) {
                        if let Some(info) = q.progress.on_warden_down() {
                            outbox.push(Scope::To(pe), ServerMsg::Quest(info));
                        }
                    }
                }
            }
        }
    }
}

/// Corvin's corpse never despawns.
fn keep_corpses(mut corpses: Query<&mut Dead, With<KeepCorpse>>) {
    for mut d in &mut corpses {
        d.timer = d.timer.max(3600.0);
    }
}

/// Risen Stooped whose corpses are gone leave the world for good.
fn cleanup_risen(
    mut commands: Commands,
    mut index: ResMut<NetIndex>,
    gone: Query<(Entity, &NetId), (With<Risen>, With<Hidden>)>,
) {
    for (e, id) in &gone {
        index.0.remove(&id.0);
        commands.entity(e).despawn();
    }
}

/// Villagers (and Ysolde) say something now and then when a player is around.
fn barks(
    time: Res<Time>,
    mut timers: Local<HashMap<Entity, f32>>,
    mut rng: Local<Rng>,
    mut outbox: ResMut<Outbox>,
    npcs: Query<(Entity, &NetId, &Npc, &OnMap, &Motion), (Without<Dead>, Without<Hidden>)>,
    players: Query<(&OnMap, &Motion), (With<Player>, Without<Dead>)>,
) {
    let dt = time.delta_secs();
    for (e, id, npc, map, m) in &npcs {
        let lines = script::bark_lines(npc.entry);
        if lines.is_empty() {
            continue;
        }
        let quiet = if npc.entry == entry::YSOLDE { 60.0 } else { 25.0 };
        let t = timers.entry(e).or_insert_with(|| rng.range(5.0, quiet));
        let heard = players.iter().any(|(pm, pmo)| pm == map && pmo.pos.distance(m.pos) <= BARK_RADIUS);
        if !heard {
            continue;
        }
        *t -= dt;
        if *t <= 0.0 {
            *t = rng.range(quiet, quiet * 2.0);
            let line = lines[(rng.next_f32() * lines.len() as f32) as usize % lines.len()];
            say(&mut outbox, map.0, m.pos, id.0, line);
        }
    }
}

/// "!" / "?" over Ysolde for each player.
fn sync_markers(
    mut outbox: ResMut<Outbox>,
    mut questers: Query<(Entity, &OnMap, &mut Quester), Changed<Quester>>,
    givers: Query<(&NetId, &Npc, &OnMap), Without<Quester>>,
) {
    for (pe, map, mut q) in &mut questers {
        let marker = q.progress.marker();
        if q.marker == Some(marker) {
            continue;
        }
        q.marker = Some(marker);
        for (id, npc, on) in &givers {
            if npc.entry == entry::YSOLDE && on == map {
                outbox.push(Scope::To(pe), ServerMsg::QuestMarker { npc: id.0, marker });
            }
        }
    }
}
