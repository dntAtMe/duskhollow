//! Connection handling and replication to clients.

use crate::combat::{Attacking, player_stats_msg};
use crate::items::ItemRequests;
use crate::spells::{CastRequest, CastRequests, Casting, Spellbook};
use crate::stats::{Stats, player_stats};
use crate::world::{
    Dead, Faction, GameWorld, Hidden, Motion, NetId, NetIndex, Npc, OnMap, Outbox, Player, Scope, entity_info, to_pos,
};
use bevy::prelude::*;
use crossbeam_channel::{Receiver, TryRecvError};
use dusk_formats::db::faction;
use dusk_protocol::{ClientMsg, PROTOCOL_VERSION, ServerMsg, net::ServerConnection};

/// Players may move this much faster than the client's run speed before being corrected.
const MAX_SPEED: f32 = 4.0 * 1.5;
/// Absolute slack (cells) for jitter in packet timing.
const MOVE_SLACK: f32 = 0.75;
const MAX_NAME: usize = 15;
/// Movement/combat updates are only sent for entities this close (cells) to the receiving player.
/// A 1280x720 view spans roughly 30 cells; the margin keeps entering units smooth.
const INTEREST_RADIUS: f32 = 45.0;

#[derive(Resource)]
pub struct Acceptor(pub Receiver<ServerConnection>);

/// Connections that have not said `Hello` yet.
#[derive(Default)]
pub struct Pending(Vec<ServerConnection>);

#[derive(Component)]
pub struct Client {
    pub conn: ServerConnection,
    /// Seconds since startup at the last accepted `Move`.
    last_move: f32,
}

type Visible<'a> = (&'a NetId, &'a OnMap, &'a Motion, &'a Stats, Option<&'a Npc>, Option<&'a Player>, Has<Dead>);

pub fn accept(
    acceptor: Res<Acceptor>,
    mut pending: Local<Pending>,
    mut commands: Commands,
    mut world: ResMut<GameWorld>,
    mut index: ResMut<NetIndex>,
    mut outbox: ResMut<Outbox>,
    time: Res<Time>,
    visible: Query<Visible, Without<Hidden>>,
) {
    pending.0.extend(acceptor.0.try_iter().inspect(|c| info!("connection from {}", c.peer)));

    let mut i = 0;
    while i < pending.0.len() {
        let msg = pending.0[i].incoming.try_recv();
        match msg {
            Err(TryRecvError::Empty) => i += 1,
            Err(TryRecvError::Disconnected) => {
                pending.0.swap_remove(i);
            }
            Ok(ClientMsg::Hello { protocol, name, class }) => {
                let conn = pending.0.swap_remove(i);
                let name: String = name.trim().chars().filter(|c| c.is_alphanumeric()).take(MAX_NAME).collect();
                let class = class as i64;
                let Some(cs) = world.class_stats(class, 1).copied() else {
                    conn.send(ServerMsg::Rejected { reason: format!("unknown class {class}") });
                    continue;
                };
                if protocol != PROTOCOL_VERSION || name.is_empty() {
                    conn.send(ServerMsg::Rejected {
                        reason: format!("bad hello (protocol {protocol}, expected {PROTOCOL_VERSION})"),
                    });
                    continue;
                }
                let (map, pos) = world.start;
                let Some(map_name) = world.maps.get(&map).map(|m| m.name.clone()) else { continue };
                let id = world.alloc_id();
                let motion = Motion { pos, orientation: std::f32::consts::FRAC_PI_4, moving: false, dirty: false };
                // Gear is applied by `items::init_inventories` once the entity exists.
                let stats = player_stats(&cs, &Default::default());
                let player = Player { name: name.clone(), class, xp: 0 };
                let known: Vec<u32> =
                    world.class_spells.get(&class).map(|v| v.iter().map(|s| *s as u32).collect()).unwrap_or_default();

                conn.send(ServerMsg::Welcome {
                    your_id: id,
                    map: map_name.clone(),
                    pos: to_pos(pos),
                    orientation: motion.orientation,
                });
                conn.send(player_stats_msg(&world, &player, &stats));
                conn.send(ServerMsg::KnownSpells { spells: known.clone() });
                for (nid, on, m, s, npc, p, dead) in &visible {
                    if on.0 == map {
                        conn.send(ServerMsg::Spawn(entity_info(nid, m, s, npc, p, dead)));
                    }
                }
                let me = entity_info(&NetId(id), &motion, &stats, None, Some(&player), false);
                outbox.push(Scope::Map(map), ServerMsg::Spawn(me));
                info!("{name} ({}) joined {map_name} as #{id} (class {class})", conn.peer);
                let e = commands
                    .spawn((
                        NetId(id),
                        OnMap(map),
                        motion,
                        stats,
                        player,
                        Faction(faction::PLAYER_DEFAULT),
                        Spellbook::new(known),
                        Client { conn, last_move: time.elapsed_secs() },
                    ))
                    .id();
                index.0.insert(id, e);
            }
            Ok(_) => {
                // Anything before Hello is a protocol violation.
                pending.0.swap_remove(i);
            }
        }
    }
}

