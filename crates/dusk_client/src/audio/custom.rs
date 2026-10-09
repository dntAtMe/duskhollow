//! Duskhollow additions to the audio system (docs/audio.md, "Gaze layer" and after):
//!
//! - the gaze ambience layer: open sky / shelter / open-Eye loops mixed on top of the zone
//!   ambience from [`GazeView`] (cover, openness), plus strain cues (heartbeat >= 40,
//!   whispers and breaths >= 70);
//! - `DUSK_GAZE_FAKE` to drive [`GazeView`] without the server;
//! - custom NPC voices (`npc_<model>_<event>.wav`) and greets on targeting;
//! - data-driven proximity loops (`custom_assets/data/sprite_sounds.txt`) and cairn fire at
//!   the `C` cells of a map's `.cover` sidecar;
//! - player footsteps on custom maps.
//!
//! Sounds come from `tools/sfxgen` (`custom_assets/content/custom/sfx/`).

use super::{AudioSettings, ProximityGroup, Rng, SfxAt, SfxVoice};
use crate::{
    data::GameData,
    gaze::{CoverKind, EyeState, GazeView},
    player::Player,
    unit::{Npc, Targeted, Unit},
};
use bevy::audio::{AudioSinkPlayback, Volume};
use bevy::prelude::*;
use dusk_formats::{FileIndex, custom::CoverGrid, map::MapFile, sound::resolve_sound};
use std::collections::HashMap;

/// DESIGN: gaze layer fade time (seconds for a full 0 -> 1 change).
const LAYER_FADE: f32 = 1.5;
/// DESIGN: radius (cells) of the cairn fire loop around each `C` cover cell.
const CAIRN_FIRE_RADIUS: f32 = 6.0;
const CAIRN_FIRE: &str = "loop_cairn_fire.wav";
/// DESIGN: one footstep per this many cells walked (run speed is 4 cells/s).
const STEP_CELLS: f32 = 1.25;
/// DESIGN: footsteps play at this fraction of the effects volume.
const STEP_GAIN: f32 = 0.45;
/// DESIGN: a friendly NPC greets at most once per this many seconds.
const GREET_COOLDOWN: f32 = 8.0;

pub(super) fn build(app: &mut App) {
    app.init_resource::<GazeLayer>().add_systems(
        Update,
        (fake_gaze, drive_gaze_layer, strain_cues, footsteps, greet_on_target)
            .chain()
            .after(super::drive_proximity)
            .before(super::play_sfx),
    );
}

// ---------------------------------------------------------------- name resolution

/// [`resolve_sound`], also accepting bare names of our own `.wav` files (`gaze_open`).
pub(super) fn resolve_sfx<'a>(index: &'a FileIndex, name: &str) -> Option<&'a str> {
    resolve_sound(index, name).or_else(|| (!name.contains('.')).then(|| index.resolve(&format!("{name}.wav")))?)
}

/// Custom voice lines of an NPC model: `npc_<model>_<event>.wav` and `_1`.. `_4` variants.
/// db events map to file events (`damage` -> `hit`, `die` -> `death`); a model without
/// files falls back to its prefix (`glarewolf_alpha` -> `glarewolf`).
pub(super) fn custom_voice(index: &FileIndex, model: &str, event: &str) -> Vec<String> {
    let event = match event {
        "damage" => "hit",
        "die" => "death",
        e => e,
    };
    let mut model = model.to_lowercase();
    loop {
        let lines: Vec<String> = std::iter::once(format!("npc_{model}_{event}.wav"))
            .chain((1..=4).map(|i| format!("npc_{model}_{event}_{i}.wav")))
            .filter(|n| index.resolve(n).is_some())
            .collect();
        match model.rsplit_once('_') {
            Some((head, _)) if lines.is_empty() && !head.is_empty() => model = head.to_string(),
            _ => return lines,
        }
    }
}

// ---------------------------------------------------------------- proximity loops

/// One line of `custom_assets/data/sprite_sounds.txt`: `sprite sound radius`.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SpriteSound {
    /// Lowercased sprite file stem; a trailing `*` matches any stem with that prefix.
    pattern: String,
    sound: String,
    radius: f32,
}

