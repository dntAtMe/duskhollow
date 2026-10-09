//! The "First Gaze" run as plain data (docs/demo-plan.md): quest stages, Ysolde's dialogue,
//! villager lines. No ECS here, so the whole flow is unit-testable.

use dusk_protocol::{QuestInfo, QuestMarker, QuestStatus};

/// Custom NPC entries (docs/demo-plan.md).
pub mod entry {
    pub const GLAREWOLF: i64 = 50001;
    pub const GLAREWOLF_ALPHA: i64 = 50002;
    pub const STOOPED: i64 = 50003;
    pub const CORVIN: i64 = 50004;
    pub const YSOLDE: i64 = 50010;
    pub const LIGHTWORKER: i64 = 50011;
    pub const GUARD: i64 = 50012;
}

pub const RED_FIELDS: u32 = 1;
pub const WARDEN: u32 = 2;
pub const WOLVES_NEEDED: u32 = 4;
/// Ember draughts: `item_template` 1 (Minor Life Potion), three of them.
pub const EMBER_DRAUGHT: u32 = 1;
pub const EMBER_DRAUGHTS: u32 = 3;
/// Gold for putting Corvin down.
pub const WARDEN_GOLD: u32 = 25;

/// Where a player is in the run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Stage {
    #[default]
    Start,
    /// The Red Fields taken.
    Wolves,
    /// The Red Fields turned in; the Warden quest was offered but not taken yet.
    WardenOffered,
    Warden,
    /// Everything turned in.
    End,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Progress {
    pub stage: Stage,
    pub wolves: u32,
    pub warden_down: bool,
}

/// Something the ECS side has to act on after a [`Progress`] change.
#[derive(Clone, Debug, PartialEq)]
pub enum Outcome {
    Quest(QuestInfo),
    /// Hand out the reward of this quest.
    Reward(u32),
    /// The run is over (end card).
    End,
}

impl Progress {
    /// `DUSK_QUEST_TEST` values: `start`, `wolves[:N]`, `wolves_ready`, `warden_offered`, `warden`,
    /// `warden_ready`, `end`.
    pub fn parse_test(s: &str) -> Option<Self> {
        let (name, n) = s.trim().split_once(':').unwrap_or((s.trim(), ""));
        let p = |stage, wolves, warden_down| Some(Self { stage, wolves, warden_down });
        match name {
            "start" => p(Stage::Start, 0, false),
            "wolves" => p(Stage::Wolves, n.parse::<u32>().unwrap_or(0).min(WOLVES_NEEDED), false),
            "wolves_ready" => p(Stage::Wolves, WOLVES_NEEDED, false),
            "warden_offered" => p(Stage::WardenOffered, WOLVES_NEEDED, false),
            "warden" => p(Stage::Warden, WOLVES_NEEDED, false),
            "warden_ready" => p(Stage::Warden, WOLVES_NEEDED, true),
            "end" => p(Stage::End, WOLVES_NEEDED, true),
            _ => None,
        }
    }

    pub fn wolves_ready(&self) -> bool {
        self.stage == Stage::Wolves && self.wolves >= WOLVES_NEEDED
    }

    pub fn warden_ready(&self) -> bool {
        self.stage == Stage::Warden && self.warden_down
    }

    /// Ysolde's head marker for this player.
    pub fn marker(&self) -> QuestMarker {
        match self.stage {
            Stage::Start | Stage::WardenOffered => QuestMarker::Available,
            Stage::Wolves if self.wolves_ready() => QuestMarker::TurnIn,
            Stage::Warden if self.warden_ready() => QuestMarker::TurnIn,
            _ => QuestMarker::None,
        }
    }

