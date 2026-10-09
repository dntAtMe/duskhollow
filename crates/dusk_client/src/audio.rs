//! Audio: per-map music and ambience (crossfaded), distance-attenuated sound effects for
//! melee, spells, NPC voices, level-up and UI, plus looping sounds next to map sprites
//! (`data/sprite_sounds.txt`). See `docs/audio.md`.
//!
//! Every sound is one of our files (`tools/sfxgen`, the soundtrack), resolved by bare name
//! ([`resolve_sound`]).
//! Other modules can trigger a sound by writing a [`PlaySfx`] message.
//! Keys: `M` toggles music + ambience, `N` toggles sound effects.
//! `DUSK_AUDIO_LOG=1` logs every sound that starts (at `info` level) and every missing one.
//! The gaze ambience layer, NPC voices, proximity emitters, footsteps and `DUSK_GAZE_FAKE`
//! live in [`custom`].

mod custom;

use crate::{
    combat_ui::UiFont,
    data::GameData,
    items_ui::{ItemDb, ItemsState},
    map_render::{CurrentMap, MapLoaded},
    net::{CombatNet, Net, PlayerState, SpellNet},
    player::Player,
    unit::{Npc, Targeted, Unit},
};
use bevy::audio::{AudioSinkPlayback, PlaybackMode, Volume};
use bevy::prelude::*;
use dusk_formats::{
    item::slot,
    map::MapFile,
    sound::{SoundTables, builtin, resolve_sound, split_playlist, voice_lines},
};
use dusk_protocol::{HitResult, ServerMsg};
use std::collections::HashMap;

/// DESIGN: sound effects play at full volume within `NEAR` cells of the player, fade out
/// linearly and are dropped beyond `FAR` cells (about the visible screen width).
const NEAR: f32 = 4.0;
const FAR: f32 = 18.0;
/// DESIGN: music/ambience crossfade duration.
const FADE_SECS: f32 = 2.0;
/// How often the proximity loops are re-evaluated.
const PROXIMITY_CHECK: f32 = 0.25;
/// DESIGN: an NPC/player voices at most one line per this many seconds (deaths excepted).
const VOICE_COOLDOWN: f32 = 1.5;
/// DESIGN: an NPC that has not swung for this long plays its `aggro` line on the next swing.
const AGGRO_GAP: f32 = 15.0;
/// Cap on simultaneous one-shot effects.
const MAX_SFX: usize = 24;

pub struct AudioPlugin;

impl Plugin for AudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<AudioSettings>()
            .init_resource::<Music>()
            .init_resource::<Proximity>()
            .init_resource::<SfxQueue>()
            .init_resource::<Rng>()
            .init_resource::<Listener>()
            .add_message::<PlaySfx>()
            .add_systems(Startup, setup)
            .add_systems(
                Update,
                (
                    update_listener,
                    toggle_keys.run_if(crate::state::in_game),
                    on_map_loaded,
                    drive_tracks,
                    drive_proximity,
                    combat_sounds.run_if(crate::state::in_game),
                    spell_sounds.run_if(crate::state::in_game),
                    ui_sounds,
                    play_sfx,
                    fade_notice,
                )
                    .chain(),
            );
        custom::build(app);
    }
}

// ---------------------------------------------------------------- settings

/// Volumes are linear gains (the options menu and the env overrides use 0..100).
#[derive(Resource, Debug, Clone, PartialEq)]
pub struct AudioSettings {
    /// Music and ambience.
    pub music_on: bool,
    pub sfx_on: bool,
    pub music_volume: f32,
    pub sfx_volume: f32,
    /// Scales both (the main menu's master volume; 1 by default).
    pub master_volume: f32,
    /// `DUSK_AUDIO_LOG`: log every started sound at info level.
    pub log: bool,
}

impl Default for AudioSettings {
    /// DESIGN: music 15 %, effects 20 % (the options menu changes them).
    fn default() -> Self {
        Self {
            music_on: true,
            sfx_on: true,
            music_volume: 0.15,
            sfx_volume: 0.20,
            master_volume: 1.0,
            log: std::env::var_os("DUSK_AUDIO_LOG").is_some(),
        }
    }
}

