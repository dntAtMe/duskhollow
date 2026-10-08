//! The Eye's gaze (docs/demo-plan.md): per-cell cover from `maps/<name>.cover`, per-player
//! strain and corruption, the Eye's lid per map, the wandering gaze spot and rest cairns.
//!
//! Maps without a `.cover` sidecar have no gaze at all. Effects reach the rest of the server
//! through [`GazeMods`] (regen, damage, speed multipliers) and [`Bound`] (respawn point).
//!
//! Director API: [`Gaze::force_open`] / [`Gaze::settle`] (e.g. the Glare Gate boss fight).

use crate::ai::Rng;
use crate::combat::CombatClock;
use crate::stats::Stats;
use crate::world::{Dead, Faction, GameWorld, Hidden, Motion, NetId, Npc, OnMap, Outbox, Player, Scope, to_pos};
use bevy::prelude::*;
use dusk_formats::custom::{Cover, CoverGrid};
use dusk_formats::db::faction;
use dusk_protocol::{EyeState, GazeCover, ServerMsg};
use std::collections::HashMap;
use std::path::Path;

/// Strain per second by cover (before multipliers).
const RATE_OPEN: f32 = 1.0;
const RATE_SHADE: f32 = 0.25;
const RATE_SHELTER: f32 = -5.0;
const RATE_CAIRN: f32 = -15.0;
/// Gains while in combat (dealt or took damage within [`COMBAT_SECS`]).
const COMBAT_MULT: f32 = 4.0;
const COMBAT_SECS: f32 = 5.0;
/// Gains while the Eye is wide open (scaled by openness during Opening/Closing).
const EYE_OPEN_MULT: f32 = 2.0;
/// Gains inside the wandering gaze spot.
const SPOT_MULT: f32 = 3.0;
/// Ender resistance: gains x (1 - min(WIL x 0.5 %, 40 %)).
const WIL_RESIST: f32 = 0.005;
const WIL_RESIST_MAX: f32 = 0.4;

pub const STRAIN_MAX: f32 = 100.0;
/// Regen -50 %.
pub const WEARY: f32 = 40.0;
/// No HP regen, move -10 %, damage dealt -15 %.
pub const GAZE_SICK: f32 = 70.0;
/// Outside deep shelter (and cairns) strain never drops below this.
const STRAIN_FLOOR: f32 = 10.0;
/// While dead strain is capped here, so a revive is not instantly overwhelmed again.
const DEAD_STRAIN_CAP: f32 = 50.0;
/// Overwhelmed: max HP lost per second, corruption gained per second.
const OVERWHELM_HP: f32 = 0.02;
const OVERWHELM_CORRUPTION: f32 = 1.0;
/// Cells from a cairn centre that count as resting by it.
pub const CAIRN_RANGE: f32 = 3.0;
/// Corruption cleared per second while resting at a cairn.
const CAIRN_CLEANSE: f32 = 0.5;

/// Hostile NPCs hit this much harder while the Eye is open ("beasts turn").
const NPC_OPEN_DAMAGE: f32 = 1.2;
const SICK_DAMAGE: f32 = 0.85;
const SICK_SPEED: f32 = 0.9;

/// Eye timer: half-lidded this long, then opens for [`OPEN_SECS`].
const LIDDED_SECS: f32 = 360.0;
const OPEN_SECS: f32 = 45.0;
/// Opening / Closing take this long.
pub const TRANSITION_SECS: f32 = 4.0;

pub const SPOT_RADIUS: f32 = 5.0;
/// Cells per second the gaze spot drifts.
const SPOT_SPEED: f32 = 0.8;
/// Eye and per-player gaze updates go out this often.
const SEND_SECS: f32 = 0.25;

/// Gaze state of every map that has a cover grid.
#[derive(Resource, Default)]
pub struct Gaze {
    pub maps: HashMap<i64, MapGaze>,
    test: GazeTest,
}

/// `DUSK_GAZE_TEST=open,strain=85,corruption=60` (comma separated) for screenshots: `open` (held
/// open), `opening` (opens after 3 s), `cycle` (20 s lidded / 12 s open), `spot` (the spot starts
/// on the start point), `strain=N` / `corruption=N` (initial player values), `at=X:Y` (players
/// are moved to cell X,Y when they join).
#[derive(Default, Clone)]
struct GazeTest {
    open: bool,
    cycle: bool,
    opening: bool,
    /// The gaze spot starts on the start point.
    spot: bool,
    strain: Option<f32>,
    corruption: Option<f32>,
    at: Option<Vec2>,
}

