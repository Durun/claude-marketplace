// レイヤー 1 枚を、変形した四角形として premultiplied のまま重ねる。
// 合成は固定機能のブレンド（src: One、dst: OneMinusSrcAlpha）に任せる。

struct Item {
    uv_to_ndc: mat4x4<f32>,
    opacity: f32,
}

@group(0) @binding(0) var<uniform> item: Item;
@group(0) @binding(1) var layer_tex: texture_2d<f32>;
@group(0) @binding(2) var layer_sampler: sampler;

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var corners = array<vec2<f32>, 6>(
        vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(0.0, 1.0),
        vec2<f32>(0.0, 1.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
    );
    let uv = corners[index];
    var out: VsOut;
    out.position = item.uv_to_ndc * vec4<f32>(uv, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    return textureSample(layer_tex, layer_sampler, in.uv) * item.opacity;
}