impl AudioSettings {
    /// Applies `DUSK_MUSIC_VOLUME` / `DUSK_SFX_VOLUME` (0..100; unparsable values are ignored).
    fn apply_env(&mut self, var: impl Fn(&str) -> Option<String>) {
        let volume = |name| var(name).and_then(|v| v.trim().parse::<f32>().ok()).map(|v| (v / 100.0).clamp(0.0, 1.0));
        if let Some(v) = volume("DUSK_MUSIC_VOLUME") {
            self.music_volume = v;
        }
        if let Some(v) = volume("DUSK_SFX_VOLUME") {
            self.sfx_volume = v;
        }
    }

    fn music_gain(&self) -> f32 {
        if self.music_on { self.music_volume * self.master_volume } else { 0.0 }
    }

    fn sfx_gain(&self) -> f32 {
        if self.sfx_on { self.sfx_volume * self.master_volume } else { 0.0 }
    }

    /// Reports a sound name that does not resolve to one of our files (once per name; a warning
    /// with `DUSK_AUDIO_LOG`).
    fn missing(&self, name: &str) {
        static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
        let mut seen = SEEN.lock().unwrap();
        if seen.iter().any(|n| n == name) {
            return;
        }
        seen.push(name.to_string());
        if self.log {
            warn!("audio: missing sound {name}");
        } else {
            debug!("audio: missing sound {name}");
        }
    }

    fn log(&self, what: std::fmt::Arguments) {
        if self.log {
            info!("audio: {what}");
        } else {
            debug!("audio: {what}");
        }
    }
}

#[derive(Resource, Default)]
struct SoundDb(SoundTables);

/// Where the ears are (cells): the player, else (main menu) the middle of the view.
#[derive(Resource, Default)]
pub struct Listener(pub Option<Vec2>);

fn update_listener(
    player: Query<&Unit, With<Player>>,
    camera: Query<&Transform, With<crate::player::MainCamera>>,
    mut listener: ResMut<Listener>,
) {
    let at = player
        .single()
        .ok()
        .map(|u| u.pos)
        .or_else(|| camera.single().ok().map(|t| crate::iso::to_cell(t.translation.truncate())));
    if listener.0 != at {
        listener.0 = at;
    }
}

fn setup(mut commands: Commands, data: Res<GameData>, mut settings: ResMut<AudioSettings>) {
    settings.apply_env(|name| std::env::var(name).ok());
    let tables = dusk_formats::content::sounds::load(&data.root).unwrap_or_else(|e| {
        warn!("audio: cannot read sound tables: {e}");
        SoundTables::default()
    });
    info!(
        "audio: music {:.0}% ({}), sfx {:.0}% ({}); {} soundtrack tracks, {} sprite sounds",
        settings.music_volume * 100.0,
        if settings.music_on { "on" } else { "off" },
        settings.sfx_volume * 100.0,
        if settings.sfx_on { "on" } else { "off" },
        tables.soundtrack.len(),
        tables.sprite_sounds.len()
    );
    commands.insert_resource(SoundDb(tables));
}

/// Small xorshift RNG for picking playlist entries and sound variants.
#[derive(Resource)]
struct Rng(u64);

impl Default for Rng {
    fn default() -> Self {
        let seed =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64);
        Self(seed | 1)
    }
}

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        if items.is_empty() { None } else { items.get((self.next() % items.len() as u64) as usize) }
    }
}

// ---------------------------------------------------------------- one-shot effects

/// Where a sound effect plays.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SfxAt {
    /// Not positional (UI, alerts).
    Ui,
    /// At a unit (position looked up when the sound starts).
    Unit(Entity),
}

/// Request to play a sound effect by name (bare names are our `.wav` files: `gaze_open`).
#[derive(Message, Clone, Debug)]
pub struct PlaySfx {
    pub name: String,
    pub at: SfxAt,
    /// Seconds to wait before playing (e.g. projectile travel time).
    pub delay: f32,
}

impl PlaySfx {
    pub fn ui(name: impl Into<String>) -> Self {
        Self { name: name.into(), at: SfxAt::Ui, delay: 0.0 }
    }