impl GazeTest {
    fn from_env() -> Self {
        let mut t = Self::default();
        for tok in std::env::var("DUSK_GAZE_TEST").unwrap_or_default().split(',').map(str::trim) {
            match tok.split_once('=') {
                Some(("strain", v)) => t.strain = v.parse().ok(),
                Some(("corruption", v)) => t.corruption = v.parse().ok(),
                Some(("at", v)) => {
                    t.at = v.split_once(':').and_then(|(x, y)| Some(Vec2::new(x.parse().ok()?, y.parse().ok()?)))
                }
                _ => match tok {
                    "open" => t.open = true,
                    "cycle" => t.cycle = true,
                    "opening" => t.opening = true,
                    "spot" => t.spot = true,
                    _ => {}
                },
            }
        }
        t
    }
}

pub struct MapGaze {
    pub cover: CoverGrid,
    /// Cell centres of the rest cairns.
    pub cairns: Vec<Vec2>,
    pub eye: EyeState,
    /// Seconds left in the current eye state (infinite while held open).
    timer: f32,
    /// How long the next `Open` lasts.
    open_for: f32,
    lidded_for: f32,
    /// Centre of the wandering gaze spot (cells); `None` if the map has no open ground.
    pub spot: Option<Vec2>,
    spot_vel: Vec2,
    spot_goal: Vec2,
    /// Walkable open-sky cells the spot wanders between.
    open_cells: Vec<Vec2>,
    send: f32,
    sent_state: Option<EyeState>,
}

