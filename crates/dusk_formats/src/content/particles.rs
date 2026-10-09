//! Particle system definitions by name (map emitters and spell kits): `data/particles.txt`.
//!
//! ```text
//! [campfire]
//! sprite=10            # atlas cell 0..15 of fx_particles.png (4x4, row-major)
//! blend=add            # add | alpha
//! emission=40          # particles per second
//! lifetime=-1          # seconds the system emits; <= 0 = until stopped
//! life=1.6,1.9         # particle life, seconds
//! direction=0          # degrees, 0 = up on screen, clockwise
//! spread=360           # degrees, full cone width
//! relative=0           # 1: add the emitter's direction of movement
//! speed=-5,0           # px/s
//! gravity=0,-40        # px/s^2, + = down
//! radial=-14,0         # px/s^2 away from the emitter
//! tangential=0,0       # px/s^2
//! size=0.3,0.5,0       # start, end, variation (multiples of 32 px)
//! spin=0,0,0           # start, end, variation (not drawn)
//! color_start=1,0.7,0,0
//! color_end=1,0,0,0.4  # RGBA 0..1
//! color_var=0          # 0..1: how far towards color_end a particle's start colour may be
//! alpha_var=1          # the same for alpha
//! ```
//! Missing keys default to 0 (`blend` to `add`, `lifetime` to -1). See [`crate::psi`].

use super::sections::{self, Section};
use crate::psi::ParticleSystemInfo;
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::Path;

/// Keys of a `data/particles.txt` section.
pub const PARTICLE_KEYS: &[&str] = &[
    "sprite",
    "blend",
    "emission",
    "lifetime",
    "life",
    "direction",
    "spread",
    "relative",
    "speed",
    "gravity",
    "radial",
    "tangential",
    "size",
    "spin",
    "color_start",
    "color_end",
    "color_var",
    "alpha_var",
];

/// Lookup key of a particle system name: lowercase, without a `.psi` extension.
pub fn key(name: &str) -> String {
    let lower = name.trim().to_lowercase();
    lower.strip_suffix(".psi").map(str::to_string).unwrap_or(lower)
}

/// Every particle system of `data/particles.txt`, keyed by [`key`].
pub fn load(root: &Path) -> anyhow::Result<HashMap<String, ParticleSystemInfo>> {
    parse(&sections::load(&root.join("data/particles.txt"))?)
}

pub fn parse(sections: &[Section]) -> anyhow::Result<HashMap<String, ParticleSystemInfo>> {
    let mut out = HashMap::new();
    for s in sections {
        if s.kind.is_some() {
            bail!("line {}: particle sections are plain [name]", s.line);
        }
        let info = parse_one(s).with_context(|| format!("[{}] (line {})", s.id, s.line))?;
        out.insert(key(&s.id), info);
    }
    Ok(out)
}

fn floats<const N: usize>(s: &Section, k: &str) -> anyhow::Result<[f32; N]> {
    let mut out = [0.0; N];
    let Some(v) = s.get(k) else { return Ok(out) };
    let items: Vec<&str> = v.split(',').map(str::trim).collect();
    if items.len() != N {
        bail!("{k}: expected {N} comma-separated numbers, got {v:?}");
    }
    for (o, item) in out.iter_mut().zip(items) {
        *o = item.parse().with_context(|| format!("{k}: {item:?}"))?;
    }
    Ok(out)
}

