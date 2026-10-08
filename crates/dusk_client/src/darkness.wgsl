// Zone darkness: black with alpha (1 - brightness), multiplied by the alpha of
// shader_light.png around every light (the original's BlendMultiply render texture).
#import bevy_sprite::mesh2d_vertex_output::VertexOutput

struct Darkness {
    // x: alpha, y: light count, zw: shader_light.png size in px
    params: vec4<f32>,
    // xy: cut-out centre (world), z: scale
    lights: array<vec4<f32>, 64>,
};

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var<uniform> m: Darkness;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var light_texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(2) var light_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var a = m.params.x;
    let p = mesh.world_position.xy;
    let n = u32(m.params.y);
    for (var i = 0u; i < n; i = i + 1u) {
        let l = m.lights[i];
        let size = m.params.zw * l.z;
        // World is y-up, the texture y-down.
        let uv = vec2(p.x - l.x, l.y - p.y) / size + 0.5;
        if (all(uv >= vec2(0.0)) && all(uv <= vec2(1.0))) {
            a = a * textureSampleLevel(light_texture, light_sampler, uv, 0.0).a;
        }
    }
    return vec4(0.0, 0.0, 0.0, a);
}