impl MapGaze {
    fn new(cover: CoverGrid, grid: &dusk_formats::map::WalkGrid, test: &GazeTest, rng: &mut Rng) -> Self {
        let cairns = cover.cairns().into_iter().map(|(x, y)| Vec2::new(x, y)).collect();
        let open_cells: Vec<Vec2> = (0..cover.height as i32)
            .flat_map(|y| (0..cover.width as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| cover.get(x, y) == Cover::Open)
            .map(|(x, y)| Vec2::new(x as f32 + 0.5, y as f32 + 0.5))
            .filter(|c| grid.is_walkable(c.x, c.y))
            .collect();
        let pick = |rng: &mut Rng| open_cells[(rng.next_f32() * open_cells.len() as f32) as usize % open_cells.len()];
        let spot = (!open_cells.is_empty()).then(|| pick(rng));
        let goal = if open_cells.is_empty() { Vec2::ZERO } else { pick(rng) };
        let lidded_for = if test.cycle { 20.0 } else { LIDDED_SECS };
        let mut m = Self {
            cover,
            cairns,
            eye: EyeState::Lidded,
            timer: lidded_for,
            open_for: if test.cycle { 12.0 } else { OPEN_SECS },
            lidded_for,
            spot,
            spot_vel: Vec2::ZERO,
            spot_goal: goal,
            open_cells,
            send: 0.0,
            sent_state: None,
        };
        if test.open {
            m.eye = EyeState::Open;
            m.timer = f32::INFINITY;
        } else if test.opening {
            m.timer = 3.0;
        }
        m
    }

    /// 0 = half-lidded, 1 = wide open.
    pub fn openness(&self) -> f32 {
        let t = (self.timer / TRANSITION_SECS).clamp(0.0, 1.0);
        match self.eye {
            EyeState::Lidded => 0.0,
            EyeState::Opening => 1.0 - t,
            EyeState::Open => 1.0,
            EyeState::Closing => t,
        }
    }

    /// Cover at a position, with cairn range taking precedence.
    pub fn cover_at(&self, p: Vec2) -> GazeCover {
        if self.cairn_near(p).is_some() {
            return GazeCover::Cairn;
        }
        match self.cover.get(p.x.floor() as i32, p.y.floor() as i32) {
            Cover::Open => GazeCover::Open,
            Cover::Shade => GazeCover::Shade,
            Cover::Shelter | Cover::Cairn => GazeCover::Shelter,
        }
    }

    /// The cairn whose range contains `p`.
    pub fn cairn_near(&self, p: Vec2) -> Option<Vec2> {
        self.cairns
            .iter()
            .copied()
            .filter(|c| c.distance(p) <= CAIRN_RANGE)
            .min_by(|a, b| a.distance_squared(p).total_cmp(&b.distance_squared(p)))
    }

    pub fn in_spot(&self, p: Vec2) -> bool {
        self.spot.is_some_and(|s| s.distance(p) <= SPOT_RADIUS)
    }

    fn tick_eye(&mut self, dt: f32) {
        self.timer -= dt;
        if self.timer > 0.0 {
            return;
        }
        (self.eye, self.timer) = match self.eye {
            EyeState::Lidded => (EyeState::Opening, TRANSITION_SECS),
            EyeState::Opening => (EyeState::Open, self.open_for),
            EyeState::Open => (EyeState::Closing, TRANSITION_SECS),
            EyeState::Closing => (EyeState::Lidded, self.lidded_for),
        };
    }

    fn tick_spot(&mut self, dt: f32, rng: &mut Rng) {
        let Some(spot) = self.spot else { return };
        if spot.distance(self.spot_goal) < 1.5 {
            // Next goal: a random open cell, preferably not right next to the last one.
            for _ in 0..12 {
                let i = (rng.next_f32() * self.open_cells.len() as f32) as usize % self.open_cells.len();
                self.spot_goal = self.open_cells[i];
                if self.spot_goal.distance(spot) > 8.0 {
                    break;
                }
            }
        }
        let desired = (self.spot_goal - spot).normalize_or_zero() * SPOT_SPEED;
        // Slow steering so the path curves instead of zig-zagging.
        self.spot_vel = self.spot_vel.lerp(desired, (dt * 0.4).min(1.0));
        self.spot = Some(spot + self.spot_vel * dt);
    }
}

impl Gaze {
    /// Loads `maps/<name>.cover` for every loaded map that has one.
    pub fn load(root: &Path, world: &GameWorld) -> Self {
        let test = GazeTest::from_env();
        let mut rng = Rng::default();
        let mut maps = HashMap::new();
        let mut ids: Vec<_> = world.maps.keys().copied().collect();
        ids.sort();
        for id in ids {
            let m = &world.maps[&id];
            let path = root.join("maps").join(format!("{}.cover", m.name));
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            match CoverGrid::parse(&text) {
                Some(cover) => {
                    info!("gaze: {} has a cover grid ({}x{})", m.name, cover.width, cover.height);
                    let mut g = MapGaze::new(cover, &m.grid, &test, &mut rng);
                    if test.spot && world.start.0 == id {
                        g.spot = Some(world.start.1 + Vec2::new(1.5, 1.0));
                    }
                    maps.insert(id, g);
                }
                None => warn!("gaze: cannot parse {}", path.display()),
            }
        }
        Self { maps, test }
    }

    pub fn active(&self, map: i64) -> bool {
        self.maps.contains_key(&map)
    }

    pub fn eye(&self, map: i64) -> Option<EyeState> {
        self.maps.get(&map).map(|m| m.eye)
    }

    /// Opens the Eye on `map` (through `Opening`) and keeps it open for `secs` once fully open;
    /// pass `f32::INFINITY` to hold it open until [`Gaze::settle`]. No-op on maps without gaze.
    pub fn force_open(&mut self, map: i64, secs: f32) {
        let Some(m) = self.maps.get_mut(&map) else { return };
        match m.eye {
            EyeState::Lidded => {
                m.eye = EyeState::Opening;
                m.timer = TRANSITION_SECS;
                m.open_for = secs;
            }
            EyeState::Closing => {
                // Reverse from the current openness.
                let open = m.openness();
                m.eye = EyeState::Opening;
                m.timer = TRANSITION_SECS * (1.0 - open);
                m.open_for = secs;
            }
            EyeState::Opening => m.open_for = secs,
            EyeState::Open => m.timer = m.timer.max(secs),
        }
    }

