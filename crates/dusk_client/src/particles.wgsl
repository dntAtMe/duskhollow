// FxMaterial: texture * vertex colour (blend state set by the pipeline specialisation).
#import bevy_sprite::{
    mesh2d_vertex_output::VertexOutput,
    mesh2d_view_bindings::view,
}
#ifdef TONEMAP_IN_SHADER
#import bevy_core_pipeline::tonemapping
#endif
#ifdef SRGB_OUTPUT
#import bevy_render::color_operations::linear_to_srgb
#endif

@group(#{MATERIAL_BIND_GROUP}) @binding(0) var texture: texture_2d<f32>;
@group(#{MATERIAL_BIND_GROUP}) @binding(1) var texture_sampler: sampler;

@fragment
fn fragment(mesh: VertexOutput) -> @location(0) vec4<f32> {
    var c = textureSample(texture, texture_sampler, mesh.uv);
#ifdef VERTEX_COLORS
    c = c * mesh.color;
#endif
#ifdef TONEMAP_IN_SHADER
    c = tonemapping::tone_mapping(c, view.color_grading);
#endif
#ifdef SRGB_OUTPUT
    c = vec4(linear_to_srgb(c.rgb), c.a);
#endif
    return c;
}