    pub fn at_unit(name: impl Into<String>, unit: Entity) -> Self {
        Self { name: name.into(), at: SfxAt::Unit(unit), delay: 0.0 }
    }
}

#[derive(Resource, Default)]
struct SfxQueue(Vec<(f32, PlaySfx)>);

#[derive(Component)]
struct SfxVoice;

/// DESIGN: linear distance falloff (cells) for positional effects.
fn attenuation(distance: f32) -> f32 {
    ((FAR - distance) / (FAR - NEAR)).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn play_sfx(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    mut queue: ResMut<SfxQueue>,
    mut requests: MessageReader<PlaySfx>,
    units: Query<&Unit>,
    player: Query<&Unit, With<Player>>,
    voices: Query<(), With<SfxVoice>>,
) {
    let now = time.elapsed_secs();
    queue.0.extend(requests.read().map(|r| (now + r.delay, r.clone())));
    if queue.0.is_empty() {
        return;
    }
    let (due, waiting): (Vec<_>, Vec<_>) = std::mem::take(&mut queue.0).into_iter().partition(|(t, _)| *t <= now);
    queue.0 = waiting;
    let listener = player.single().ok().map(|u| u.pos);
    let mut active = voices.iter().count();
    let mut started: Vec<&str> = Vec::new();
    for (_, sfx) in &due {
        if settings.sfx_gain() <= 0.0 || active >= MAX_SFX || started.contains(&sfx.name.as_str()) {
            continue;
        }
        let pos = match sfx.at {
            SfxAt::Ui => None,
            SfxAt::Unit(e) => match units.get(e) {
                Ok(u) => Some(u.pos),
                Err(_) => continue,
            },
        };
        let gain = match (pos, listener) {
            (None, _) => 1.0,
            (Some(p), Some(l)) => attenuation(p.distance(l)),
            (Some(_), None) => 0.0,
        };
        if gain <= 0.0 {
            continue;
        }
        let Some(path) = custom::resolve_sfx(&data.index, &sfx.name) else {
            settings.missing(&sfx.name);
            continue;
        };
        commands.spawn((
            SfxVoice,
            AudioPlayer::new(assets.load(path.to_string())),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain * settings.sfx_gain())),
        ));
        settings.log(format_args!("sfx {} (gain {gain:.2})", sfx.name));
        started.push(&sfx.name);
        active += 1;
    }
}

// ---------------------------------------------------------------- music + ambience

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Music,
    Ambience,
}

/// A playing music or ambience track; `gain` is its fade level (0..1).
#[derive(Component)]
struct Track {
    channel: Channel,
    gain: f32,
}

/// Track is being faded out (despawned at gain 0).
#[derive(Component)]
struct FadingOut;

#[derive(Default)]
struct ChannelState {
    /// Playable tracks of the current map.
    playlist: Vec<String>,
    current: Option<(Entity, String)>,
}

#[derive(Resource, Default)]
struct Music {
    music: ChannelState,
    ambience: ChannelState,
}

/// DESIGN: a map's `music=` list (its playable tracks), else the whole soundtrack.
fn map_playlist(map_music: &[String], soundtrack: &[String], playable: impl Fn(&str) -> bool) -> Vec<String> {
    let own: Vec<String> = map_music.iter().filter(|t| playable(t)).cloned().collect();
    if own.is_empty() { soundtrack.iter().filter(|t| playable(t)).cloned().collect() } else { own }
}