pub(super) fn parse_sprite_sounds(text: &str) -> Vec<SpriteSound> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let v: Vec<&str> = l.split_whitespace().collect();
            let [sprite, sound, radius] = v[..] else { return None };
            let sprite = sprite.to_lowercase();
            let pattern = match sprite.strip_suffix('*') {
                Some(p) => format!("{}*", p.rsplit_once('.').map_or(p, |s| s.0)),
                None => sprite.rsplit_once('.').map_or(sprite.as_str(), |s| s.0).to_string(),
            };
            let sound = if sound.contains('.') { sound.to_string() } else { format!("{sound}.wav") };
            Some(SpriteSound { pattern, sound, radius: radius.parse().ok()? })
        })
        .collect()
}

fn sprite_matches(pattern: &str, texture: &str) -> bool {
    let tex = texture.to_lowercase();
    let stem = tex.rsplit_once('.').map_or(tex.as_str(), |s| s.0);
    match pattern.strip_suffix('*') {
        Some(prefix) => stem.starts_with(prefix),
        None => stem == pattern,
    }
}

fn add_points(groups: &mut Vec<ProximityGroup>, sound: &str, radius: f32, points: Vec<Vec2>) {
    if points.is_empty() {
        return;
    }
    let i = groups.iter().position(|g| g.sound.eq_ignore_ascii_case(sound)).unwrap_or_else(|| {
        groups.push(ProximityGroup {
            sound: sound.to_string(),
            radius,
            points: Vec::new(),
            entity: None,
            gain: 0.0,
            target: 0.0,
        });
        groups.len() - 1
    });
    groups[i].radius = groups[i].radius.max(radius);
    groups[i].points.extend(points);
}

/// Emitters of our own sprites (`sprite_sounds`) and cairn fire at the `.cover` `C` cells.
pub(super) fn custom_proximity(
    groups: &mut Vec<ProximityGroup>,
    map: &MapFile,
    sprite_sounds: &[SpriteSound],
    cover: Option<&CoverGrid>,
) {
    for s in sprite_sounds {
        let textures: Vec<bool> = map.textures.iter().map(|t| sprite_matches(&s.pattern, t)).collect();
        let points = map
            .cells
            .iter()
            .filter(|c| c.layers.iter().flatten().any(|l| textures.get(l.texture as usize) == Some(&true)))
            .map(|c| Vec2::new(c.x as f32 + 0.5, c.y as f32 + 0.5))
            .collect();
        add_points(groups, &s.sound, s.radius, points);
    }
    if let Some(cover) = cover {
        let points = cover.cairns().into_iter().map(|(x, y)| Vec2::new(x, y)).collect();
        add_points(groups, CAIRN_FIRE, CAIRN_FIRE_RADIUS, points);
    }
}

/// Reads `sprite_sounds.txt` and the map's `.cover` and adds their emitters.
pub(super) fn load_custom_proximity(groups: &mut Vec<ProximityGroup>, map: &MapFile, data: &GameData, name: &str) {
    let sprite_sounds = std::fs::read_to_string(data.root.join("data/sprite_sounds.txt"))
        .map(|t| parse_sprite_sounds(&t))
        .unwrap_or_default();
    let cover = std::fs::read_to_string(data.root.join("maps").join(format!("{name}.cover")))
        .ok()
        .and_then(|t| CoverGrid::parse(&t));
    custom_proximity(groups, map, &sprite_sounds, cover.as_ref());
}

// ---------------------------------------------------------------- DUSK_GAZE_FAKE

/// `DUSK_GAZE_FAKE=strain,cover,openness` (cover: open|shade|shelter|cairn or `.sSC`) or
/// `DUSK_GAZE_FAKE=cycle` (cover changes every 10 s, strain and openness sweep).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum FakeGaze {
    Fixed { strain: f32, cover: CoverKind, openness: f32 },
    Cycle,
}

