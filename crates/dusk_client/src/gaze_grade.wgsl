// The Eye's crimson grade (gaze.rs): an alpha-blended tint over the world. Each zone is a
// (tint colour, amount) pair: crimson from above on open ground, bruised violet-black in shade,
// near-black in deep shelter; lights and cairns pull the amount down (warm holes).
#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Grade {
    // x: strength, y: openness, z: flare, w: time
    params: vec4<f32>,
    // xy: cover grid size (cells), z: light count, w: has cover
    map: vec4<f32>,
    // xy: spot centre (cells), z: radius, w: visible
    spot: vec4<f32>,
    // xy: light centre (world), z: scale
    lights: array<vec4<f32>, 32>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> m: Grade;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var cover_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var cover_sampler: sampler;

fn bayer4(p: vec2<f32>) -> f32 {
    let i = vec2<u32>(p) % vec2(4u);
    var b = array<f32, 16>(0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0, 3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return (b[i.y * 4u + i.x] + 0.5) / 16.0;
}

// Quantise to `levels` steps with ordered dither (oldschool banding between cover zones).
fn dq(v: f32, levels: f32, d: f32) -> f32 {
    return clamp(floor(v * levels + d) / levels, 0.0, 1.0);
}

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    let p = mesh.world_position.xy;
    // Bevy world -> cell space (iso.rs: 64x32 diamonds).
    let sx = p.x / 32.0;
    let sy = -p.y / 16.0;
    let cell = vec2((sx + sy) * 0.5, (sy - sx) * 0.5);
    // Dither in 2x2 screen pixel blocks: chunkier, reads as pixel art.
    let d = bayer4(floor(mesh.position.xy * 0.5));
    let open = m.params.y;
    let t = m.params.w;

    var c = vec4(0.0);
    if (m.map.w > 0.5) {
        c = textureSampleLevel(cover_texture, cover_sampler, cell / m.map.xy, 0.0);
    }
    let shade = dq(smoothstep(0.1, 0.9, c.r), 4.0, d);
    let shelter = dq(smoothstep(0.1, 0.9, c.g), 4.0, d);

    // rgb = tint, a = amount. Crimson from above; the open Eye burns hotter.
    var g = vec4(mix(vec3(0.34, 0.008, 0.02), vec3(0.46, 0.01, 0.015), open), 0.42 + 0.1 * open);
    // Shade: bruised violet-black. Deep shelter: darkest.
    g = mix(g, vec4(0.04, 0.012, 0.06, 0.58 + 0.06 * open), shade);
    g = mix(g, vec4(0.012, 0.008, 0.025, 0.74), shelter);

    // The wandering gaze spot: pale crimson wash, soft edge, faint ring. Slow shimmer.
    if (m.spot.w > 0.5) {
        let r = m.spot.z;
        let dist = distance(cell, m.spot.xy);
        let shimmer = 0.85 + 0.15 * sin(t * 0.7 + dist * 0.8);
        var wash = (1.0 - smoothstep(r * 0.3, r, dist)) * shimmer;
        wash = dq(wash, 5.0, d) * (1.0 - shelter) * (1.0 - 0.6 * shade);
        g = mix(g, vec4(0.78, 0.34, 0.32, 0.36), wash * 0.85);
        let ring = exp(-pow((dist - r) / 0.28, 2.0)) * (1.0 - shelter);
        g = mix(g, vec4(0.70, 0.22, 0.22, 0.42), dq(ring, 3.0, d) * 0.6);
    }

    // Fires blind the Watcher: warm holes around lights and cairns.
    var warm = c.b * (0.92 + 0.08 * sin(t * 7.0 + cell.x));
    let n = u32(m.map.z);
    for (var i = 0u; i < n; i = i + 1u) {
        let l = m.lights[i];
        let q = (p - l.xy) / (vec2(300.0, 150.0) * max(l.z, 0.05));
        warm = max(warm, 1.0 - smoothstep(0.2, 1.0, length(q)));
    }
    g = mix(g, vec4(0.45, 0.22, 0.06, g.a * 0.3), dq(warm, 4.0, d));

    // Sky flare when the Eye opens (dimmer under cover).
    let flare = m.params.z * m.params.z * (1.0 - 0.7 * shelter) * (1.0 - 0.4 * shade);
    g = mix(g, vec4(0.92, 0.48, 0.42, 0.55), flare);

    return vec4(g.rgb, g.a * m.params.x);
}
