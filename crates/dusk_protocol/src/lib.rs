//! Client <-> server protocol. Our own design (not the original wire format):
//! `u32` little-endian length prefix + postcard-encoded message, over TCP.
//!
//! Positions are in map cell units, orientation is cell-space radians
//! (`atan2(dy, dx)`, same as `npc.orientation` in game.db).

pub mod net;

use serde::{Deserialize, Serialize};

/// Same port the original client used.
pub const DEFAULT_PORT: u16 = 16383;
/// Bump on any incompatible message change.
pub const PROTOCOL_VERSION: u32 = 5;
/// Frames above this are rejected (protects against garbage length prefixes).
pub const MAX_FRAME: usize = 1 << 20;

pub type EntityId = u64;
/// `spell_template.entry`
pub type SpellId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pos {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientMsg {
    /// `class`: `player_class_stats.Class` (1..=4).
    Hello {
        protocol: u32,
        name: String,
        class: u8,
    },
    /// Client-predicted movement; the server validates and rebroadcasts.
    Move {
        pos: Pos,
        orientation: f32,
        moving: bool,
    },
    Chat {
        text: String,
    },
    /// Start auto-attacking `target` (swings happen server-side when in range).
    Attack {
        target: EntityId,
    },
    StopAttack,
    /// `target`: unit for targeted spells; ignored by self/area spells.
    CastSpell {
        spell: SpellId,
        target: Option<EntityId>,
    },
    CancelCast,

    /// Equip the item in bag slot `bag_slot` (swaps with whatever is in its equipment slot).
    EquipItem {
        bag_slot: u8,
    },
    /// Move equipment slot `slot` back into the first free bag slot.
    UnequipItem {
        slot: u8,
    },
    /// Use (potions: cast `item_template.spell_1`) one item from a bag stack.
    UseItem {
        bag_slot: u8,
    },
    /// Destroy a bag stack.
    DestroyItem {
        bag_slot: u8,
    },
    /// Ask for the loot window of a corpse we may loot.
    OpenLoot {
        corpse: EntityId,
    },
    /// Take one loot entry (`Some(i)`: `items[i]` of the last `LootWindow`) or everything incl. gold.
    TakeLoot {
        corpse: EntityId,
        index: Option<u8>,
    },

    /// Talk to a friendly NPC (the server checks range; answers with `ServerMsg::Dialogue`).
    Interact {
        target: EntityId,
    },
    /// Pick `choices[index]` of the last `ServerMsg::Dialogue` shown by `speaker`.
    DialogueChoice {
        speaker: EntityId,
        index: u8,
    },
}

/// One item stack. Stats are derived from the template (+ affix) with `dusk_formats::item`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    /// `item_template.entry`
    pub entry: u32,
    /// `affix_template.entry`, 0 = none.
    pub affix: u32,
    pub count: u32,
}