pub(super) fn parse_fake(s: &str) -> Option<FakeGaze> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("cycle") {
        return Some(FakeGaze::Cycle);
    }
    let v: Vec<&str> = s.split(',').map(str::trim).collect();
    let strain = v.first()?.parse::<f32>().ok()?.clamp(0.0, 100.0);
    let cover = match v.get(1).copied().unwrap_or("open") {
        "open" | "." | "0" => CoverKind::Open,
        "shade" | "s" | "1" => CoverKind::Shade,
        "shelter" | "S" | "2" => CoverKind::Shelter,
        "cairn" | "C" | "3" => CoverKind::Cairn,
        _ => return None,
    };
    let openness = v.get(2).map_or(Some(0.0), |o| o.parse::<f32>().ok())?.clamp(0.0, 1.0);
    Some(FakeGaze::Fixed { strain, cover, openness })
}

fn fake_view(fake: FakeGaze, t: f32) -> GazeView {
    let (strain, cover, openness) = match fake {
        FakeGaze::Fixed { strain, cover, openness } => (strain, cover, openness),
        FakeGaze::Cycle => {
            let cover =
                [CoverKind::Open, CoverKind::Shade, CoverKind::Shelter, CoverKind::Cairn][(t / 10.0) as usize % 4];
            let strain = 55.0 - 45.0 * (t * std::f32::consts::TAU / 30.0).cos();
            let openness = 1.0 - ((t / 20.0).fract() * 2.0 - 1.0).abs();
            (strain, cover, openness)
        }
    };
    GazeView {
        active: true,
        eye: if openness > 0.5 { EyeState::Open } else { EyeState::Lidded },
        openness,
        strain,
        cover,
        ..default()
    }
}

fn fake_gaze(time: Res<Time>, mut view: ResMut<GazeView>, mut fake: Local<Option<Option<FakeGaze>>>) {
    let fake = *fake.get_or_insert_with(|| {
        let v = std::env::var("DUSK_GAZE_FAKE").ok()?;
        let parsed = parse_fake(&v);
        match parsed {
            Some(f) => info!("audio: DUSK_GAZE_FAKE {f:?}"),
            None => warn!("audio: DUSK_GAZE_FAKE '{v}' not understood (strain,cover,openness | cycle)"),
        }
        parsed
    });
    if let Some(f) = fake {
        *view = fake_view(f, time.elapsed_secs());
    }
}

// ---------------------------------------------------------------- gaze layer

/// Loops of the gaze layer, in [`layer_targets`] order.
const LAYER_SOUNDS: [&str; 3] = ["amb_open_sky.wav", "amb_shelter.wav", "amb_eye_open.wav"];

#[derive(Default)]
struct LayerLoop {
    entity: Option<Entity>,
    gain: f32,
}

#[derive(Resource, Default)]
struct GazeLayer {
    loops: [LayerLoop; 3],
    cover: Option<CoverKind>,
    heart_wait: f32,
    cue_wait: f32,
}

#[derive(Component)]
struct GazeLoop;

/// DESIGN: target gains of (open sky, shelter, open Eye) for the player's cover. Shelter
/// keeps a little of the sky wind (muffled by volume, there is no filter), and the open Eye
/// presses through everything except deep cover.
pub(super) fn layer_targets(view: &GazeView) -> [f32; 3] {
    if !view.active {
        return [0.0; 3];
    }
    let (sky, shelter, eye_through) = match view.cover {
        CoverKind::Open => (1.0, 0.0, 1.0),
        CoverKind::Shade => (0.55, 0.3, 0.75),
        CoverKind::Shelter => (0.12, 1.0, 0.35),
        CoverKind::Cairn => (0.2, 0.75, 0.3),
    };
    let eye = view.openness.clamp(0.0, 1.0) * eye_through;
    let eye = if eye < 0.02 { 0.0 } else { eye }; // a half-lidded Eye is silent
    [sky * (1.0 - 0.3 * eye), shelter, eye]
}

