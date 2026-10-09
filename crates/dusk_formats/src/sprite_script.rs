//! `scripts/npc/*.txt`, `scripts/player/*.txt` — 8-directional
//! sprite-sheet animations for units and paper-doll gear.
//!
//! ```text
//! image=npc_goblin.png
//! [stance]
//! frames=4
//! duration=800ms
//! type=back_forth            // looped | play_once | back_forth
//! frame=F,D,x,y,w,h,px,py    // F frame idx, D direction 0..8, rect in sheet,
//!                            // (px,py) pivot (feet) inside rect; may be negative
//! # comment
//! ```

use std::collections::BTreeMap;

pub const DIRECTIONS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlayMode {
    #[default]
    Looped,
    PlayOnce,
    /// Ping-pong: 0,1,..,n-1,n-2,..,1
    BackForth,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FrameRect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub pivot_x: i32,
    pub pivot_y: i32,
}

#[derive(Debug, Clone, Default)]
pub struct Animation {
    pub duration_ms: u32,
    pub mode: PlayMode,
    /// `hit=` (our generated attack animations): ms into the animation where the blow lands.
    pub hit_ms: Option<u32>,
    /// `frames[frame][direction]`
    pub frames: Vec<[FrameRect; DIRECTIONS]>,
}

impl Animation {
    /// Frame index for elapsed time, honoring play mode.
    pub fn frame_at(&self, elapsed_ms: u32) -> usize {
        let n = self.frames.len().max(1);
        if n == 1 || self.duration_ms == 0 {
            return 0;
        }
        let per = (self.duration_ms / n as u32).max(1);
        let step = (elapsed_ms / per) as usize;
        match self.mode {
            PlayMode::Looped => step % n,
            PlayMode::PlayOnce => step.min(n - 1),
            PlayMode::BackForth => {
                let period = 2 * n - 2;
                let s = step % period;
                if s < n { s } else { period - s }
            }
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SpriteScript {
    pub image: String,
    pub animations: BTreeMap<String, Animation>,
}

impl SpriteScript {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let mut script = SpriteScript::default();
        let mut current: Option<String> = None;

        for (lineno, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
                script.animations.entry(name.to_string()).or_default();
                current = Some(name.to_string());
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                anyhow::bail!("line {}: expected key=value: {raw:?}", lineno + 1);
            };
            if key == "image" {
                script.image = value.to_string();
                continue;
            }
            let Some(anim) = current.as_ref().and_then(|c| script.animations.get_mut(c)) else {
                anyhow::bail!("line {}: {key} outside of [section]", lineno + 1);
            };
            match key {
                "frames" => {} // derived from frame= lines
                "duration" => anim.duration_ms = parse_duration_ms(value)?,
                "hit" => anim.hit_ms = Some(parse_duration_ms(value)?),
                "type" => {
                    anim.mode = match value {
                        "looped" => PlayMode::Looped,
                        "play_once" => PlayMode::PlayOnce,
                        "back_forth" => PlayMode::BackForth,
                        other => anyhow::bail!("line {}: unknown type {other}", lineno + 1),
                    }
                }
                "frame" => {
                    let v: Vec<i64> = value.split(',').map(|s| s.trim().parse()).collect::<Result<_, _>>()?;
                    anyhow::ensure!(v.len() == 8, "line {}: frame needs 8 values", lineno + 1);
                    let (f, d) = (v[0] as usize, v[1] as usize);
                    anyhow::ensure!(d < DIRECTIONS, "line {}: direction {d}", lineno + 1);
                    if anim.frames.len() <= f {
                        anim.frames.resize(f + 1, [FrameRect::default(); DIRECTIONS]);
                    }
                    anim.frames[f][d] = FrameRect {
                        x: v[2] as u32,
                        y: v[3] as u32,
                        w: v[4] as u32,
                        h: v[5] as u32,
                        pivot_x: v[6] as i32,
                        pivot_y: v[7] as i32,
                    };
                }
                _ => {} // tolerate unknown keys
            }
        }
        Ok(script)
    }
}

/// `800ms`, `1s`, `1.5s` or bare milliseconds.
fn parse_duration_ms(v: &str) -> anyhow::Result<u32> {
    Ok(if let Some(ms) = v.strip_suffix("ms") {
        ms.trim().parse()?
    } else if let Some(s) = v.strip_suffix('s') {
        (s.trim().parse::<f32>()? * 1000.0) as u32
    } else {
        v.trim().parse()?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_steps() {
        let s = SpriteScript::parse(
            "image=a.png\n\n[run]\nframes=3\nduration=300ms\ntype=back_forth\nframe=0,0,1,2,3,4,-5,6\nframe=2,7,0,0,1,1,0,0\n# [x]\n",
        )
        .unwrap();
        let run = &s.animations["run"];
        assert_eq!(s.image, "a.png");
        assert_eq!(run.frames.len(), 3);
        assert_eq!(run.frames[0][0].pivot_x, -5);
        let seq: Vec<_> = (0..5).map(|i| run.frame_at(i * 100)).collect();
        assert_eq!(seq, [0, 1, 2, 1, 0]);
        assert_eq!(run.hit_ms, None);
    }

    #[test]
    fn parses_hit_time() {
        let s = SpriteScript::parse("image=a.png\n[swing]\nduration=640ms\ntype=play_once\nhit=160ms\n").unwrap();
        assert_eq!(s.animations["swing"].hit_ms, Some(160));
    }
}