/// Fades out the channel's current track and starts another one from its playlist.
fn start_track(
    commands: &mut Commands,
    assets: &AssetServer,
    data: &GameData,
    settings: &AudioSettings,
    rng: &mut Rng,
    state: &mut ChannelState,
    channel: Channel,
) {
    let previous = state.current.take();
    if let Some((e, _)) = &previous {
        commands.entity(*e).try_insert(FadingOut);
    }
    let candidates: Vec<&String> = state
        .playlist
        .iter()
        .filter(|t| state.playlist.len() == 1 || previous.as_ref().is_none_or(|(_, p)| p != *t))
        .collect();
    let Some(name) = rng.pick(&candidates).map(|n| (*n).clone()) else { return };
    let Some(path) = resolve_sound(&data.index, &name) else { return };
    let mode = match channel {
        Channel::Music => PlaybackMode::Once,
        Channel::Ambience => PlaybackMode::Loop,
    };
    let e = commands
        .spawn((
            Track { channel, gain: 0.0 },
            AudioPlayer::new(assets.load(path.to_string())),
            PlaybackSettings { mode, volume: Volume::Linear(0.0), ..PlaybackSettings::ONCE },
        ))
        .id();
    settings.log(format_args!("{channel:?} -> {name}"));
    state.current = Some((e, name));
}

#[allow(clippy::too_many_arguments)]
fn on_map_loaded(
    mut commands: Commands,
    mut loaded: MessageReader<MapLoaded>,
    current: Res<CurrentMap>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    db: Res<SoundDb>,
    settings: Res<AudioSettings>,
    mut rng: ResMut<Rng>,
    mut music: ResMut<Music>,
    mut proximity: ResMut<Proximity>,
) {
    if loaded.read().count() == 0 {
        return;
    }
    let info = data.maps.iter().find(|m| m.name == current.name);
    let playable = |t: &str| resolve_sound(&data.index, t).is_some();
    let map_music = info.map(|i| split_playlist(&i.music.join(","))).unwrap_or_default();
    let music_list = map_playlist(&map_music, &db.0.soundtrack, playable);
    let ambience_list: Vec<String> =
        info.map(|i| split_playlist(&i.ambience)).unwrap_or_default().into_iter().filter(|t| playable(t)).collect();
    settings.log(format_args!("map {}: music {music_list:?}, ambience {ambience_list:?}", current.name));
    let music = &mut *music;
    for (state, list, channel) in
        [(&mut music.music, music_list, Channel::Music), (&mut music.ambience, ambience_list, Channel::Ambience)]
    {
        // A track that is also on the new map's list keeps playing.
        let keep = state.current.as_ref().is_some_and(|(_, name)| list.contains(name));
        state.playlist = list;
        if !keep {
            start_track(&mut commands, &assets, &data, &settings, &mut rng, state, channel);
        }
    }

    for g in proximity.groups.drain(..) {
        if let Some(e) = g.entity {
            commands.entity(e).despawn();
        }
    }
    let path = data.find_file(&format!("maps/{}.map", current.name));
    let Some(Ok(map)) = path.map(MapFile::load) else { return };
    custom::load_proximity(&mut proximity.groups, &map, &data, &db.0, &current.name);
    info!(
        "audio: map {}: proximity sounds {:?}",
        current.name,
        proximity.groups.iter().map(|g| (g.sound.as_str(), g.points.len())).collect::<Vec<_>>()
    );
}

/// Fades tracks in/out, applies the volume settings and picks the next song when one ends.
#[allow(clippy::too_many_arguments)]
fn drive_tracks(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    mut rng: ResMut<Rng>,
    mut music: ResMut<Music>,
    mut tracks: Query<(Entity, &mut Track, Has<FadingOut>, Option<&mut AudioSink>)>,
) {
    let step = time.delta_secs() / FADE_SECS;
    let mut finished = false;
    for (e, mut track, fading, sink) in &mut tracks {
        track.gain = if fading { track.gain - step } else { (track.gain + step).min(1.0) };
        if fading && track.gain <= 0.0 {
            commands.entity(e).despawn();
            continue;
        }
        let Some(mut sink) = sink else { continue };
        sink.set_volume(Volume::Linear(track.gain.max(0.0) * settings.music_gain()));
        if track.channel == Channel::Music && !fading && sink.empty() {
            commands.entity(e).despawn();
            if music.music.current.as_ref().is_some_and(|(c, _)| *c == e) {
                music.music.current = None;
                finished = true;
            }
        }
    }
    if finished {
        start_track(&mut commands, &assets, &data, &settings, &mut rng, &mut music.music, Channel::Music);
    }
}

// ---------------------------------------------------------------- proximity loops