#[allow(clippy::too_many_arguments)]
fn drive_gaze_layer(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    view: Res<GazeView>,
    mut layer: ResMut<GazeLayer>,
    mut sinks: Query<&mut AudioSink, With<GazeLoop>>,
) {
    let targets = layer_targets(&view);
    let cover = view.active.then_some(view.cover);
    if cover != layer.cover {
        settings.log(format_args!(
            "gaze layer: cover {cover:?}, targets sky {:.2} shelter {:.2} eye {:.2}",
            targets[0], targets[1], targets[2]
        ));
        layer.cover = cover;
    }
    let step = time.delta_secs() / LAYER_FADE;
    for (i, l) in layer.loops.iter_mut().enumerate() {
        let target = targets[i];
        l.gain += (target - l.gain).clamp(-step, step);
        match l.entity {
            None if target > 0.0 && settings.music_gain() > 0.0 => {
                let Some(path) = resolve_sound(&data.index, LAYER_SOUNDS[i]) else { continue };
                l.entity = Some(
                    commands
                        .spawn((
                            GazeLoop,
                            AudioPlayer::new(assets.load(path.to_string())),
                            PlaybackSettings::LOOP.with_volume(Volume::Linear(0.0)),
                        ))
                        .id(),
                );
                settings.log(format_args!("gaze layer {} starts", LAYER_SOUNDS[i]));
            }
            Some(e) if target <= 0.0 && l.gain <= 0.0 => {
                commands.entity(e).despawn();
                l.entity = None;
                settings.log(format_args!("gaze layer {} stops", LAYER_SOUNDS[i]));
            }
            Some(e) => {
                if let Ok(mut sink) = sinks.get_mut(e) {
                    sink.set_volume(Volume::Linear(l.gain.max(0.0) * settings.music_gain()));
                }
            }
            None => {}
        }
    }
}

/// DESIGN: heartbeat period (s) and volume for a strain level; none below 40.
pub(super) fn heartbeat(strain: f32) -> Option<(f32, f32)> {
    (strain >= 40.0).then(|| {
        let k = ((strain - 40.0) / 60.0).clamp(0.0, 1.0);
        (1.5 - 0.9 * k, 0.35 + 0.65 * k)
    })
}

/// Plays one non-positional effect at `volume` (0..1 of the effects volume).
fn spawn_cue(
    commands: &mut Commands,
    assets: &AssetServer,
    data: &GameData,
    settings: &AudioSettings,
    name: &str,
    volume: f32,
) {
    let gain = volume * settings.sfx_gain();
    if gain <= 0.0 {
        return;
    }
    let Some(path) = resolve_sfx(&data.index, name) else { return };
    commands.spawn((
        SfxVoice,
        AudioPlayer::new(assets.load(path.to_string())),
        PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
    ));
    settings.log(format_args!("cue {name} (volume {volume:.2})"));
}

/// Heartbeat retriggered faster and louder as strain rises past 40; above 70 an occasional
/// whisper or strained breath.
#[allow(clippy::too_many_arguments)]
fn strain_cues(
    mut commands: Commands,
    time: Res<Time>,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    view: Res<GazeView>,
    mut rng: ResMut<Rng>,
    mut layer: ResMut<GazeLayer>,
) {
    let dt = time.delta_secs();
    let strain = if view.active { view.strain } else { 0.0 };
    match heartbeat(strain) {
        Some((period, volume)) => {
            layer.heart_wait -= dt;
            if layer.heart_wait <= 0.0 {
                layer.heart_wait = period;
                spawn_cue(&mut commands, &assets, &data, &settings, "strain_heartbeat.wav", volume);
            }
        }
        None => layer.heart_wait = 0.0,
    }
    if strain >= 70.0 {
        layer.cue_wait -= dt;
        if layer.cue_wait <= 0.0 {
            // DESIGN: every 5..13 s, sooner the worse it gets; the first one 3 s after crossing 70
            let k = ((strain - 70.0) / 30.0).clamp(0.0, 1.0);
            let name = if rng.next().is_multiple_of(2) { "strain_whisper.wav" } else { "strain_breath.wav" };
            spawn_cue(&mut commands, &assets, &data, &settings, name, 0.5 + 0.5 * k);
            let r = (rng.next() % 1000) as f32 / 1000.0;
            layer.cue_wait = (13.0 - 6.0 * k) * (0.6 + 0.4 * r);
        }
    } else {
        layer.cue_wait = 3.0;
    }
}

// ---------------------------------------------------------------- footsteps + greets

