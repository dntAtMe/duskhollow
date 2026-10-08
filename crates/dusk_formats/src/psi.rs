//! `scripts/particles/*.psi` — particle systems, plus a faithful CPU simulation of them.
//!
//! The file is the 128-byte `hgeParticleSystemInfo` of the HGE engine's particle editor
//! (little-endian, no header). The legacy client does not use HGE at runtime: its own
//! `ParticleSystem` (SFML vertex array) re-implements HGE's update rules with a few
//! differences. Everything below is verified against the client binary:
//!
//! - `ParticleSystemInfo_loadFromFile` (0x4e0b90): field order and the `sprite` decoding.
//! - `ParticleSystem_ctor` (0x4f51a0): texture `particles.png` (4x4 grid of 32x32 cells).
//! - `ParticleSystem_update` (0x4f5880), `ParticleSystem_spawnParticle` (0x4f5ee0): rules.
//! - `ParticleSystem_draw` (0x4f5e70): SFML `BlendAdd` when additive, else default alpha.
//! - `ParticleSystem_setPosition` (0x4f5700), `ParticleSystem_move` (0x4f54d0).
//!
//! ```text
//! off  type  field                    notes
//!   0  u32   sprite                   bits 0-1: column, bits 2-15: row of a 32x32 cell in
//!                                     particles.png; bits 16+: HGE blend (4 = additive,
//!                                     6 = alpha blend). Client: additive = (v & 0xffff0000) != 0x60000
//!   4  i32   emission                 particles per second
//!   8  f32   lifetime                 system lifetime, <= 0 = forever (all shipped files: -1)
//!  12  f32   particle_life_min/max    seconds (min may exceed max; uniform in between)
//!  20  f32   direction                radians, 0 = up (screen), clockwise
//!  24  f32   spread                   radians, full cone width
//!  28  u32   relative                 add the emitter's movement direction to `direction`
//!  32  f32   speed_min/max            px/s
//!  40  f32   gravity_min/max          px/s^2 along screen +y (down)
//!  48  f32   radial_accel_min/max     px/s^2 away from the emitter
//!  56  f32   tangential_accel_min/max px/s^2 perpendicular to the radial direction
//!  64  f32   size_start/end/var       multiples of 32 px
//!  76  f32   spin_start/end/var       simulated but never rendered by the client
//!  88  f32x4 color_start              RGBA 0..1
//! 104  f32x4 color_end
//! 120  f32   color_var, alpha_var     0..1: how far towards *_end the start value may be randomised
//! ```

use std::f32::consts::FRAC_PI_2;
use std::path::Path;

/// Size of a `.psi` file / `hgeParticleSystemInfo`.
pub const PSI_SIZE: usize = 128;
/// `ParticleSystem::MaxParticles` (asserted in `spawnParticle`).
pub const MAX_PARTICLES: usize = 500;
/// Texture shared by every particle system.
pub const TEXTURE: &str = "particles.png";
/// Side of one particle cell in [`TEXTURE`], also the on-screen size of a particle at size 1.0.
pub const CELL: f32 = 32.0;

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ParticleSystemInfo {
    /// Raw `sprite` word (frame + HGE blend flags).
    pub sprite: u32,
    pub emission: i32,
    pub lifetime: f32,
    pub particle_life_min: f32,
    pub particle_life_max: f32,
    pub direction: f32,
    pub spread: f32,
    pub relative: bool,
    pub speed_min: f32,
    pub speed_max: f32,
    pub gravity_min: f32,
    pub gravity_max: f32,
    pub radial_accel_min: f32,
    pub radial_accel_max: f32,
    pub tangential_accel_min: f32,
    pub tangential_accel_max: f32,
    pub size_start: f32,
    pub size_end: f32,
    pub size_var: f32,
    pub spin_start: f32,
    pub spin_end: f32,
    pub spin_var: f32,
    pub color_start: [f32; 4],
    pub color_end: [f32; 4],
    pub color_var: f32,
    pub alpha_var: f32,
}

