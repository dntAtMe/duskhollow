//! Server connection: the server owns the world; this module mirrors it locally.
//! Offline play runs `dusk_server` embedded on a background thread and connects to it.

use crate::{
    combat_ui::{FloatKind, FloatingText},
    data::GameData,
    iso,
    items_ui::ItemNet,
    map_render::CurrentMap,
    player::{Player, PlayerMotion},
    unit::{self, Dead, Health, Level, Targeted, Unit},
};
use bevy::prelude::*;
use crossbeam_channel::TryRecvError;
use dusk_protocol::{
    ClientMsg, EntityId, EntityInfo, EntityKind, HitResult, PROTOCOL_VERSION, Pos, ServerMsg, net::ClientConnection,
};
use std::collections::HashMap;

/// How often the local player's position is sent while moving.
const SEND_INTERVAL: f32 = 0.1;
/// Remote units further than this from their target snap instead of sliding.
const SNAP_DISTANCE: f32 = 6.0;

pub struct NetPlugin {
    pub addr: String,
    pub name: String,
    pub class: u8,
}

impl Plugin for NetPlugin {
    fn build(&self, app: &mut App) {
        let conn = dusk_protocol::net::connect(&self.addr)
            .unwrap_or_else(|e| panic!("cannot connect to {}: {e} (is dusk_server running?)", self.addr));
        conn.send(ClientMsg::Hello { protocol: PROTOCOL_VERSION, name: self.name.clone(), class: self.class });
        info!("connected to {}", self.addr);
        app.insert_resource(Net { conn, my_id: None, entities: HashMap::new() })
            .insert_resource(PlayerState { name: self.name.clone(), speed_mult: 1.0, ..default() })
            .add_message::<SpellNet>()
            .add_message::<CombatNet>()
            .add_message::<DirectorNet>()
            .add_systems(Update, (receive, send_movement, interpolate_remote).chain());
    }
}

#[derive(Resource)]
pub struct Net {
    conn: ClientConnection,
    pub my_id: Option<EntityId>,
    /// Server entity id -> local entity (including our own player).
    pub entities: HashMap<EntityId, Entity>,
}

impl Net {
    pub fn send(&self, msg: ClientMsg) {
        self.conn.send(msg);
    }

    pub fn entity_id(&self, e: Entity) -> Option<EntityId> {
        self.entities.iter().find(|(_, v)| **v == e).map(|(k, _)| *k)
    }
}

/// The local player's progression and combat state (drives the HUD).
#[derive(Resource, Default)]
pub struct PlayerState {
    pub name: String,
    pub level: u32,
    pub xp: u32,
    pub xp_next: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub mana: i32,
    pub max_mana: i32,
    pub dead: bool,
    /// Auto-attack target (server id).
    pub target: Option<EntityId>,
    pub attributes: dusk_protocol::Attributes,
    /// Movement multiplier from snares (1.0 = normal).
    pub speed_mult: f32,
    pub rooted: bool,
    pub stunned: bool,
}

/// Spell-related and chat server messages, forwarded to `spells_ui` / `spell_fx` / `chat`.
#[derive(Message, Clone)]
pub struct SpellNet(pub ServerMsg);

/// Combat server messages (`Swing`, `Died`), forwarded for audio and other listeners.
#[derive(Message, Clone)]
pub struct CombatNet(pub ServerMsg);

/// Dialogue, quest, boss-bar and end-of-run messages, for `dialogue` / `director_ui`.
#[derive(Message, Clone)]
pub struct DirectorNet(pub ServerMsg);

/// Latest authoritative state of a remote unit; [`interpolate_remote`] eases toward it.
#[derive(Component)]
pub struct NetTarget {
    pos: Vec2,
    orientation: f32,
    moving: bool,
}

fn v(p: Pos) -> Vec2 {
    Vec2::new(p.x, p.y)
}