/// Player footsteps: stone under deep shelter (roofed lanes, the cairn), dirt elsewhere.
#[allow(clippy::too_many_arguments)]
fn footsteps(
    mut commands: Commands,
    data: Res<GameData>,
    assets: Res<AssetServer>,
    settings: Res<AudioSettings>,
    view: Res<GazeView>,
    mut rng: ResMut<Rng>,
    player: Query<&Unit, With<Player>>,
    mut walked: Local<(Option<Vec2>, f32)>,
) {
    let Ok(unit) = player.single() else { return };
    let (last, dist) = &mut *walked;
    let moved = last.map_or(0.0, |l| l.distance(unit.pos));
    *last = Some(unit.pos);
    if unit.anim != "run" || moved > 3.0 {
        *dist = STEP_CELLS * 0.6; // first step comes quickly after starting to run
        return;
    }
    *dist += moved;
    if *dist < STEP_CELLS {
        return;
    }
    *dist -= STEP_CELLS;
    let surface = match view.cover {
        CoverKind::Shelter | CoverKind::Cairn if view.active => "stone",
        _ => "dirt",
    };
    let name = format!("foot_{surface}_{}.wav", 1 + rng.next() % 4);
    let Some(path) = resolve_sfx(&data.index, &name) else { return };
    let gain = STEP_GAIN * settings.sfx_gain();
    if gain > 0.0 {
        commands.spawn((
            SfxVoice,
            AudioPlayer::new(assets.load(path.to_string())),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(gain)),
        ));
    }
}