impl ParticleSystemInfo {
    pub fn parse(data: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(data.len() >= PSI_SIZE, "psi too short: {} bytes", data.len());
        let u = |i: usize| u32::from_le_bytes(data[i * 4..i * 4 + 4].try_into().unwrap());
        let f = |i: usize| f32::from_bits(u(i));
        Ok(Self {
            sprite: u(0),
            emission: u(1) as i32,
            lifetime: f(2),
            particle_life_min: f(3),
            particle_life_max: f(4),
            direction: f(5),
            spread: f(6),
            relative: u(7) != 0,
            speed_min: f(8),
            speed_max: f(9),
            gravity_min: f(10),
            gravity_max: f(11),
            radial_accel_min: f(12),
            radial_accel_max: f(13),
            tangential_accel_min: f(14),
            tangential_accel_max: f(15),
            size_start: f(16),
            size_end: f(17),
            size_var: f(18),
            spin_start: f(19),
            spin_end: f(20),
            spin_var: f(21),
            color_start: [f(22), f(23), f(24), f(25)],
            color_end: [f(26), f(27), f(28), f(29)],
            color_var: f(30),
            alpha_var: f(31),
        })
    }

    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        Self::parse(&std::fs::read(path)?)
    }

    /// Additive (`sf::BlendAdd`) or plain alpha blending. The client tests
    /// `(sprite & 0xffff0000) != 0x60000`, i.e. everything but HGE `BLEND_ALPHABLEND|ZWRITE`.
    pub fn additive(&self) -> bool {
        self.sprite & 0xffff_0000 != 0x6_0000
    }

    /// Top-left pixel of the particle's 32x32 cell in [`TEXTURE`].
    pub fn texture_origin(&self) -> (u32, u32) {
        ((self.sprite & 3) * 32, ((self.sprite >> 2) & 0x3fff) * 32)
    }
}

/// One live particle (`0x54` bytes in the client; same fields).
#[derive(Debug, Clone, Copy, Default)]
pub struct Particle {
    pub gravity: f32,
    pub radial_accel: f32,
    pub tangential_accel: f32,
    pub spin: f32,
    pub spin_delta: f32,
    /// Multiple of [`CELL`].
    pub size: f32,
    pub size_delta: f32,
    pub age: f32,
    pub terminal_age: f32,
    pub color: [f32; 4],
    pub color_delta: [f32; 4],
    /// Position in the system's space (pixels, y down).
    pub pos: [f32; 2],
    pub vel: [f32; 2],
}

impl Particle {
    /// Vertex colour as the client builds it: `(u8)(int)(c * 255)` per channel. Values
    /// stay inside 0..1 for all shipped files (linear interpolation between start and end).
    pub fn rgba8(&self) -> [u8; 4] {
        self.color.map(|c| (c * 255.0) as i32 as u8)
    }
}

/// Small deterministic PRNG (the client uses a `std::mt19937` behind `randomFloat`, 0x4f5000).
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// Uniform in `[a, b)` (`b < a` allowed). Returns `a` when `a == b` like `randomFloat`.
    pub fn range(&mut self, a: f32, b: f32) -> f32 {
        if a == b {
            return a;
        }
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let r = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40;
        a + (b - a) * (r as f32 / (1u64 << 24) as f32)
    }
}

/// Port of the client's `ParticleSystem`. Positions are in pixels with y pointing down
/// (SFML screen space); callers convert to their own world space.
#[derive(Debug, Clone)]
pub struct ParticleSystem {
    pub info: ParticleSystemInfo,
    pub particles: Vec<Particle>,
    /// Emitter position (`0x14c`) and the position before the last move (`0x154`).
    pub location: [f32; 2],
    pub prev_location: [f32; 2],
    /// Seconds since creation (`0xbc`).
    pub age: f32,
    emission_residue: f32,
    /// No new particles once set (`0xc0`); live ones finish their life.
    pub stopped: bool,
    rng: Rng,
}

impl ParticleSystem {
    pub fn new(info: ParticleSystemInfo, seed: u64) -> Self {
        Self {
            info,
            particles: Vec::new(),
            location: [0.0; 2],
            prev_location: [0.0; 2],
            age: 0.0,
            emission_residue: 0.0,
            stopped: false,
            rng: Rng::new(seed),
        }
    }