    pub fn quest_info(&self, quest: u32) -> QuestInfo {
        let (title, objective, count, need, status) = match quest {
            RED_FIELDS => {
                let status = match self.stage {
                    Stage::Start => QuestStatus::Active,
                    Stage::Wolves if self.wolves_ready() => QuestStatus::Ready,
                    Stage::Wolves => QuestStatus::Active,
                    _ => QuestStatus::Done,
                };
                let (objective, count, need) = match status {
                    QuestStatus::Ready => ("Return to Ysolde", 0, 0),
                    _ => ("Glarewolves slain", self.wolves.min(WOLVES_NEEDED), WOLVES_NEEDED),
                };
                ("The Red Fields", objective, count, need, status)
            }
            _ => {
                let status = match self.stage {
                    Stage::End => QuestStatus::Done,
                    _ if self.warden_down => QuestStatus::Ready,
                    _ => QuestStatus::Active,
                };
                let (objective, count, need) = match status {
                    QuestStatus::Ready => ("Return to Ysolde", 0, 0),
                    _ => ("Hollowed Warden Corvin", 0, 1),
                };
                ("The Warden at the Glare Gate", objective, count, need, status)
            }
        };
        let journal = journal(quest);
        QuestInfo {
            id: quest,
            title: title.into(),
            objective: objective.into(),
            count,
            need,
            status,
            description: match status {
                QuestStatus::Active => journal.active,
                QuestStatus::Ready => journal.ready,
                QuestStatus::Done => journal.done,
            }
            .into(),
            giver: journal.giver.into(),
            location: match status {
                QuestStatus::Active => journal.location,
                _ => journal.turn_in,
            }
            .into(),
            reward: journal.reward.into(),
        }
    }

    /// Quests the journal and tracker should know about right now (on join), completed ones
    /// included.
    pub fn open_quests(&self) -> Vec<QuestInfo> {
        match self.stage {
            Stage::Start => vec![],
            Stage::Wolves => vec![self.quest_info(RED_FIELDS)],
            Stage::WardenOffered => vec![self.quest_info(RED_FIELDS)],
            Stage::Warden | Stage::End => vec![self.quest_info(RED_FIELDS), self.quest_info(WARDEN)],
        }
    }

    /// A kill by this player; returns the tracker update if it counted.
    pub fn on_kill(&mut self, npc_entry: i64) -> Option<QuestInfo> {
        let wolf = matches!(npc_entry, entry::GLAREWOLF | entry::GLAREWOLF_ALPHA);
        if !wolf || self.stage != Stage::Wolves || self.wolves >= WOLVES_NEEDED {
            return None;
        }
        self.wolves += 1;
        Some(self.quest_info(RED_FIELDS))
    }

    /// Corvin fell (credit goes to everyone on the quest nearby).
    pub fn on_warden_down(&mut self) -> Option<QuestInfo> {
        if self.stage != Stage::Warden || self.warden_down {
            return None;
        }
        self.warden_down = true;
        Some(self.quest_info(WARDEN))
    }

    /// Applies a dialogue action. Invalid ones (wrong stage) do nothing.
    pub fn apply(&mut self, action: Action) -> Vec<Outcome> {
        match action {
            Action::Accept(RED_FIELDS) if self.stage == Stage::Start => {
                self.stage = Stage::Wolves;
                self.wolves = 0;
                vec![Outcome::Quest(self.quest_info(RED_FIELDS))]
            }
            Action::TurnIn(RED_FIELDS) if self.wolves_ready() => {
                self.stage = Stage::WardenOffered;
                vec![Outcome::Quest(self.quest_info(RED_FIELDS)), Outcome::Reward(RED_FIELDS)]
            }
            Action::Accept(WARDEN) if self.stage == Stage::WardenOffered => {
                self.stage = Stage::Warden;
                vec![Outcome::Quest(self.quest_info(WARDEN))]
            }
            Action::TurnIn(WARDEN) if self.warden_ready() => {
                self.stage = Stage::End;
                vec![Outcome::Quest(self.quest_info(WARDEN)), Outcome::Reward(WARDEN), Outcome::End]
            }
            _ => vec![],
        }
    }
}

// ---------------------------------------------------------------- journal

/// Quest journal text (client `journal.rs`), one entry per quest.
pub struct Journal {
    pub giver: &'static str,
    /// Where the work is.
    pub location: &'static str,
    /// Where to turn it in.
    pub turn_in: &'static str,
    pub reward: &'static str,
    /// Log text per status.
    pub active: &'static str,
    pub ready: &'static str,
    pub done: &'static str,
}