    /// Settles the Eye on `map` back to half-lidded (through `Closing`) and restarts its timer.
    pub fn settle(&mut self, map: i64) {
        let Some(m) = self.maps.get_mut(&map) else { return };
        let open = m.openness();
        match m.eye {
            EyeState::Opening | EyeState::Open => {
                m.eye = EyeState::Closing;
                m.timer = TRANSITION_SECS * open;
            }
            EyeState::Lidded => m.timer = m.lidded_for,
            EyeState::Closing => {}
        }
        m.open_for = if self.test.cycle { 12.0 } else { OPEN_SECS };
    }
}

/// Strain and corruption of a player (added on first gaze tick).
#[derive(Component, Debug, Clone)]
pub struct Strain {
    pub strain: f32,
    pub corruption: f32,
    pub cover: GazeCover,
    pub in_spot: bool,
    pub in_combat: bool,
    send: f32,
    /// Fractional HP lost while overwhelmed, applied in whole points.
    hp_debt: f32,
    /// Seconds since the gaze first saw this player.
    age: f32,
}

impl Strain {
    pub fn weary(&self) -> bool {
        self.strain >= WEARY
    }

    pub fn gaze_sick(&self) -> bool {
        self.strain >= GAZE_SICK
    }

    pub fn overwhelmed(&self) -> bool {
        self.strain >= STRAIN_MAX
    }
}

/// The cairn a player last rested at: deaths respawn here (a walkable spot by the fire).
#[derive(Component, Debug, Clone, Copy)]
pub struct Bound {
    pub map: i64,
    pub pos: Vec2,
    pub cairn: Vec2,
}

/// Multipliers the gaze applies to a unit; read by `combat` and `spells`.
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct GazeMods {
    pub hp_regen: f32,
    pub mana_regen: f32,
    /// Damage dealt.
    pub damage: f32,
    pub speed: f32,
}

impl Default for GazeMods {
    fn default() -> Self {
        Self { hp_regen: 1.0, mana_regen: 1.0, damage: 1.0, speed: 1.0 }
    }
}

/// Damage after the attacker's gaze multiplier.
pub fn scale_damage(amount: i32, mods: Option<&GazeMods>) -> i32 {
    match mods {
        Some(m) if amount > 0 && m.damage != 1.0 => ((amount as f32 * m.damage).round() as i32).max(1),
        _ => amount,
    }
}

/// Strain gain multiplier from Ender resistance.
pub fn wil_resist(willpower: i32) -> f32 {
    1.0 - (willpower.max(0) as f32 * WIL_RESIST).min(WIL_RESIST_MAX)
}

/// Strain change per second for one player.
pub fn strain_rate(cover: GazeCover, in_combat: bool, openness: f32, in_spot: bool, willpower: i32) -> f32 {
    let base = match cover {
        GazeCover::Open => RATE_OPEN,
        GazeCover::Shade => RATE_SHADE,
        GazeCover::Shelter => return RATE_SHELTER,
        GazeCover::Cairn => return RATE_CAIRN,
    };
    let mut mult = 1.0 + (EYE_OPEN_MULT - 1.0) * openness;
    if in_combat {
        mult *= COMBAT_MULT;
    }
    if in_spot {
        mult *= SPOT_MULT;
    }
    base * mult * wil_resist(willpower)
}

/// Eye timers, spot drift and the per-map `Eye` broadcast.
pub fn tick_eyes(time: Res<Time>, mut gaze: ResMut<Gaze>, mut rng: Local<Rng>, mut outbox: ResMut<Outbox>) {
    let dt = time.delta_secs();
    for (&map, m) in gaze.maps.iter_mut() {
        m.tick_eye(dt);
        m.tick_spot(dt, &mut rng);
        m.send -= dt;
        if m.send <= 0.0 || m.sent_state != Some(m.eye) {
            m.send = SEND_SECS;
            m.sent_state = Some(m.eye);
            outbox.push(
                Scope::Map(map),
                ServerMsg::Eye {
                    state: m.eye,
                    openness: m.openness(),
                    spot: m.spot.map(to_pos),
                    spot_radius: SPOT_RADIUS,
                },
            );
        }
    }
}

type PlayerGaze<'a> = (
    Entity,
    &'a OnMap,
    &'a mut Motion,
    &'a mut Stats,
    &'a NetId,
    Option<&'a mut Strain>,
    Option<&'a mut GazeMods>,
    Option<&'a Bound>,
    Option<&'a CombatClock>,
    Has<Dead>,
);