    /// `setPosition(moveParticles, x, y)`. With `move_particles` the live particles are
    /// translated too (effect attached to a sprite/unit); without, they stay where they
    /// were emitted (projectile trails).
    pub fn set_position(&mut self, x: f32, y: f32, move_particles: bool) {
        if self.location == [x, y] {
            return;
        }
        if move_particles {
            let (dx, dy) = (x - self.location[0], y - self.location[1]);
            for p in &mut self.particles {
                p.pos[0] += dx;
                p.pos[1] += dy;
            }
            self.prev_location[0] += dx;
            self.prev_location[1] += dy;
        } else if self.age == 0.0 {
            self.prev_location = [x, y];
        } else {
            self.prev_location = self.location;
        }
        self.location = [x, y];
    }

    /// True once nothing is alive and nothing more will be emitted (the kit-finished
    /// test of the spell visual code, 0x500d00).
    pub fn finished(&self) -> bool {
        if !self.particles.is_empty() {
            return false;
        }
        if self.info.lifetime <= 0.0 {
            self.stopped && (self.age as i32) as f32 > 1.0
        } else {
            self.age >= self.info.lifetime
        }
    }

    pub fn update(&mut self, dt: f32) {
        self.age += dt;
        let needed = dt * self.info.emission as f32 + self.emission_residue;
        let n = needed.floor() as i32;
        self.emission_residue = needed - n as f32;
        if !self.stopped {
            for _ in 0..n {
                if self.particles.len() >= MAX_PARTICLES {
                    break;
                }
                self.spawn();
            }
        }
        let [lx, ly] = self.location;
        self.particles.retain_mut(|p| {
            p.age += dt;
            if p.age >= p.terminal_age {
                return false;
            }
            // Radial unit vector (the client uses the 0x5f3759df fast inverse square root;
            // a particle exactly on the emitter gets a zero vector there too).
            let (dx, dy) = (p.pos[0] - lx, p.pos[1] - ly);
            let len2 = dx * dx + dy * dy;
            let inv = if len2 > 0.0 { len2.sqrt().recip() } else { 0.0 };
            let (nx, ny) = (dx * inv, dy * inv);
            p.vel[0] += (nx * p.radial_accel - ny * p.tangential_accel) * dt;
            p.vel[1] += (ny * p.radial_accel + nx * p.tangential_accel) * dt;
            p.vel[1] += p.gravity * dt;
            p.pos[0] += p.vel[0] * dt;
            p.pos[1] += p.vel[1] * dt;
            p.spin += p.spin_delta * dt;
            p.size += p.size_delta * dt;
            for i in 0..4 {
                p.color[i] += p.color_delta[i] * dt;
            }
            true
        });
    }