fn parse_one(s: &Section) -> anyhow::Result<ParticleSystemInfo> {
    let num = |k: &str, d: f32| -> anyhow::Result<f32> {
        s.get(k).map_or(Ok(d), |v| v.parse().with_context(|| format!("{k}: {v:?}")))
    };
    let cell = num("sprite", 0.0)? as u32;
    if cell > 15 {
        bail!("sprite: cell {cell} outside the 4x4 atlas");
    }
    let blend_add = match s.get("blend").unwrap_or("add") {
        "add" => true,
        "alpha" => false,
        v => bail!("blend: {v:?} (add | alpha)"),
    };
    let [life_min, life_max] = floats(s, "life")?;
    let [speed_min, speed_max] = floats(s, "speed")?;
    let [gravity_min, gravity_max] = floats(s, "gravity")?;
    let [radial_min, radial_max] = floats(s, "radial")?;
    let [tangential_min, tangential_max] = floats(s, "tangential")?;
    let [size_start, size_end, size_var] = floats(s, "size")?;
    let [spin_start, spin_end, spin_var] = floats(s, "spin")?;
    Ok(ParticleSystemInfo {
        cell,
        blend_add,
        emission: num("emission", 0.0)? as i32,
        lifetime: num("lifetime", -1.0)?,
        particle_life_min: life_min,
        particle_life_max: life_max,
        direction: num("direction", 0.0)?.to_radians(),
        spread: num("spread", 0.0)?.to_radians(),
        relative: num("relative", 0.0)? != 0.0,
        speed_min,
        speed_max,
        gravity_min,
        gravity_max,
        radial_accel_min: radial_min,
        radial_accel_max: radial_max,
        tangential_accel_min: tangential_min,
        tangential_accel_max: tangential_max,
        size_start,
        size_end,
        size_var,
        spin_start,
        spin_end,
        spin_var,
        color_start: floats(s, "color_start")?,
        color_end: floats(s, "color_end")?,
        color_var: num("color_var", 0.0)?,
        alpha_var: num("alpha_var", 0.0)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::psi::{MAX_PARTICLES, ParticleSystem};

    #[test]
    fn parses_a_definition() {
        let s = sections::parse(
            "[Spark]\nsprite=6\nblend=alpha\nemission=40\nlife=0.5,1\ndirection=90\nspread=180\n\
             speed=-5,10\nsize=0.5,0.1,1\ncolor_start=1,0.5,0,1\ncolor_end=0.5,0,0,0\n",
        )
        .unwrap();
        let all = parse(&s).unwrap();
        let i = all["spark"];
        assert_eq!((i.cell, i.additive(), i.emission, i.lifetime), (6, false, 40, -1.0));
        assert_eq!((i.particle_life_min, i.particle_life_max, i.speed_min), (0.5, 1.0, -5.0));
        assert!((i.direction - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!((i.size_start, i.size_var, i.color_end[0]), (0.5, 1.0, 0.5));
        assert_eq!(key("Campfire.PSI"), "campfire");
    }

    #[test]
    fn rejects_bad_values() {
        for bad in ["[a]\nsprite=16\n", "[a]\nblend=screen\n", "[a]\nlife=1\n", "[a]\nspeed=x,1\n", "[k a]\n"] {
            assert!(parse(&sections::parse(bad).unwrap()).is_err(), "{bad}");
        }
    }

    /// Every shipped system parses with known keys only, emits, and stays bounded and finite.
    #[test]
    fn shipped_particles_parse_and_simulate() {
        let root = crate::content_root();
        let s = sections::load(&root.join("data/particles.txt")).unwrap();
        assert_eq!(sections::unknown_keys(&s, PARTICLE_KEYS), Vec::<String>::new());
        let all = parse(&s).unwrap();
        for name in ["campfire", "lantern_embers", "fireflies", "fire_cast", "ember_trail", "knife_trail", "hook_trail"]
        {
            assert!(all.contains_key(name), "missing {name}");
        }
        for (name, info) in &all {
            assert!(info.emission > 0 && info.emission <= 1000, "{name}");
            assert!(info.particle_life_min > 0.0 && info.particle_life_max > 0.0, "{name}");
            let mut sys = ParticleSystem::new(*info, 7);
            for i in 0..300 {
                sys.set_position(i as f32, 0.0, i % 2 == 0);
                sys.update(1.0 / 60.0);
                assert!(sys.particles.len() <= MAX_PARTICLES, "{name}");
            }
            assert!(!sys.particles.is_empty(), "{name}: nothing emitted");
            assert!(sys.particles.iter().all(|p| p.pos[0].is_finite() && p.pos[1].is_finite()), "{name}");
            sys.stopped = true;
            for _ in 0..600 {
                sys.update(1.0 / 60.0);
            }
            assert!(sys.finished(), "{name}: never finishes once stopped");
        }
    }
}