pub fn journal(quest: u32) -> Journal {
    match quest {
        RED_FIELDS => Journal {
            giver: "Ysolde, the cairnkeeper",
            location: "The Red Fields, east of Lowshade",
            turn_in: "Ysolde, by the cairn in Lowshade",
            reward: "3 Ember Draughts",
            active: "Glarewolves came down off the open ground, and the lightworkers can't cut grain with \
                     wolves at their backs. Kill four of them in the Red Fields.\n\nFight under the canopy \
                     shelters where you can: under a tarp the Eye only half finds you.",
            ready: "Four glarewolves lie dead in the rows. Ysolde will want to hear it. She keeps the cairn \
                    in Lowshade, under the overhang.",
            done: "The rows get cut today. Ysolde paid in ember draughts, brewed over the cairn fire. \
                   Drink one when your head starts to ring.",
        },
        _ => Journal {
            giver: "Ysolde, the cairnkeeper",
            location: "The Glare Gate, north mouth of the vale",
            turn_in: "Ysolde, by the cairn in Lowshade",
            reward: "25 Gold Pieces",
            active: "Warden Corvin stood at the Glare Gate for thirty-one years. Since the Eye opened wide he \
                     hasn't come down, and the two who went up to look haven't either.\n\nGo north to the \
                     gate. If he isn't Corvin any more, put him down.",
            ready: "Corvin is relieved of his post, and the Eye has settled back to half-lidded. Return to \
                    Ysolde in Lowshade.",
            done: "Thirty-one years, and he still held the gate. Ysolde says to sit by the fire anyway. \
                   Nobody rests. Sit anyway.",
        },
    }
}

// ---------------------------------------------------------------- dialogue

/// A page of Ysolde's dialogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Node {
    Greeting,
    Where,
    WolfOffer,
    WolvesTaken,
    WolvesWaiting,
    Canopies,
    WolvesReady,
    WolvesThanks,
    WardenOffer,
    WardenTaken,
    WardenWaiting,
    AboutCorvin,
    WardenReady,
    Farewell,
    After,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Goto(Node),
    Accept(u32),
    /// Turn in, then show the node.
    TurnIn(u32),
    Close,
}