    fn spawn(&mut self) {
        let i = self.info;
        let r = &mut self.rng;
        // The client clears this guard only through `stopped`/`lifetime`; keep it identical.
        if i.lifetime <= 0.0 {
            if ((self.age as i32) as f32) > 1.0 && self.particles.is_empty() && self.stopped {
                return;
            }
        } else if i.lifetime <= self.age && self.particles.is_empty() {
            return;
        }
        let mut color = [0.0; 4];
        for c in 0..3 {
            color[c] = r.range(i.color_start[c], i.color_start[c] + (i.color_end[c] - i.color_start[c]) * i.color_var);
        }
        color[3] = r.range(i.color_start[3], i.color_start[3] + (i.color_end[3] - i.color_start[3]) * i.alpha_var);
        let terminal_age = r.range(i.particle_life_min, i.particle_life_max);
        // Unlike HGE, no interpolation between the previous and current emitter position.
        let pos = [self.location[0] + r.range(-2.0, 2.0), self.location[1] + r.range(-2.0, 2.0)];
        let mut ang = i.direction - FRAC_PI_2 + r.range(0.0, i.spread) - i.spread * 0.5;
        if i.relative {
            let (dx, dy) = (self.prev_location[0] - self.location[0], self.prev_location[1] - self.location[1]);
            ang += dy.atan2(dx) + FRAC_PI_2;
        }
        let speed = r.range(i.speed_min, i.speed_max);
        let gravity = r.range(i.gravity_min, i.gravity_max);
        let radial_accel = r.range(i.radial_accel_min, i.radial_accel_max);
        let tangential_accel = r.range(i.tangential_accel_min, i.tangential_accel_max);
        let size = r.range(i.size_start, i.size_start + (i.size_end - i.size_start) * i.size_var);
        let spin = r.range(i.spin_start, i.spin_start + (i.spin_end - i.spin_start) * i.spin_var);
        let mut color_delta = [0.0; 4];
        for c in 0..4 {
            color_delta[c] = (i.color_end[c] - color[c]) / terminal_age;
        }
        self.particles.push(Particle {
            gravity,
            radial_accel,
            tangential_accel,
            spin,
            spin_delta: (i.spin_end - spin) / terminal_age,
            size,
            size_delta: (i.size_end - size) / terminal_age,
            age: 0.0,
            terminal_age,
            color,
            color_delta,
            pos,
            vel: [ang.cos() * speed, ang.sin() * speed],
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ParticleSystemInfo {
        ParticleSystemInfo {
            sprite: 0x4_000a,
            emission: 100,
            lifetime: -1.0,
            particle_life_min: 0.5,
            particle_life_max: 0.5,
            spread: std::f32::consts::TAU,
            size_start: 1.0,
            size_end: 0.0,
            color_start: [1.0, 0.5, 0.0, 1.0],
            color_end: [1.0, 0.5, 0.0, 0.0],
            ..Default::default()
        }
    }

    #[test]
    fn parse_roundtrip_layout() {
        let mut b = vec![0u8; PSI_SIZE];
        b[0..4].copy_from_slice(&0x6_0005u32.to_le_bytes());
        b[4..8].copy_from_slice(&40i32.to_le_bytes());
        b[28..32].copy_from_slice(&1u32.to_le_bytes());
        b[64..68].copy_from_slice(&0.75f32.to_le_bytes());
        b[124..128].copy_from_slice(&0.25f32.to_le_bytes());
        let i = ParticleSystemInfo::parse(&b).unwrap();
        assert_eq!((i.emission, i.relative, i.size_start, i.alpha_var), (40, true, 0.75, 0.25));
        assert!(!i.additive());
        assert_eq!(i.texture_origin(), (32, 32));
        assert!(ParticleSystemInfo::parse(&b[..100]).is_err());
    }

    #[test]
    fn emission_rate_and_lifetime() {
        let mut s = ParticleSystem::new(info(), 1);
        for _ in 0..30 {
            s.update(1.0 / 60.0); // 0.5 s
        }
        // 100/s for 0.5 s, nothing has died yet (life 0.5 s, first spawned at t=1/60)
        assert!((49..=50).contains(&s.particles.len()), "{}", s.particles.len());
        for _ in 0..120 {
            s.update(1.0 / 60.0);
        }
        assert!((49..=51).contains(&s.particles.len()), "steady state {}", s.particles.len());
        s.stopped = true;
        for _ in 0..120 {
            s.update(1.0 / 60.0);
        }
        assert!(s.particles.is_empty() && s.finished());
    }

    #[test]
    fn attached_vs_trail() {
        let mut a = ParticleSystem::new(info(), 2);
        a.update(0.1);
        let before = a.particles[0].pos;
        a.set_position(10.0, 0.0, true);
        assert_eq!(a.particles[0].pos[0], before[0] + 10.0);
        a.set_position(20.0, 0.0, false);
        assert_eq!(a.particles[0].pos[0], before[0] + 10.0);
    }

    #[test]
    fn colour_and_size_reach_end_values() {
        let mut s = ParticleSystem::new(info(), 3);
        s.update(0.02);
        s.stopped = true;
        let p0 = s.particles[0];
        s.update(0.45);
        let p = s.particles[0];
        // size_var = 0: spawned at size_start (1.0), linear to size_end (0.0) over its life.
        assert!((p.size_delta + 1.0 / p.terminal_age).abs() < 1e-4);
        assert!((p.size - p.size_delta * p.age - 1.0).abs() < 1e-4);
        assert!(p.color[3] < p0.color[3]);
    }
}