pub fn handle_players(
    mut commands: Commands,
    time: Res<Time>,
    world: Res<GameWorld>,
    mut index: ResMut<NetIndex>,
    mut outbox: ResMut<Outbox>,
    mut casts: ResMut<CastRequests>,
    mut item_requests: ResMut<ItemRequests>,
    mut players: Query<(Entity, &NetId, &OnMap, &Player, &mut Client, &mut Motion, Has<Dead>, Option<&Casting>)>,
    targets: Query<(&OnMap, Option<&Faction>, Has<Npc>), (Without<Dead>, Without<Hidden>)>,
) {
    let now = time.elapsed_secs();
    for (entity, id, map, player, mut client, mut motion, dead, casting) in &mut players {
        loop {
            match client.conn.incoming.try_recv() {
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    info!("{} left", player.name);
                    outbox.push(Scope::Map(map.0), ServerMsg::Despawn { id: id.0 });
                    index.0.remove(&id.0);
                    commands.entity(entity).despawn();
                    break;
                }
                Ok(ClientMsg::Move { .. }) if dead => {
                    client.conn.send(ServerMsg::Correct { pos: to_pos(motion.pos) });
                }
                Ok(ClientMsg::Move { pos, orientation, moving }) => {
                    let target = Vec2::new(pos.x, pos.y);
                    let budget = MAX_SPEED * (now - client.last_move) + MOVE_SLACK;
                    if !target.is_finite() || target.distance(motion.pos) > budget || !world.is_walkable(map.0, target)
                    {
                        client.conn.send(ServerMsg::Correct { pos: to_pos(motion.pos) });
                    } else {
                        motion.pos = target;
                    }
                    client.last_move = now;
                    motion.orientation = if orientation.is_finite() { orientation } else { 0.0 };
                    motion.moving = moving;
                    motion.dirty = true;
                }
                Ok(ClientMsg::Attack { target }) => {
                    let valid = index.0.get(&target).copied().filter(|t| {
                        *t != entity
                            && targets.get(*t).is_ok_and(|(tm, f, is_npc)| {
                                // Only NPCs that are not friendly can be attacked (no PvP yet).
                                tm == map && is_npc && f.is_none_or(|f| f.0 != faction::FRIENDLY)
                            })
                    });
                    match valid {
                        Some(t) if !dead => {
                            commands.entity(entity).insert(Attacking::new(t));
                        }
                        _ => client.conn.send(ServerMsg::TargetLost),
                    }
                }
                Ok(ClientMsg::StopAttack) => {
                    commands.entity(entity).remove::<Attacking>();
                }
                Ok(ClientMsg::CastSpell { spell, target }) => {
                    let target = target.and_then(|t| index.0.get(&t).copied());
                    casts.0.push(CastRequest { caster: entity, spell, target, from_item: false });
                }
                Ok(ClientMsg::CancelCast) => {
                    if let Some(c) = casting {
                        commands.entity(entity).remove::<Casting>();
                        outbox.push(
                            Scope::Near(map.0, motion.pos),
                            ServerMsg::CastEnd { caster: id.0, spell: c.spell, interrupted: true },
                        );
                    }
                }
                Ok(ClientMsg::Chat { text }) => {
                    let text: String = text.chars().take(255).collect();
                    if !text.trim().is_empty() {
                        outbox.push(Scope::Map(map.0), ServerMsg::Chat { from: player.name.clone(), text });
                    }
                }
                Ok(ClientMsg::Hello { .. }) => {}
                Ok(
                    msg @ (ClientMsg::EquipItem { .. }
                    | ClientMsg::UnequipItem { .. }
                    | ClientMsg::UseItem { .. }
                    | ClientMsg::DestroyItem { .. }
                    | ClientMsg::OpenLoot { .. }
                    | ClientMsg::TakeLoot { .. }),
                ) => item_requests.0.push((entity, msg)),
            }
        }
    }
}

pub fn broadcast_motion(mut movers: Query<(&NetId, &OnMap, &mut Motion)>, mut outbox: ResMut<Outbox>) {
    for (id, map, mut m) in &mut movers {
        if m.dirty {
            m.dirty = false;
            let msg = ServerMsg::Moved { id: id.0, pos: to_pos(m.pos), orientation: m.orientation, moving: m.moving };
            outbox.push(Scope::Near(map.0, m.pos), msg);
        }
    }
}

/// Delivers everything queued in the [`Outbox`] this tick.
pub fn flush_outbox(mut outbox: ResMut<Outbox>, clients: Query<(Entity, &NetId, &OnMap, &Motion, &Client)>) {
    for (scope, msg) in outbox.0.drain(..) {
        match scope {
            Scope::To(e) => {
                if let Ok((.., c)) = clients.get(e) {
                    c.conn.send(msg);
                }
            }
            Scope::Map(map) => {
                for (.., on, _, c) in &clients {
                    if on.0 == map {
                        c.conn.send(msg.clone());
                    }
                }
            }
            Scope::Near(map, pos) => {
                // Don't echo a player's own movement back to them.
                let own = match &msg {
                    ServerMsg::Moved { id, .. } => Some(*id),
                    _ => None,
                };
                for (_, id, on, m, c) in &clients {
                    if on.0 == map
                        && Some(id.0) != own
                        && m.pos.distance_squared(pos) <= INTEREST_RADIUS * INTEREST_RADIUS
                    {
                        c.conn.send(msg.clone());
                    }
                }
            }
        }
    }
}