/// Derived combat numbers of the receiver (Character window).
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct CombatStats {
    pub weapon_value: i32,
    pub melee_speed_ms: u32,
    pub armor: i32,
    pub melee_crit: i32,
    pub spell_crit: i32,
    pub dodge: i32,
    pub parry: i32,
    pub block: i32,
    /// Frost, fire, shadow, holy.
    pub resist: [i32; 4],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    Player {
        name: String,
    },
    /// `npc_template.entry`
    Npc {
        entry: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityInfo {
    pub id: EntityId,
    pub kind: EntityKind,
    pub pos: Pos,
    pub orientation: f32,
    pub moving: bool,
    pub level: u32,
    pub hp: i32,
    pub max_hp: i32,
    pub dead: bool,
}

/// Outcome of an attack; names/order follow the original client's enum (1..=8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HitResult {
    Hit,
    Miss,
    Resist,
    Evade,
    Dodge,
    Block,
    Parry,
    Crit,
    Immune,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerMsg {
    /// Sent after `Hello`; the client should (re)load `map` and place itself at `pos`.
    Welcome {
        your_id: EntityId,
        map: String,
        pos: Pos,
        orientation: f32,
    },
    Rejected {
        reason: String,
    },
    Spawn(EntityInfo),
    Despawn {
        id: EntityId,
    },
    Moved {
        id: EntityId,
        pos: Pos,
        orientation: f32,
        moving: bool,
    },
    /// Authoritative correction of the receiver's own position.
    Correct {
        pos: Pos,
    },
    Chat {
        from: String,
        text: String,
    },
    /// A melee swing landed (or not). `amount` is damage after mitigation.
    Swing {
        attacker: EntityId,
        target: EntityId,
        result: HitResult,
        amount: i32,
    },
    Health {
        id: EntityId,
        hp: i32,
        max_hp: i32,
    },
    Died {
        id: EntityId,
    },
    /// A dead unit came back (players after death; NPCs respawn via `Spawn`).
    Revive {
        id: EntityId,
        pos: Pos,
        hp: i32,
    },
    /// The receiver's own progression / resources / attributes (attributes feed tooltip formulas).
    PlayerStats {
        level: u32,
        xp: u32,
        xp_next: u32,
        mana: i32,
        max_mana: i32,
        attributes: Attributes,
    },
    /// The server cleared the receiver's attack target (target died, became invalid...).
    TargetLost,

    /// The receiver's spellbook.
    KnownSpells {
        spells: Vec<SpellId>,
    },
    CastStart {
        caster: EntityId,
        spell: SpellId,
        target: Option<EntityId>,
        cast_ms: u32,
    },
    /// Cast finished (`interrupted == false`) or was cancelled.
    CastEnd {
        caster: EntityId,
        spell: SpellId,
        interrupted: bool,
    },
    /// Spell released; effects land after `travel_ms` (projectiles).
    SpellGo {
        caster: EntityId,
        spell: SpellId,
        targets: Vec<EntityId>,
        travel_ms: u32,
    },
    /// Damage or healing from a spell or a periodic aura tick.
    SpellHit {
        caster: EntityId,
        target: EntityId,
        spell: SpellId,
        result: HitResult,
        amount: i32,
        heal: bool,
    },
    /// Receiver's cooldown on `spell` and the global cooldown, both in ms.
    Cooldown {
        spell: SpellId,
        ms: u32,
        gcd_ms: u32,
    },
    AuraApply {
        target: EntityId,
        spell: SpellId,
        caster: EntityId,
        duration_ms: u32,
        positive: bool,
    },
    AuraRemove {
        target: EntityId,
        spell: SpellId,
    },
    /// Why the receiver's last cast failed ("Out of range", "Not enough mana", ...).
    CastError {
        reason: String,
    },
    /// Movement restrictions on the receiver (snares, roots, stuns).
    ControlState {
        speed_pct: i32,
        rooted: bool,
        stunned: bool,
    },

    /// Full snapshot of the receiver's bags (`dusk_formats::item::BAG_SLOTS`), equipment
    /// (`EQUIP_SLOTS`, layout `dusk_formats::item::slot`) and gold.
    Inventory {
        bag: Vec<Option<Item>>,
        equipment: Vec<Option<Item>>,
        gold: u32,
    },
    /// Visible gear of a player: `item_template.entry` per equipment slot (0 = empty).
    /// Sent after `Spawn` and whenever it changes; drives the paper doll.
    Appearance {
        id: EntityId,
        gear: Vec<u32>,
    },
    CombatStats(CombatStats),
    /// A corpse the receiver may loot (`lootable == false`: emptied or gone).
    Lootable {
        id: EntityId,
        lootable: bool,
    },
    /// Contents of a corpse; empty (no items, no gold) means the window should close.
    LootWindow {
        corpse: EntityId,
        gold: u32,
        items: Vec<Item>,
    },
    /// Why an item/loot action failed ("Inventory is full", "Requires level 5"...).
    ItemError {
        reason: String,
    },
    /// The receiver got something (loot, quest reward...), for the chat log / floating text.
    Received {
        item: Option<Item>,
        gold: u32,
    },

    /// An NPC talks to the receiver. Empty `choices`: the client offers a plain "Goodbye".
    Dialogue {
        speaker: EntityId,
        text: String,
        choices: Vec<String>,
    },
    /// An NPC says something out loud (speech bubble + chat log).
    NpcSay {
        id: EntityId,
        text: String,
    },
    /// State of one of the receiver's quests (sent on accept, progress, ready and turn-in).
    Quest(QuestInfo),
    /// What the receiver should see over a quest giver's head.
    QuestMarker {
        npc: EntityId,
        marker: QuestMarker,
    },
    /// Show (`Some`) or drop the boss health bar for a scripted encounter.
    BossBar {
        boss: Option<EntityId>,
    },
    /// The demo run is over: time since joining and deaths, for the end card.
    DemoEnd {
        secs: u32,
        deaths: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QuestStatus {
    Active,
    /// Objective done; turn it in.
    Ready,
    Done,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuestInfo {
    pub id: u32,
    pub title: String,
    /// Tracker line, e.g. "Glarewolves" (shown as "Glarewolves 2/4" when `need > 0`).
    pub objective: String,
    pub count: u32,
    pub need: u32,
    pub status: QuestStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum QuestMarker {
    #[default]
    None,
    /// Has a quest for the receiver.
    Available,
    /// The receiver can turn a quest in here.
    TurnIn,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Attributes {
    pub strength: i32,
    pub agility: i32,
    pub willpower: i32,
    pub intelligence: i32,
    pub courage: i32,
}

pub fn encode<T: Serialize>(msg: &T) -> Vec<u8> {
    let body = postcard::to_stdvec(msg).expect("protocol messages always serialize");
    let mut frame = Vec::with_capacity(body.len() + 4);
    frame.extend((body.len() as u32).to_le_bytes());
    frame.extend(body);
    frame
}

pub fn decode<'a, T: Deserialize<'a>>(body: &'a [u8]) -> Result<T, postcard::Error> {
    postcard::from_bytes(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let msg = ServerMsg::Moved { id: 7, pos: Pos { x: 1.5, y: 2.0 }, orientation: 3.0, moving: true };
        let frame = encode(&msg);
        let len = u32::from_le_bytes(frame[..4].try_into().unwrap()) as usize;
        assert_eq!(len, frame.len() - 4);
        assert_eq!(decode::<ServerMsg>(&frame[4..]).unwrap(), msg);
    }
}
