//! Player settings (Options menu), persisted as `settings.ini`:
//! `DUSK_SETTINGS` if set, else `%APPDATA%\Duskhollow\settings.ini` (Windows) /
//! `$XDG_CONFIG_HOME/duskhollow/settings.ini` / `~/.config/duskhollow/settings.ini`.
//!
//! ```ini
//! [audio]
//! master = 100          ; 0..100
//! music = 15
//! effects = 20
//! [display]
//! fullscreen = 0        ; borderless fullscreen on the current monitor
//! resolution = 1280x720 ; windowed size
//! vsync = 1
//! ui_scale = 1.00
//! show_fps = 0
//! [game]
//! screen_shake = 1
//! hit_stop = 1
//! [player]
//! name = Wanderer
//! class = 1
//! map = duskhollow
//! servers = 127.0.0.1:16383   ; recently joined, newest first
//! ```
//!
//! A menu launch applies the file at start-up; command-line / debug launches ignore it until
//! the Options menu changes something (so screenshots and tests behave as before).

use crate::{audio::AudioSettings, feel::FeelSettings, state::Launch};
use bevy::diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin};
use bevy::prelude::*;
use bevy::window::{MonitorSelection, PresentMode, PrimaryWindow, WindowMode};
use std::path::PathBuf;

pub const RESOLUTIONS: [(u32, u32); 6] =
    [(1280, 720), (1366, 768), (1600, 900), (1920, 1080), (2560, 1440), (1024, 768)];
pub const UI_SCALES: [f32; 5] = [0.75, 1.0, 1.25, 1.5, 2.0];
pub const MAX_SERVERS: usize = 5;
pub const DEFAULT_MAP: &str = "duskhollow";

#[derive(Resource, Debug, Clone, PartialEq)]
pub struct Settings {
    /// 0..=100 each.
    pub master: u32,
    pub music: u32,
    pub effects: u32,
    pub fullscreen: bool,
    pub resolution: (u32, u32),
    pub vsync: bool,
    pub ui_scale: f32,
    pub show_fps: bool,
    pub screen_shake: bool,
    pub hit_stop: bool,
    pub name: String,
    pub class: u8,
    pub map: String,
    pub servers: Vec<String>,
    /// The audio volumes came from the file (else they are taken from the audio defaults).
    pub audio_from_file: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            master: 100,
            music: 15,
            effects: 20,
            fullscreen: false,
            resolution: (1280, 720),
            vsync: true,
            ui_scale: 1.0,
            show_fps: false,
            screen_shake: true,
            hit_stop: true,
            name: String::new(),
            class: 1,
            map: DEFAULT_MAP.into(),
            servers: Vec::new(),
            audio_from_file: false,
        }
    }
}

pub fn path() -> PathBuf {
    if let Some(p) = std::env::var_os("DUSK_SETTINGS") {
        return p.into();
    }
    let base = std::env::var_os("APPDATA")
        .map(|d| PathBuf::from(d).join("Duskhollow"))
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|d| PathBuf::from(d).join("duskhollow")))
        .or_else(|| std::env::var_os("HOME").map(|d| PathBuf::from(d).join(".config/duskhollow")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("settings.ini")
}

/// Player names follow the server's rule: letters and digits, at most this many.
pub const MAX_NAME: usize = 15;

pub fn clean_name(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).take(MAX_NAME).collect()
}

impl Settings {
    pub fn load() -> Self {
        match std::fs::read_to_string(path()) {
            Ok(text) => Self::parse(&text),
            Err(_) => Self::default(),
        }
    }

