// レイヤー 1 枚を、変形した四角形として premultiplied のまま重ねる。
// 合成は固定機能のブレンド（src: One、dst: OneMinusSrcAlpha）に任せる。
// 縮小するときは、出力の 1 画素が覆う元の画素の範囲を taps x taps 点で標本化して平均する（面積平均）。
// 線形の光の f16 のテクスチャの上で行うので、縮小はここ 1 回だけ。

struct Item {
    uv_to_ndc: mat4x4<f32>,
    opacity: f32,
    // 出力の 1 画素あたりの元の画素の数（軸ごと）。1 以下なら双線形 1 点。
    taps: vec2<f32>,
}

@group(0) @binding(0) var<uniform> item: Item;
@group(0) @binding(1) var layer_tex: texture_2d<f32>;
@group(0) @binding(2) var layer_sampler: sampler;

// 面積平均に使う点の数の上限（軸ごと）。これを超える縮小は、この間隔の点の平均になる。
const MAX_TAPS: u32 = 8u;

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
    if (item.taps.x <= 1.0 && item.taps.y <= 1.0) {
        return textureSample(layer_tex, layer_sampler, in.uv) * item.opacity;
    }
    let tex_size = vec2<f32>(textureDimensions(layer_tex));
    let n = min(vec2<u32>(ceil(max(item.taps, vec2<f32>(1.0)))), vec2<u32>(MAX_TAPS));
    // 出力の 1 画素が覆う UV の幅。この幅を n 等分した各区画の中心を標本化する。
    let footprint = item.taps / tex_size;
    var sum = vec4<f32>(0.0);
    for (var j = 0u; j < n.y; j++) {
        for (var i = 0u; i < n.x; i++) {
            let f = (vec2<f32>(f32(i), f32(j)) + 0.5) / vec2<f32>(n) - 0.5;
            sum += textureSampleLevel(layer_tex, layer_sampler, in.uv + f * footprint, 0.0);
        }
    }
    return sum / f32(n.x * n.y) * item.opacity;
}