/// Strain, corruption, cairn binding and the per-player `Gaze` message.
pub fn update_strain(
    mut commands: Commands,
    time: Res<Time>,
    gaze: Res<Gaze>,
    mut outbox: ResMut<Outbox>,
    mut players: Query<PlayerGaze, (With<Player>, Without<Hidden>)>,
) {
    let dt = time.delta_secs();
    for (e, map, mut m, mut stats, id, strain, mods, bound, clock, dead) in &mut players {
        let Some(g) = gaze.maps.get(&map.0) else {
            if mods.is_some_and(|m| *m != GazeMods::default()) {
                commands.entity(e).insert(GazeMods::default());
            }
            continue;
        };
        let Some(mut st) = strain else {
            commands.entity(e).insert((
                Strain {
                    strain: gaze.test.strain.unwrap_or(STRAIN_FLOOR),
                    corruption: gaze.test.corruption.unwrap_or(0.0),
                    cover: GazeCover::Open,
                    in_spot: false,
                    in_combat: false,
                    send: 0.0,
                    hp_debt: 0.0,
                    age: 0.0,
                },
                GazeMods::default(),
            ));
            continue;
        };
        st.age += dt;
        if let Some(at) = gaze.test.at.filter(|_| st.age >= 3.0 && st.age - dt < 3.0) {
            // Test teleport, once the client has spawned its player.
            m.pos = at;
            m.dirty = true;
            outbox.push(Scope::To(e), ServerMsg::Correct { pos: to_pos(at) });
        }
        st.cover = g.cover_at(m.pos);
        st.in_spot = matches!(st.cover, GazeCover::Open | GazeCover::Shade) && g.in_spot(m.pos);
        st.in_combat = clock.is_some_and(|c| c.0 < COMBAT_SECS);
        if dead {
            st.strain = st.strain.min(DEAD_STRAIN_CAP);
            st.hp_debt = 0.0;
        } else {
            let rate = strain_rate(st.cover, st.in_combat, g.openness(), st.in_spot, stats.attrs.willpower);
            let floor = if matches!(st.cover, GazeCover::Shelter | GazeCover::Cairn) { 0.0 } else { STRAIN_FLOOR };
            let next = (st.strain + rate * dt).min(STRAIN_MAX);
            // Gains can't push below the floor; shelter drains to zero.
            st.strain = if rate < 0.0 { next.max(floor.min(st.strain)) } else { next.max(floor) };

            if st.overwhelmed() {
                st.corruption = (st.corruption + OVERWHELM_CORRUPTION * dt).min(100.0);
                st.hp_debt += stats.max_hp as f32 * OVERWHELM_HP * dt;
                let lose = st.hp_debt.floor() as i32;
                if lose > 0 && stats.hp > 0 {
                    st.hp_debt -= lose as f32;
                    stats.hp = (stats.hp - lose).max(0);
                    outbox.push(
                        Scope::Near(map.0, m.pos),
                        ServerMsg::Health { id: id.0, hp: stats.hp, max_hp: stats.max_hp },
                    );
                }
            } else {
                st.hp_debt = 0.0;
            }
            if let Some(cairn) = g.cairn_near(m.pos) {
                st.corruption = (st.corruption - CAIRN_CLEANSE * dt).max(0.0);
                if bound.is_none_or(|b| b.map != map.0 || b.cairn != cairn) {
                    commands.entity(e).insert(Bound { map: map.0, pos: m.pos, cairn });
                    outbox.push(Scope::To(e), ServerMsg::CairnBound { pos: to_pos(cairn) });
                }
            }
        }

        let want = GazeMods {
            hp_regen: if st.gaze_sick() {
                0.0
            } else if st.weary() {
                0.5
            } else {
                1.0
            },
            mana_regen: if st.weary() { 0.5 } else { 1.0 },
            damage: if st.gaze_sick() { SICK_DAMAGE } else { 1.0 },
            speed: if st.gaze_sick() { SICK_SPEED } else { 1.0 },
        };
        match mods {
            Some(mut cur) if *cur != want => *cur = want,
            Some(_) => {}
            None => {
                commands.entity(e).insert(want);
            }
        }

        st.send -= dt;
        if st.send <= 0.0 {
            st.send = SEND_SECS;
            outbox.push(
                Scope::To(e),
                ServerMsg::Gaze {
                    strain: st.strain,
                    corruption: st.corruption,
                    cover: st.cover,
                    in_combat: st.in_combat,
                    in_spot: st.in_spot,
                },
            );
        }
    }
}