struct ProximityGroup {
    sound: String,
    radius: f32,
    /// Cell centres of every sprite that emits this sound.
    points: Vec<Vec2>,
    entity: Option<Entity>,
    gain: f32,
    target: f32,
}

#[derive(Resource, Default)]
struct Proximity {
    groups: Vec<ProximityGroup>,
    check: f32,
}

#[derive(Component)]
struct ProximityVoice;

/// DESIGN: linear falloff from the nearest emitter to silence at `radius` cells.
fn proximity_gain(points: &[Vec2], radius: f32, listener: Vec2) -> f32 {
    if radius <= 0.0 {
        return 0.0;
    }
    let d = points.iter().map(|p| p.distance_squared(listener)).fold(f32::INFINITY, f32::min).sqrt();
    (1.0 - d / radius).clamp(0.0, 1.0)
}

#[allow(clippy::too_many_arguments)]
fn drive_proximity(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    mut proximity: ResMut<Proximity>,
    mut sinks: Query<&mut AudioSink, With<ProximityVoice>>,
    listener: Res<Listener>,
) {
    let Some(listener) = listener.0 else { return };
    proximity.check -= time.delta_secs();
    let recheck = proximity.check <= 0.0;
    if recheck {
        proximity.check = PROXIMITY_CHECK;
    }
    let step = time.delta_secs() / 0.5;
    for g in &mut proximity.groups {
        if recheck {
            g.target = proximity_gain(&g.points, g.radius, listener);
        }
        g.gain += (g.target - g.gain).clamp(-step, step);
        match g.entity {
            None if g.target > 0.0 && settings.sfx_gain() > 0.0 => {
                let Some(path) = resolve_sound(&data.index, &g.sound) else {
                    settings.missing(&g.sound);
                    continue;
                };
                g.entity = Some(
                    commands
                        .spawn((
                            ProximityVoice,
                            AudioPlayer::new(assets.load(path.to_string())),
                            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
                        ))
                        .id(),
                );
                settings.log(format_args!("proximity loop {} starts", g.sound));
            }
            Some(e) if g.target <= 0.0 && g.gain <= 0.0 => {
                commands.entity(e).despawn();
                g.entity = None;
                settings.log(format_args!("proximity loop {} stops", g.sound));
            }
            Some(e) => {
                if let Ok(mut sink) = sinks.get_mut(e) {
                    sink.set_volume(Volume::Linear(g.gain * settings.sfx_gain()));
                }
            }
            None => {}
        }
    }
}

// ---------------------------------------------------------------- game events

/// Per-unit timers for voice throttling and aggro detection.
#[derive(Default)]
struct VoiceTimers {
    last_voice: HashMap<Entity, f32>,
    last_swing: HashMap<Entity, f32>,
}

fn npc_model<'a>(data: &'a GameData, npc: &Npc) -> Option<&'a str> {
    let template = data.npc_templates.get(&npc.entry)?;
    data.npc_models.get(&template.model_id).map(|m| m.name.as_str())
}

/// DESIGN: the hit sounds of a melee swing by the local player: edged weapons (axe, sword,
/// dagger) cut, the rest (mace, staff, wand) and an empty hand thud. `None` = unknown weapon.
fn player_hit_sounds(items: Option<&ItemsState>, db: Option<&ItemDb>) -> Option<&'static [&'static str]> {
    let Some(weapon) = items?.equipment.get(slot::WEAPON)? else { return Some(&builtin::HIT_UNARMED) };
    Some(match db?.items.get(&(weapon.entry as i64))?.weapon_type {
        1 | 4 | 6 => &builtin::HIT_BLADE,
        _ => &builtin::HIT_BLUNT,
    })
}