pub struct Page {
    pub text: &'static str,
    pub choices: Vec<(&'static str, Action)>,
}

/// The page Ysolde opens with for this player.
pub fn ysolde_entry(p: &Progress) -> Node {
    match p.stage {
        Stage::Start => Node::Greeting,
        Stage::Wolves if p.wolves_ready() => Node::WolvesReady,
        Stage::Wolves => Node::WolvesWaiting,
        Stage::WardenOffered => Node::WardenOffer,
        Stage::Warden if p.warden_ready() => Node::WardenReady,
        Stage::Warden => Node::WardenWaiting,
        Stage::End => Node::After,
    }
}

/// Node shown after a turn-in.
pub fn after_turn_in(quest: u32) -> Node {
    if quest == RED_FIELDS { Node::WolvesThanks } else { Node::Farewell }
}

pub fn ysolde(node: Node) -> Page {
    use Action::*;
    let page = |text, choices| Page { text, choices };
    match node {
        Node::Greeting => page(
            "Another sword under the Eye. Few walk in from the gorge any more.\n\nCome in out of the red. Fire's lit.",
            vec![
                ("Where am I?", Goto(Node::Where)),
                ("You look like you need a sword.", Goto(Node::WolfOffer)),
                ("Goodbye.", Close),
            ],
        ),
        Node::Where => page(
            "Lowshade. The cliff keeps the worst of it off us. Stand by the cairn if your head starts ringing. \
             Fire blinds the Watcher. Doesn't make it stop looking.",
            vec![("You look like you need a sword.", Goto(Node::WolfOffer)), ("Goodbye.", Close)],
        ),
        Node::WolfOffer => page(
            "The Red Fields, east of here. Glarewolves came down off the open ground three days back. \
             Eye-turned. All teeth, no fear.\n\nMy lightworkers can't cut grain with wolves at their backs. \
             Kill four. Pull them under the canopies if you can.",
            vec![("I'll see to the wolves.", Accept(RED_FIELDS)), ("Not yet.", Close)],
        ),
        Node::WolvesTaken => page(
            "Mind the open rows. Ember draughts when you're back.",
            vec![("Why the canopies?", Goto(Node::Canopies)), ("Going.", Close)],
        ),
        Node::WolvesWaiting => page(
            "Wolves are still out there. I can tell. Nobody's singing in the rows.",
            vec![("Why the canopies?", Goto(Node::Canopies)), ("On my way.", Close)],
        ),
        Node::Canopies => page(
            "Under a tarp the Eye only half finds you. Fight in the open and you'll feel it climb up your neck. \
             Everyone does. Even swords.",
            vec![("On my way.", Close)],
        ),
        Node::WolvesReady => page(
            "You smell of wolf. Good.",
            vec![("Four of them. It's done.", TurnIn(RED_FIELDS)), ("Not yet.", Close)],
        ),
        Node::WolvesThanks => page(
            "Then the rows get cut today. Take these. Ember draughts, brewed over the cairn. \
             Drink when your head starts to ring.\n\nThere's one more thing. If you've the stomach.",
            vec![("Go on.", Goto(Node::WardenOffer)), ("Later.", Close)],
        ),
        Node::WardenOffer => page(
            "North, at the Glare Gate. Warden Corvin. Thirty-one years at that post. \
             Brought me salt every winter.\n\nSince the Eye opened wide he hasn't come down. \
             The two who went up to look haven't either. Go and see. If he isn't Corvin any more, put him down.",
            vec![("I'll go to the gate.", Accept(WARDEN)), ("Not yet.", Close)],
        ),
        Node::WardenTaken => page(
            "North mouth of the vale. Open sky the whole way. Don't stop to look up.",
            vec![("Tell me about Corvin.", Goto(Node::AboutCorvin)), ("Going.", Close)],
        ),
        Node::WardenWaiting => page(
            "The gate's north. Corvin's there. Was there. Whatever's there now.",
            vec![("Tell me about Corvin.", Goto(Node::AboutCorvin)), ("Going.", Close)],
        ),
        Node::AboutCorvin => page(
            "Stubborn. Kind, when nobody watched. Said the gate was the one place the Eye couldn't make him leave.\n\n\
             Seems he was right.",
            vec![("Going.", Close)],
        ),
        Node::WardenReady => page(
            "You came back. That's more than the last two did.",
            vec![("Corvin's at rest.", TurnIn(WARDEN)), ("Not yet.", Close)],
        ),
        Node::Farewell => page(
            "Relieved of post, then. Thirty-one years, and he still held the gate.\n\n\
             Feel that? It's settled. Half-lidded, same as always. Sit by the fire, sword. \
             You won't rest. Nobody does. Sit anyway.",
            vec![("Farewell, Ysolde.", Close)],
        ),
        Node::After => page("Still here? Good. Fire needs watching, and so do you.", vec![("Goodbye.", Close)]),
    }
}

/// The node an accepted quest moves to.
pub fn after_accept(quest: u32) -> Node {
    if quest == RED_FIELDS { Node::WolvesTaken } else { Node::WardenTaken }
}

// ---------------------------------------------------------------- villagers

/// What villagers answer when talked to.
pub fn talk_lines(npc_entry: i64) -> &'static [&'static str] {
    match npc_entry {
        entry::LIGHTWORKER => &[
            "Can't talk. Grain won't cut itself, Eye or no Eye.",
            "Keep your head down, sword. It sees the proud first.",
            "Wolves took Bren's boy last week. Took his sickle too, somehow.",
            "Under the tarp. Always under the tarp.",
            "My eyes? Red, same as everyone's. Don't stare.",
        ],
        entry::GUARD => &[
            "Hamlet's under the rock. Stay under the rock and you keep your wits.",
            "Nine years at this post. Corvin did thirty-one. Don't ask me how.",
            "Swords walk up the gorge. Few walk back down.",
        ],
        _ => &["They look through you, at the sky."],
    }
}