/// Eye-turned hostiles hit harder while the Eye is open.
pub fn npc_mods(
    mut commands: Commands,
    gaze: Res<Gaze>,
    mut npcs: Query<(Entity, &OnMap, &Faction, Option<&mut GazeMods>), With<Npc>>,
) {
    for (e, map, f, mods) in &mut npcs {
        let Some(g) = gaze.maps.get(&map.0) else { continue };
        let damage = if f.0 == faction::HOSTILE { 1.0 + (NPC_OPEN_DAMAGE - 1.0) * g.openness() } else { 1.0 };
        let want = GazeMods { damage, ..default() };
        match mods {
            Some(mut cur) if *cur != want => *cur = want,
            Some(_) => {}
            None => {
                commands.entity(e).insert(want);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(text: &str) -> MapGaze {
        let cover = CoverGrid::parse(text).unwrap();
        let n = cover.width * cover.height;
        let grid = dusk_formats::map::WalkGrid { size: cover.width as u32, flags: vec![0; n], floor: vec![true; n] };
        MapGaze::new(cover, &grid, &GazeTest::default(), &mut Rng::default())
    }

    #[test]
    fn rates() {
        assert_eq!(strain_rate(GazeCover::Open, false, 0.0, false, 0), 1.0);
        assert_eq!(strain_rate(GazeCover::Shade, false, 0.0, false, 0), 0.25);
        assert_eq!(strain_rate(GazeCover::Open, true, 1.0, true, 0), 24.0);
        assert_eq!(strain_rate(GazeCover::Shelter, true, 1.0, true, 0), -5.0);
        assert_eq!(strain_rate(GazeCover::Cairn, false, 0.0, false, 0), -15.0);
        // 40 % cap on Ender resistance.
        assert!((strain_rate(GazeCover::Open, false, 0.0, false, 20) - 0.9).abs() < 1e-6);
        assert!((strain_rate(GazeCover::Open, false, 0.0, false, 500) - 0.6).abs() < 1e-6);
    }

    #[test]
    fn cover_and_cairn_range() {
        let m = map("8 2\n.sS....C\n........\n");
        assert_eq!(m.cover_at(Vec2::new(0.5, 0.5)), GazeCover::Open);
        assert_eq!(m.cover_at(Vec2::new(1.5, 0.5)), GazeCover::Shade);
        assert_eq!(m.cover_at(Vec2::new(2.5, 0.5)), GazeCover::Shelter);
        assert_eq!(m.cover_at(Vec2::new(5.0, 1.5)), GazeCover::Cairn);
        assert_eq!(m.cover_at(Vec2::new(4.0, 1.5)), GazeCover::Open);
    }

    #[test]
    fn eye_cycle_and_director() {
        let mut gaze = Gaze::default();
        gaze.maps.insert(1, map("4 1\n....\n"));
        let m = gaze.maps.get_mut(&1).unwrap();
        m.tick_eye(LIDDED_SECS + 0.1);
        assert_eq!(m.eye, EyeState::Opening);
        m.tick_eye(TRANSITION_SECS / 2.0);
        assert!((m.openness() - 0.525).abs() < 0.05);
        gaze.settle(1);
        let m = gaze.maps.get_mut(&1).unwrap();
        assert_eq!(m.eye, EyeState::Closing);
        assert!(m.openness() > 0.4);
        gaze.force_open(1, f32::INFINITY);
        let m = gaze.maps.get_mut(&1).unwrap();
        assert_eq!(m.eye, EyeState::Opening);
        m.tick_eye(TRANSITION_SECS);
        assert_eq!(m.eye, EyeState::Open);
        m.tick_eye(10_000.0);
        assert_eq!(m.eye, EyeState::Open);
        gaze.settle(1);
        let m = gaze.maps.get_mut(&1).unwrap();
        m.tick_eye(TRANSITION_SECS + 0.01);
        assert_eq!(m.eye, EyeState::Lidded);
        gaze.force_open(2, 5.0); // no gaze on map 2: harmless
    }

    #[test]
    fn spot_stays_on_map() {
        let mut m = map("20 20\n");
        let mut rng = Rng::default();
        for _ in 0..2000 {
            m.tick_spot(0.1, &mut rng);
        }
        let s = m.spot.unwrap();
        assert!(s.x > -2.0 && s.y > -2.0 && s.x < 22.0 && s.y < 22.0);
    }
}