#[allow(clippy::too_many_arguments)]
fn receive(
    mut commands: Commands,
    mut net: ResMut<Net>,
    mut me: ResMut<PlayerState>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    mut map: ResMut<CurrentMap>,
    mut targets: Query<&mut NetTarget>,
    mut units: Query<(&mut Unit, &mut Health)>,
    roots: Query<Entity, (With<Unit>, Without<ChildOf>)>,
    mut exit: MessageWriter<AppExit>,
    mut spell_out: MessageWriter<SpellNet>,
    mut item_out: MessageWriter<ItemNet>,
    mut combat_out: MessageWriter<CombatNet>,
    mut director_out: MessageWriter<DirectorNet>,
    dead: Query<(), With<Dead>>,
) {
    loop {
        let msg = match net.conn.incoming.try_recv() {
            Ok(m) => m,
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                error!("disconnected from server");
                exit.write(AppExit::error());
                return;
            }
        };
        if matches!(msg, ServerMsg::Swing { .. } | ServerMsg::Died { .. }) {
            combat_out.write(CombatNet(msg.clone()));
        }
        match msg {
            ServerMsg::Welcome { your_id, map: map_name, pos, orientation } => {
                info!("welcome: id {your_id} on {map_name}");
                for e in &roots {
                    commands.entity(e).despawn();
                }
                net.entities.clear();
                net.my_id = Some(your_id);
                map.request(map_name);
                if let Some(e) = unit::spawn_paper_doll(&mut commands, &data, &assets, &me.name, v(pos), orientation) {
                    commands.entity(e).insert((Player, PlayerMotion { orientation, moving: false }));
                    net.entities.insert(your_id, e);
                }
            }
            ServerMsg::Rejected { reason } => {
                error!("server rejected us: {reason}");
                exit.write(AppExit::error());
            }
            ServerMsg::Spawn(info) => {
                if Some(info.id) == net.my_id {
                    // Our own spawn only carries our initial health.
                    (me.hp, me.max_hp) = (info.hp, info.max_hp);
                    if let Some(Ok((_, mut h))) = net.entities.get(&info.id).map(|e| units.get_mut(*e)) {
                        *h = Health { hp: info.hp, max: info.max_hp };
                    }
                    continue;
                }
                if let Some(old) = net.entities.remove(&info.id) {
                    commands.entity(old).despawn();
                }
                if let Some(e) = spawn_remote(&mut commands, &data, &assets, &info) {
                    net.entities.insert(info.id, e);
                }
            }
            ServerMsg::Despawn { id } => {
                if let Some(e) = net.entities.remove(&id) {
                    // Corpses fade out instead of popping.
                    crate::feel::despawn_unit(&mut commands, e, dead.contains(e));
                }
                if me.target == Some(id) {
                    me.target = None;
                }
            }
            ServerMsg::Moved { id, pos, orientation, moving } => {
                if let Some(mut t) = net.entities.get(&id).and_then(|e| targets.get_mut(*e).ok()) {
                    *t = NetTarget { pos: v(pos), orientation, moving };
                }
            }
            ServerMsg::Correct { pos } => {
                if let Some(Ok((mut u, _))) = net.my_id.and_then(|id| net.entities.get(&id)).map(|e| units.get_mut(*e))
                {
                    u.pos = v(pos);
                }
            }
            ServerMsg::Swing { attacker, target, result, amount } => {
                let att = net.entities.get(&attacker).copied();
                let tgt = net.entities.get(&target).copied();
                let tgt_pos = tgt.and_then(|t| units.get(t).ok()).map(|(u, _)| (u.pos, u.height * u.scale));
                if let Some(Ok((mut u, _))) = att.map(|a| units.get_mut(a)) {
                    if let Some((tp, _)) = tgt_pos {
                        u.dir = iso::direction_from_orientation(iso::orientation_of(tp - u.pos));
                    }
                    u.play_action("swing");
                }
                if let Some(Ok((mut u, _))) = tgt.map(|t| units.get_mut(t)) {
                    match result {
                        HitResult::Hit | HitResult::Crit if !u.is_acting() => u.play_action("hit"),
                        HitResult::Block | HitResult::Parry if !u.is_acting() => u.play_action("block"),
                        _ => {}
                    }
                }
                if let Some((pos, height)) = tgt_pos {
                    let kind = if Some(target) == net.my_id {
                        FloatKind::Incoming
                    } else if result == HitResult::Crit {
                        FloatKind::Crit
                    } else {
                        FloatKind::Outgoing
                    };
                    let text = match result {
                        HitResult::Hit | HitResult::Crit => amount.to_string(),
                        HitResult::Block | HitResult::Parry => format!("{amount} ({result:?})"),
                        other => format!("{other:?}"),
                    };
                    commands.spawn(FloatingText::bundle(text, kind, pos, height));
                }
            }
            ServerMsg::Health { id, hp, max_hp } => {
                if let Some(Ok((_, mut h))) = net.entities.get(&id).map(|e| units.get_mut(*e)) {
                    *h = Health { hp, max: max_hp };
                }
                if Some(id) == net.my_id {
                    me.hp = hp;
                    me.max_hp = max_hp;
                }
            }
            ServerMsg::Died { id } => {
                if let Some(&e) = net.entities.get(&id) {
                    if let Ok((mut u, _)) = units.get_mut(e) {
                        u.set_anim("die");
                    }
                    commands.entity(e).insert(Dead).remove::<Targeted>();
                }
                if Some(id) == net.my_id {
                    me.dead = true;
                    me.target = None;
                }
                if me.target == Some(id) {
                    me.target = None;
                }
            }
            ServerMsg::Revive { id, pos, hp } => {
                if let Some(&e) = net.entities.get(&id) {
                    if let Ok((mut u, mut h)) = units.get_mut(e) {
                        u.pos = v(pos);
                        u.set_anim("stance");
                        h.hp = hp;
                    }
                    commands.entity(e).remove::<Dead>();
                    if let Ok(mut t) = targets.get_mut(e) {
                        t.pos = v(pos);
                    }
                }
                if Some(id) == net.my_id {
                    me.dead = false;
                    me.hp = hp;
                }
            }
            ServerMsg::PlayerStats { level, xp, xp_next, mana, max_mana, attributes } => {
                if level > me.level && me.level > 0 {
                    info!("level up! now level {level}");
                }
                (me.level, me.xp, me.xp_next, me.mana, me.max_mana) = (level, xp, xp_next, mana, max_mana);
                me.attributes = attributes;
            }
            ServerMsg::TargetLost => me.target = None,
            ServerMsg::ControlState { speed_pct, rooted, stunned } => {
                (me.speed_mult, me.rooted, me.stunned) = (speed_pct as f32 / 100.0, rooted, stunned);
            }
            msg @ (ServerMsg::KnownSpells { .. }
            | ServerMsg::CastStart { .. }
            | ServerMsg::CastEnd { .. }
            | ServerMsg::SpellGo { .. }
            | ServerMsg::SpellHit { .. }
            | ServerMsg::Cooldown { .. }
            | ServerMsg::AuraApply { .. }
            | ServerMsg::AuraRemove { .. }
            | ServerMsg::CastError { .. }
            | ServerMsg::Chat { .. }
            | ServerMsg::NpcSay { .. }) => {
                spell_out.write(SpellNet(msg));
            }
            msg @ (ServerMsg::Inventory { .. }
            | ServerMsg::Appearance { .. }
            | ServerMsg::CombatStats(_)
            | ServerMsg::Lootable { .. }
            | ServerMsg::LootWindow { .. }
            | ServerMsg::ItemError { .. }
            | ServerMsg::Received { .. }) => {
                item_out.write(ItemNet(msg));
            }
            msg @ (ServerMsg::Dialogue { .. }
            | ServerMsg::Quest(_)
            | ServerMsg::QuestMarker { .. }
            | ServerMsg::BossBar { .. }
            | ServerMsg::DemoEnd { .. }) => {
                director_out.write(DirectorNet(msg));
            }
        }
    }
}