/// Selecting an NPC with a custom greet (`npc_<model>_greet.wav`) plays it at the NPC.
fn greet_on_target(
    time: Res<Time>,
    data: Res<GameData>,
    mut rng: ResMut<Rng>,
    targeted: Query<(Entity, &Npc), Added<Targeted>>,
    mut last: Local<HashMap<Entity, f32>>,
    mut out: MessageWriter<super::PlaySfx>,
) {
    let now = time.elapsed_secs();
    for (e, npc) in &targeted {
        let Some(model) = super::npc_model(&data, npc) else { continue };
        if last.get(&e).is_some_and(|t| now - t < GREET_COOLDOWN) {
            continue;
        }
        let lines = custom_voice(&data.index, model, "greet");
        if let Some(line) = rng.pick(&lines) {
            last.insert(e, now);
            out.write(super::PlaySfx { name: line.clone(), at: SfxAt::Unit(e), delay: 0.0 });
        }
    }
    if last.len() > 256 {
        last.retain(|_, t| now - *t < GREET_COOLDOWN);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name of the demo-plan sound contract exists in `custom_assets` and decodes.
    #[test]
    fn contract_sounds_decode() {
        use bevy::audio::{AudioSource, Decodable};
        let dir = dusk_formats::content_root().join("content/custom/sfx");
        let mut names: Vec<String> = [
            "gaze_open",
            "gaze_close",
            "gaze_spot_enter",
            "strain_heartbeat",
            "strain_breath",
            "strain_whisper",
            "strain_overwhelm",
            "cairn_kindle",
            "cairn_rest",
            "quest_accept",
            "quest_progress",
            "quest_complete",
            "dialogue_open",
            "title_sting",
            "end_sting",
            "hit_heavy",
            "amb_open_sky",
            "amb_shelter",
            "amb_eye_open",
            "loop_cairn_fire",
        ]
        .iter()
        .map(|n| n.to_string())
        .collect();
        for model in ["glarewolf", "stooped", "hollowed_warden"] {
            names.extend(["aggro", "attack", "hit", "death"].map(|e| format!("npc_{model}_{e}")));
        }
        for model in ["cairnkeeper", "lightworker", "lowshade_guard"] {
            names.push(format!("npc_{model}_greet"));
        }
        for name in names {
            let bytes = std::fs::read(dir.join(format!("{name}.wav"))).unwrap_or_else(|e| panic!("{name}: {e}"));
            let source = AudioSource { bytes: bytes.into() };
            assert!(source.decoder().take(4096).count() > 0, "{name} does not decode");
        }
    }

    #[test]
    fn fake_gaze_env() {
        assert_eq!(
            parse_fake("75, shelter, 0.5"),
            Some(FakeGaze::Fixed { strain: 75.0, cover: CoverKind::Shelter, openness: 0.5 })
        );
        assert_eq!(
            parse_fake("120,s"),
            Some(FakeGaze::Fixed { strain: 100.0, cover: CoverKind::Shade, openness: 0.0 })
        );
        assert_eq!(parse_fake("cycle"), Some(FakeGaze::Cycle));
        assert_eq!(parse_fake("x,open"), None);
        assert_eq!(parse_fake("10,roof"), None);
        let v = fake_view(FakeGaze::Cycle, 25.0);
        assert!(v.active && v.cover == CoverKind::Shelter && (0.0..=100.0).contains(&v.strain));
    }

    #[test]
    fn layer_mix() {
        let mut v = GazeView { active: false, ..default() };
        assert_eq!(layer_targets(&v), [0.0; 3]);
        v.active = true;
        assert_eq!(layer_targets(&v), [1.0, 0.0, 0.0]);
        v.cover = CoverKind::Shelter;
        v.openness = 1.0;
        let [sky, shelter, eye] = layer_targets(&v);
        assert!(sky < 0.2 && shelter == 1.0 && eye > 0.0 && eye < 0.5);
        v.cover = CoverKind::Open;
        assert_eq!(layer_targets(&v)[2], 1.0);
    }

    #[test]
    fn heartbeat_rises_with_strain() {
        assert_eq!(heartbeat(39.0), None);
        let (p40, v40) = heartbeat(40.0).unwrap();
        let (p100, v100) = heartbeat(100.0).unwrap();
        assert!(p100 < p40 && v100 > v40 && v100 <= 1.0);
    }

    #[test]
    fn custom_voices_and_bare_names() {
        let mut index = FileIndex::default();
        for n in ["npc_glarewolf_hit.wav", "npc_glarewolf_death.wav", "npc_stooped_attack_2.wav", "gaze_open.wav"] {
            index.insert(n, &format!("content/custom/sfx/{n}"));
        }
        assert_eq!(custom_voice(&index, "Glarewolf", "damage"), ["npc_glarewolf_hit.wav"]);
        assert_eq!(custom_voice(&index, "glarewolf_alpha", "die"), ["npc_glarewolf_death.wav"]);
        assert_eq!(custom_voice(&index, "stooped", "attack"), ["npc_stooped_attack_2.wav"]);
        assert!(custom_voice(&index, "cairnkeeper", "greet").is_empty());
        assert_eq!(resolve_sfx(&index, "gaze_open"), Some("content/custom/sfx/gaze_open.wav"));
        assert_eq!(resolve_sfx(&index, "gaze_open.wav"), Some("content/custom/sfx/gaze_open.wav"));
        assert_eq!(resolve_sfx(&index, "gaze_close"), None);
    }

    #[test]
    fn sprite_sounds_and_cairns() {
        use dusk_formats::map::{Cell, TileLayer};
        let s = parse_sprite_sounds("# c\ncustom_cairn* loop_cairn_fire 6\nBrazier.png fire.ogg 3\nbad line\n");
        assert_eq!(s.len(), 2);
        assert_eq!((s[0].pattern.as_str(), s[0].sound.as_str()), ("custom_cairn*", "loop_cairn_fire.wav"));
        assert_eq!(s[1].pattern, "brazier");
        assert!(sprite_matches("custom_cairn*", "Custom_Cairn_lit_02.png"));
        assert!(sprite_matches("brazier", "brazier.png") && !sprite_matches("brazier", "brazier_2.png"));
        let layer = |t| Some(TileLayer { texture: t, param: 0 });
        let map = MapFile {
            size: 13,
            textures: vec!["grass.png".into(), "custom_cairn_01.png".into()],
            cells: vec![
                Cell { x: 2, y: 3, flags: 0, layers: [layer(0), None, layer(1)] },
                Cell { x: 5, y: 5, flags: 0, layers: [layer(0), None, None] },
            ],
            terrain_textures: vec![],
            terrain: vec![],
            zones: vec![],
            areas: vec![],
        };
        let cover = CoverGrid::parse("3 2\n.sS\nC..\n").unwrap();
        let mut groups = Vec::new();
        custom_proximity(&mut groups, &map, &s, Some(&cover));
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].sound, "loop_cairn_fire.wav");
        assert_eq!(groups[0].points, [Vec2::new(2.5, 3.5), Vec2::new(0.5, 1.5)]);
        assert_eq!(groups[0].radius, 6.0);
    }
}