/// Melee results (`ServerMsg::Swing`) and deaths: hit/miss sounds ([`builtin`]) and NPC
/// voices (`npc_<model>_<event>`).
#[allow(clippy::too_many_arguments)]
fn combat_sounds(
    time: Res<Time>,
    data: Res<GameData>,
    items: Option<Res<ItemsState>>,
    item_db: Option<Res<ItemDb>>,
    net: Res<Net>,
    mut rng: ResMut<Rng>,
    mut timers: Local<VoiceTimers>,
    mut events: MessageReader<CombatNet>,
    npcs: Query<&Npc>,
    units: Query<&Unit>,
    mut out: MessageWriter<PlaySfx>,
) {
    let now = time.elapsed_secs();
    let timers = &mut *timers;
    // Plays a random voice line for an NPC unit (throttled unless `throttle` is false).
    let voice = |out: &mut MessageWriter<PlaySfx>,
                 rng: &mut Rng,
                 last_voice: &mut HashMap<Entity, f32>,
                 e: Entity,
                 event: &str,
                 throttle: bool| {
        let Some(model) = npcs.get(e).ok().and_then(|n| npc_model(&data, n)) else { return };
        if throttle && last_voice.get(&e).is_some_and(|t| now - t < VOICE_COOLDOWN) {
            return;
        }
        if let Some(s) = rng.pick(&voice_lines(&data.index, model, event)) {
            last_voice.insert(e, now);
            out.write(PlaySfx::at_unit(s.clone(), e));
        }
    };
    for CombatNet(msg) in events.read() {
        match msg {
            ServerMsg::Swing { attacker, target, result, .. } => {
                let att = net.entities.get(attacker).copied();
                let tgt = net.entities.get(target).copied();
                let att_npc = att.is_some_and(|e| npcs.contains(e));
                let hit = matches!(result, HitResult::Hit | HitResult::Crit);
                // DESIGN: NPCs hit with claws and fists; other players are assumed to wield a blade.
                let hits: &[&str] = if att_npc {
                    &builtin::HIT_UNARMED
                } else if Some(*attacker) == net.my_id {
                    player_hit_sounds(items.as_deref(), item_db.as_deref()).unwrap_or(&builtin::HIT_BLADE)
                } else {
                    &builtin::HIT_BLADE
                };
                let sound = match result {
                    HitResult::Hit | HitResult::Crit => rng.pick(hits).copied(),
                    HitResult::Miss | HitResult::Evade => Some(builtin::MISS),
                    HitResult::Dodge => Some(builtin::DODGE),
                    HitResult::Block => rng.pick(&builtin::BLOCK).copied(),
                    HitResult::Parry => Some(builtin::PARRY),
                    HitResult::Resist | HitResult::Immune => None,
                };
                if let (Some(s), Some(t)) = (sound, tgt) {
                    // Lands with the swing's hit frame (`Unit::impact_in`).
                    let delay = att.and_then(|a| units.get(a).ok()).map_or(0.0, |u| u.impact_in());
                    out.write(PlaySfx { delay, ..PlaySfx::at_unit(s, t) });
                }
                if let Some(a) = att.filter(|_| att_npc) {
                    let engaged = timers.last_swing.insert(a, now).is_some_and(|t| now - t < AGGRO_GAP);
                    let model = npcs.get(a).ok().and_then(|n| npc_model(&data, n)).unwrap_or_default();
                    let aggro = !engaged && !voice_lines(&data.index, model, "aggro").is_empty();
                    voice(
                        &mut out,
                        &mut rng,
                        &mut timers.last_voice,
                        a,
                        if aggro { "aggro" } else { "attack" },
                        !aggro,
                    );
                }
                if let Some(t) = tgt.filter(|_| hit) {
                    if npcs.contains(t) {
                        voice(&mut out, &mut rng, &mut timers.last_voice, t, "damage", true);
                    } else if Some(*target) == net.my_id
                        && !timers.last_voice.get(&t).is_some_and(|l| now - l < VOICE_COOLDOWN)
                    {
                        timers.last_voice.insert(t, now);
                        if let Some(s) = rng.pick(&builtin::PLAYER_HURT) {
                            out.write(PlaySfx::at_unit(*s, t));
                        }
                    }
                }
            }
            ServerMsg::Died { id } => {
                if let Some(&e) = net.entities.get(id) {
                    voice(&mut out, &mut rng, &mut timers.last_voice, e, "die", false);
                    timers.last_swing.remove(&e);
                }
            }
            _ => {}
        }
    }
    if timers.last_voice.len() > 512 {
        timers.last_voice.retain(|_, t| now - *t < 60.0);
        timers.last_swing.retain(|_, t| now - *t < 60.0);
    }
}