/// What NPCs say on their own now and then (speech bubbles).
pub fn bark_lines(npc_entry: i64) -> &'static [&'static str] {
    match npc_entry {
        entry::LIGHTWORKER => &[
            "Row's done. Next row.",
            "Don't look up. Don't look up.",
            "Who moved my canopy pole?",
            "Hot today. Same as yesterday.",
            "Bread tonight, if the wolves let us.",
            "Feel that? It's looking this way.",
        ],
        entry::GUARD => &[
            "Eyes on the fields.",
            "Shift change soon. Not soon enough.",
            "Quiet. Or just quiet. Can't tell any more.",
            "Nobody walks the open rows after the bell.",
        ],
        entry::YSOLDE => &["Feed the fire. It remembers who sat by it.", "Come in out of the red."],
        _ => &[],
    }
}

/// Corvin when the gate wakes, and when he falls.
pub const CORVIN_AGGRO: &str = "Post's held. Thirty-one years. Turn back.";
pub const CORVIN_DEATH: &str = "Relieved... of post.";

#[cfg(test)]
mod tests {
    use super::*;

    /// Follows the dialogue tree like a player would: the full run, start to end card.
    #[test]
    fn whole_run() {
        let mut p = Progress::default();
        assert_eq!(p.marker(), QuestMarker::Available);
        assert_eq!(ysolde_entry(&p), Node::Greeting);
        let offer = ysolde(Node::Greeting).choices[1].1;
        assert_eq!(offer, Action::Goto(Node::WolfOffer));
        let accept = ysolde(Node::WolfOffer).choices[0].1;
        let out = p.apply(accept);
        assert!(matches!(&out[..], [Outcome::Quest(q)] if q.id == RED_FIELDS && q.need == 4));
        assert_eq!(p.marker(), QuestMarker::None);
        // Turning in early does nothing.
        assert!(p.apply(Action::TurnIn(RED_FIELDS)).is_empty());

        assert_eq!(p.on_kill(entry::STOOPED), None);
        for i in 1..=4 {
            let q = p.on_kill(if i == 2 { entry::GLAREWOLF_ALPHA } else { entry::GLAREWOLF }).unwrap();
            assert_eq!(q.status, if i == 4 { QuestStatus::Ready } else { QuestStatus::Active });
        }
        assert_eq!(p.on_kill(entry::GLAREWOLF), None, "no counting past the goal");
        assert_eq!(p.marker(), QuestMarker::TurnIn);
        assert_eq!(ysolde_entry(&p), Node::WolvesReady);

        let out = p.apply(ysolde(Node::WolvesReady).choices[0].1);
        assert_eq!(out[1], Outcome::Reward(RED_FIELDS));
        assert_eq!(p.stage, Stage::WardenOffered);
        assert_eq!(p.marker(), QuestMarker::Available);
        assert_eq!(ysolde(after_turn_in(RED_FIELDS)).choices[0].1, Action::Goto(Node::WardenOffer));
        p.apply(ysolde(Node::WardenOffer).choices[0].1);
        assert_eq!(p.stage, Stage::Warden);
        assert_eq!(p.open_quests().last().unwrap().title, "The Warden at the Glare Gate");
        assert_eq!(p.open_quests()[0].status, QuestStatus::Done);

        assert!(p.on_warden_down().is_some());
        assert!(p.on_warden_down().is_none());
        let out = p.apply(ysolde(ysolde_entry(&p)).choices[0].1);
        assert_eq!(out.last(), Some(&Outcome::End));
        assert_eq!(p.stage, Stage::End);
        assert_eq!(ysolde_entry(&p), Node::After);
    }

    #[test]
    fn test_stages_parse() {
        assert_eq!(Progress::parse_test("wolves:3").unwrap().wolves, 3);
        assert!(Progress::parse_test("warden_ready").unwrap().warden_ready());
        assert!(Progress::parse_test("nonsense").is_none());
    }

    #[test]
    fn every_page_has_a_way_out() {
        use Node::*;
        for n in [
            Greeting,
            Where,
            WolfOffer,
            WolvesTaken,
            WolvesWaiting,
            Canopies,
            WolvesReady,
            WolvesThanks,
            WardenOffer,
            WardenTaken,
            WardenWaiting,
            AboutCorvin,
            WardenReady,
            Farewell,
            After,
        ] {
            let page = ysolde(n);
            assert!(!page.choices.is_empty() && page.choices.len() <= 4, "{n:?}");
            assert!(page.text.len() < 600, "{n:?} is a wall of text");
        }
    }
}