fn spawn_remote(commands: &mut Commands, data: &GameData, assets: &AssetServer, info: &EntityInfo) -> Option<Entity> {
    let pos = v(info.pos);
    let e = match &info.kind {
        EntityKind::Npc { entry } => unit::spawn_npc(commands, data, assets, *entry, pos, info.orientation)?,
        EntityKind::Player { name } => unit::spawn_paper_doll(commands, data, assets, name, pos, info.orientation)?,
    };
    commands.entity(e).insert((
        NetTarget { pos, orientation: info.orientation, moving: info.moving },
        Health { hp: info.hp, max: info.max_hp },
        Level(info.level),
    ));
    if info.dead {
        commands.entity(e).insert(Dead);
        commands.queue(move |world: &mut World| {
            if let Some(mut u) = world.get_mut::<Unit>(e) {
                u.set_dead_pose();
            }
        });
    }
    Some(e)
}

fn send_movement(
    time: Res<Time>,
    net: Res<Net>,
    mut since_send: Local<f32>,
    mut was_moving: Local<bool>,
    player: Query<(&Unit, &PlayerMotion), With<Player>>,
) {
    let Ok((unit, motion)) = player.single() else { return };
    *since_send += time.delta_secs();
    // Send periodically while moving, and once immediately when stopping or starting.
    if (motion.moving && *since_send >= SEND_INTERVAL) || motion.moving != *was_moving {
        net.send(ClientMsg::Move {
            pos: Pos { x: unit.pos.x, y: unit.pos.y },
            orientation: motion.orientation,
            moving: motion.moving,
        });
        *since_send = 0.0;
        *was_moving = motion.moving;
    }
}

fn interpolate_remote(time: Res<Time>, mut remotes: Query<(&NetTarget, &mut Unit, Has<Dead>)>) {
    // Exponential ease: converges within ~0.2s, smooth over 20Hz server updates.
    let k = 1.0 - (-time.delta_secs() * 15.0).exp();
    for (t, mut u, dead) in &mut remotes {
        let d = t.pos - u.pos;
        u.pos = if d.length() > SNAP_DISTANCE { t.pos } else { u.pos + d * k };
        if dead {
            continue;
        }
        if t.moving || !u.is_acting() {
            u.dir = iso::direction_from_orientation(t.orientation);
        }
        u.set_anim(if t.moving { "run" } else { "stance" });
    }
}