/// Spell kit sounds: casting kit on `CastStart`, go + traveling kit on `SpellGo` at the
/// caster, impact kit at each target once the projectile arrives.
fn spell_sounds(
    data: Res<GameData>,
    net: Res<Net>,
    mut events: MessageReader<SpellNet>,
    mut out: MessageWriter<PlaySfx>,
) {
    let kit_sound = |k: &Option<dusk_formats::spell::VisualKit>| {
        k.as_ref().map(|k| k.sound.trim().to_string()).filter(|s| !s.is_empty() && s != "0")
    };
    for SpellNet(msg) in events.read() {
        match msg {
            ServerMsg::CastStart { caster, spell, .. } => {
                let (Some(v), Some(&c)) = (data.spell_visuals.get(&(*spell as i64)), net.entities.get(caster)) else {
                    continue;
                };
                if let Some(s) = kit_sound(&v.casting) {
                    out.write(PlaySfx::at_unit(s, c));
                }
            }
            ServerMsg::SpellGo { caster, spell, targets, travel_ms } => {
                let Some(v) = data.spell_visuals.get(&(*spell as i64)) else { continue };
                if let Some(&c) = net.entities.get(caster) {
                    if let Some(s) = kit_sound(&v.go) {
                        out.write(PlaySfx::at_unit(s, c));
                    }
                    if *travel_ms > 0 {
                        if let Some(s) = kit_sound(&v.traveling) {
                            out.write(PlaySfx::at_unit(s, c));
                        }
                    }
                }
                if let Some(s) = kit_sound(&v.impact) {
                    for t in targets.iter().filter_map(|t| net.entities.get(t)).take(3) {
                        out.write(PlaySfx { name: s.clone(), at: SfxAt::Unit(*t), delay: *travel_ms as f32 / 1000.0 });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Button clicks, target selection and level-up alerts.
fn ui_sounds(
    state: Option<Res<PlayerState>>,
    mut last_level: Local<u32>,
    buttons: Query<&Interaction, (Changed<Interaction>, With<Button>)>,
    targeted: Query<(), Added<Targeted>>,
    mut out: MessageWriter<PlaySfx>,
) {
    if buttons.iter().any(|i| *i == Interaction::Pressed) {
        out.write(PlaySfx::ui(builtin::BUTTON_CLICK));
    }
    if !targeted.is_empty() {
        out.write(PlaySfx::ui(builtin::TARGET_OPEN));
    }
    let level = state.map_or(0, |s| s.level);
    if *last_level > 0 && level > *last_level {
        out.write(PlaySfx::ui(builtin::LEVEL_UP));
    }
    *last_level = level;
}

// ---------------------------------------------------------------- keys + notice

#[derive(Component)]
struct AudioNotice(f32);

fn toggle_keys(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    font: Option<Res<UiFont>>,
    mut settings: ResMut<AudioSettings>,
    notices: Query<Entity, With<AudioNotice>>,
    captured: Res<crate::ui_input::UiInputCaptured>,
) {
    if captured.keyboard {
        return; // typing in chat
    }
    let text = if keys.just_pressed(KeyCode::KeyM) {
        settings.music_on = !settings.music_on;
        format!("Music {}", if settings.music_on { "on" } else { "off" })
    } else if keys.just_pressed(KeyCode::KeyN) {
        settings.sfx_on = !settings.sfx_on;
        format!("Sound effects {}", if settings.sfx_on { "on" } else { "off" })
    } else {
        return;
    };
    info!("audio: {text}");
    for e in &notices {
        commands.entity(e).despawn();
    }
    let mut text_font = TextFont { font_size: 18.0.into(), ..default() };
    if let Some(font) = font {
        text_font.font = font.0.clone().into();
    }
    commands.spawn((
        AudioNotice(2.0),
        Text::new(text),
        text_font,
        TextColor(Color::srgb(0.95, 0.85, 0.6)),
        Node { position_type: PositionType::Absolute, top: Val::Px(90.0), left: Val::Percent(45.0), ..default() },
    ));
}

fn fade_notice(
    mut commands: Commands,
    time: Res<Time>,
    mut notices: Query<(Entity, &mut AudioNotice, &mut TextColor)>,
) {
    for (e, mut n, mut color) in &mut notices {
        n.0 -= time.delta_secs();
        if n.0 <= 0.0 {
            commands.entity(e).despawn();
        } else {
            color.0.set_alpha(n.0.min(1.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff() {
        assert_eq!(attenuation(0.0), 1.0);
        assert_eq!(attenuation(NEAR), 1.0);
        assert!((attenuation((NEAR + FAR) / 2.0) - 0.5).abs() < 1e-6);
        assert_eq!(attenuation(FAR + 1.0), 0.0);
    }

    #[test]
    fn env_volumes() {
        let mut s = AudioSettings::default();
        let env = |name: &str| match name {
            "DUSK_MUSIC_VOLUME" => Some("40".to_string()),
            "DUSK_SFX_VOLUME" => Some(" 250 ".to_string()),
            _ => None,
        };
        s.apply_env(env);
        assert_eq!((s.music_volume, s.sfx_volume), (0.4, 1.0));
        s.apply_env(|_| Some("loud".to_string()));
        assert_eq!((s.music_volume, s.sfx_volume), (0.4, 1.0));
    }

    #[test]
    fn map_music_or_soundtrack() {
        let playable = |t: &str| t != "missing.ogg";
        let soundtrack = vec!["a.mp3".to_string(), "b.mp3".to_string()];
        let own = vec!["missing.ogg".to_string(), "m.ogg".to_string()];
        assert_eq!(map_playlist(&own, &soundtrack, playable), ["m.ogg"]);
        assert_eq!(map_playlist(&own[..1], &soundtrack, playable), soundtrack);
        assert_eq!(map_playlist(&[], &soundtrack, playable), soundtrack);
    }

    /// Every sound our data and the client name (`content::sounds::referenced`) is one of our
    /// files and decodes with Bevy's decoders (WAV, OGG/Vorbis, MP3).
    #[test]
    fn referenced_sounds_decode() {
        use bevy::audio::{AudioSource, Decodable};
        let root = dusk_formats::assets_root();
        let index = dusk_formats::content::sounds::index(&root);
        let refs = dusk_formats::content::sounds::referenced(&root, &index).unwrap();
        let mut names: Vec<&str> = refs.iter().map(|(_, n)| n.as_str()).collect();
        names.sort();
        names.dedup();
        let mut failed = Vec::new();
        for name in &names {
            let Some(rel) = resolve_sound(&index, name) else {
                failed.push(format!("{name}: missing"));
                continue;
            };
            let bytes = std::fs::read(root.join(rel)).unwrap();
            let source = AudioSource { bytes: bytes.into() };
            if !std::panic::catch_unwind(|| source.decoder().take(4096).count() > 0).unwrap_or(false) {
                failed.push(format!("{rel}: does not decode"));
            }
        }
        assert!(failed.is_empty(), "{failed:#?}");
        assert!(names.len() > 60, "only {} sounds referenced", names.len());
    }

    #[test]
    fn proximity_falloff() {
        let points = [Vec2::new(2.5, 3.5)];
        assert_eq!(proximity_gain(&points, 4.0, Vec2::new(2.5, 3.5)), 1.0);
        assert!((proximity_gain(&points, 4.0, Vec2::new(4.5, 3.5)) - 0.5).abs() < 1e-6);
        assert_eq!(proximity_gain(&points, 2.0, Vec2::new(20.0, 20.0)), 0.0);
        assert_eq!(proximity_gain(&points, 0.0, Vec2::new(2.5, 3.5)), 0.0);
    }

    #[test]
    fn rng_picks_every_entry() {
        let mut rng = Rng(12345);
        let items = [1, 2, 3];
        let mut seen = [false; 3];
        for _ in 0..100 {
            seen[*rng.pick(&items).unwrap() as usize - 1] = true;
        }
        assert!(seen.iter().all(|s| *s));
        assert!(rng.pick::<u8>(&[]).is_none());
    }
}
