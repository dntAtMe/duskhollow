//! `scripts/animation/*.sa` — spell/effect flipbooks, one PNG per frame
//! (`<filename>_<n>.png` in `content/animations/*`).
//!
//! ```text
//! ratio=4
//! size=192          // logical canvas size; frames are trimmed
//! filename=cast_001
//! loopstart=0
//! loopend=0
//! delay=50          // ms per frame
//!
//! 1,39,43           // frame n, offset of trimmed image inside canvas
//! ```
//! `ratio` meaning unknown yet (scale divisor?).

#[derive(Debug, Clone, Default)]
pub struct SpriteAnim {
    pub ratio: u32,
    pub size: u32,
    pub filename: String,
    pub loop_start: u32,
    pub loop_end: u32,
    pub delay_ms: u32,
    /// (frame number, x, y)
    pub frames: Vec<(u32, i32, i32)>,
}

impl SpriteAnim {
    pub fn parse(text: &str) -> anyhow::Result<Self> {
        let mut a = SpriteAnim::default();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            if let Some((k, v)) = line.split_once('=') {
                match k {
                    "ratio" => a.ratio = v.parse()?,
                    "size" => a.size = v.parse()?,
                    "filename" => a.filename = v.to_string(),
                    "loopstart" => a.loop_start = v.parse()?,
                    "loopend" => a.loop_end = v.parse()?,
                    "delay" => a.delay_ms = v.parse()?,
                    _ => {}
                }
            } else {
                let v: Vec<i32> = line.split(',').map(|s| s.trim().parse()).collect::<Result<_, _>>()?;
                anyhow::ensure!(v.len() == 3, "bad frame line {line:?}");
                a.frames.push((v[0] as u32, v[1], v[2]));
            }
        }
        Ok(a)
    }

    pub fn frame_file(&self, n: u32) -> String {
        format!("{}_{n}.png", self.filename)
    }
}