    pub fn parse(text: &str) -> Self {
        let mut s = Self::default();
        let mut section = String::new();
        for line in text.lines() {
            let line = line.split(';').next().unwrap_or("").trim();
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                section = name.trim().to_ascii_lowercase();
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
            let pct = |d: u32| v.parse::<u32>().map(|x| x.min(100)).unwrap_or(d);
            let flag = |d: bool| match v {
                "1" | "true" | "yes" | "on" => true,
                "0" | "false" | "no" | "off" => false,
                _ => d,
            };
            match (section.as_str(), k.as_str()) {
                ("audio", "master") => s.master = pct(s.master),
                ("audio", "music") => {
                    s.music = pct(s.music);
                    s.audio_from_file = true;
                }
                ("audio", "effects") => {
                    s.effects = pct(s.effects);
                    s.audio_from_file = true;
                }
                ("display", "fullscreen") => s.fullscreen = flag(s.fullscreen),
                ("display", "resolution") => {
                    if let Some((w, h)) = v.split_once('x')
                        && let (Ok(w), Ok(h)) = (w.trim().parse::<u32>(), h.trim().parse::<u32>())
                    {
                        s.resolution = (w.clamp(640, 7680), h.clamp(480, 4320));
                    }
                }
                ("display", "vsync") => s.vsync = flag(s.vsync),
                ("display", "ui_scale") => s.ui_scale = v.parse::<f32>().map_or(s.ui_scale, |x| x.clamp(0.5, 3.0)),
                ("display", "show_fps") => s.show_fps = flag(s.show_fps),
                ("game", "screen_shake") => s.screen_shake = flag(s.screen_shake),
                ("game", "hit_stop") => s.hit_stop = flag(s.hit_stop),
                ("player", "name") => s.name = clean_name(v),
                ("player", "class") => s.class = v.parse::<u8>().ok().filter(|c| (1..=4).contains(c)).unwrap_or(1),
                ("player", "map") if !v.is_empty() => s.map = v.to_string(),
                ("player", "servers") => {
                    s.servers = v
                        .split(',')
                        .map(str::trim)
                        .filter(|a| !a.is_empty())
                        .take(MAX_SERVERS)
                        .map(String::from)
                        .collect()
                }
                _ => {}
            }
        }
        s
    }

    pub fn to_ini(&self) -> String {
        let b = |v: bool| if v { 1 } else { 0 };
        format!(
            "; Duskhollow settings (written by the Options menu)\n\
             [audio]\nmaster = {}\nmusic = {}\neffects = {}\n\n\
             [display]\nfullscreen = {}\nresolution = {}x{}\nvsync = {}\nui_scale = {:.2}\nshow_fps = {}\n\n\
             [game]\nscreen_shake = {}\nhit_stop = {}\n\n\
             [player]\nname = {}\nclass = {}\nmap = {}\nservers = {}\n",
            self.master,
            self.music,
            self.effects,
            b(self.fullscreen),
            self.resolution.0,
            self.resolution.1,
            b(self.vsync),
            self.ui_scale,
            b(self.show_fps),
            b(self.screen_shake),
            b(self.hit_stop),
            self.name,
            self.class,
            self.map,
            self.servers.join(", "),
        )
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        match std::fs::write(&p, self.to_ini()) {
            Ok(()) => debug!("settings saved to {}", p.display()),
            Err(e) => warn!("cannot save settings to {}: {e}", p.display()),
        }
    }

    /// Moves `addr` to the front of the recent servers.
    pub fn remember_server(&mut self, addr: &str) {
        self.servers.retain(|a| a != addr);
        self.servers.insert(0, addr.to_string());
        self.servers.truncate(MAX_SERVERS);
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(FrameTimeDiagnosticsPlugin::default())
            .add_systems(Startup, spawn_fps.after(crate::combat_ui::load_font))
            .add_systems(Update, (apply, update_fps));
    }
}

/// Applies the settings whenever they change (and once at start-up for a menu launch).
#[allow(clippy::too_many_arguments)]
fn apply(
    mut settings: ResMut<Settings>,
    launch: Res<Launch>,
    mut first: Local<bool>,
    mut audio: ResMut<AudioSettings>,
    mut feel: ResMut<FeelSettings>,
    mut ui_scale: ResMut<UiScale>,
    mut window: Query<&mut Window, With<PrimaryWindow>>,
    mut last: Local<Option<Settings>>,
) {
    let startup = !*first;
    *first = true;
    if startup && !settings.audio_from_file {
        // No volumes in the file: start from what the audio module chose (config.ini, env).
        let s = settings.bypass_change_detection();
        s.music = (audio.music_volume * 100.0).round() as u32;
        s.effects = (audio.sfx_volume * 100.0).round() as u32;
    }
    if !settings.is_changed() || (startup && !launch.is_menu()) {
        if startup {
            *last = Some(settings.clone());
        }
        return;
    }
    let prev = last.replace(settings.clone());
    let settings = &*settings;
    let changed = |f: fn(&Settings) -> bool| prev.as_ref().is_none_or(|p| f(p) != f(settings));

    audio.master_volume = settings.master as f32 / 100.0;
    audio.music_volume = settings.music as f32 / 100.0;
    audio.sfx_volume = settings.effects as f32 / 100.0;
    *feel = FeelSettings { shake: settings.screen_shake, hitstop: settings.hit_stop };
    if (ui_scale.0 - settings.ui_scale).abs() > 1e-3 {
        ui_scale.0 = settings.ui_scale;
    }
    let Ok(mut w) = window.single_mut() else { return };
    let want_mode = if settings.fullscreen {
        WindowMode::BorderlessFullscreen(MonitorSelection::Current)
    } else {
        WindowMode::Windowed
    };
    if w.mode != want_mode {
        w.mode = want_mode;
    }
    let res_changed =
        prev.as_ref().is_none_or(|p| p.resolution != settings.resolution || p.fullscreen != settings.fullscreen);
    if !settings.fullscreen && res_changed {
        let (rw, rh) = settings.resolution;
        w.resolution.set(rw as f32, rh as f32);
    }
    let present = if settings.vsync { PresentMode::AutoVsync } else { PresentMode::AutoNoVsync };
    if changed(|s| s.vsync) && w.present_mode != present {
        w.present_mode = present;
    }
}

#[derive(Component)]
struct FpsText;

fn spawn_fps(mut commands: Commands, font: Res<crate::combat_ui::UiFont>) {
    commands.spawn((
        Text::new(""),
        TextFont { font: font.0.clone().into(), font_size: 13.0.into(), ..default() },
        TextColor(Color::srgb(0.85, 0.78, 0.6)),
        TextShadow { offset: Vec2::splat(1.0), color: Color::BLACK.with_alpha(0.9) },
        Node { position_type: PositionType::Absolute, right: Val::Px(10.0), bottom: Val::Px(48.0), ..default() },
        GlobalZIndex(500),
        Visibility::Hidden,
        FpsText,
        crate::menu::KeepOnMenu,
    ));
}

fn update_fps(
    settings: Res<Settings>,
    diagnostics: Res<DiagnosticsStore>,
    mut text: Query<(&mut Text, &mut Visibility), With<FpsText>>,
    mut since: Local<f32>,
    time: Res<Time>,
) {
    let Ok((mut t, mut vis)) = text.single_mut() else { return };
    vis.set_if_neq(if settings.show_fps { Visibility::Inherited } else { Visibility::Hidden });
    *since += time.delta_secs();
    if !settings.show_fps || *since < 0.25 {
        return;
    }
    *since = 0.0;
    if let Some(fps) = diagnostics.get(&FrameTimeDiagnosticsPlugin::FPS).and_then(|d| d.smoothed()) {
        t.0 = format!("{fps:.0} fps");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let mut s = Settings { name: "Ash".into(), class: 3, master: 80, fullscreen: true, ..Default::default() };
        s.remember_server("10.0.0.2:16383");
        s.remember_server("127.0.0.1:16383");
        s.remember_server("10.0.0.2:16383");
        let back = Settings::parse(&s.to_ini());
        assert_eq!(back.name, "Ash");
        assert_eq!(back.class, 3);
        assert_eq!(back.master, 80);
        assert!(back.fullscreen && back.audio_from_file);
        assert_eq!(back.servers, ["10.0.0.2:16383", "127.0.0.1:16383"]);
        assert_eq!(back.resolution, (1280, 720));
    }

    #[test]
    fn names_follow_the_server_rule() {
        assert_eq!(clean_name("Ash wood!"), "Ashwood");
        assert_eq!(clean_name("abcdefghijklmnopqrst"), "abcdefghijklmno");
    }
}
