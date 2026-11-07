// レイヤー 1 枚を、変形した四角形として premultiplied のまま重ねる。
// 合成は固定機能のブレンド（src: One、dst: OneMinusSrcAlpha）に任せる。
// 縮小するときは、出力の 1 画素が覆う元の画素の範囲を taps x taps 点で標本化して平均する（面積平均）。
// 線形の光の f16 のテクスチャの上で行うので、縮小はここ 1 回だけ。
// 切り抜きと角丸の矩形のマスクは、レイヤーの座標での被覆率として同じパスで掛ける。

struct MaskItem {
    // 左上と右下（レイヤーの座標）。
    rect: vec4<f32>,
    radius: f32,
    feather: f32,
    opacity: f32,
    // 下位 2 ビットが組み合わせ方（0 足す、1 引く、2 重なり）、4 が反転。
    flags: u32,
}

struct Item {
    uv_to_ndc: mat4x4<f32>,
    opacity: f32,
    mask_count: u32,
    // 出力の 1 画素あたりの元の画素の数（軸ごと）。1 以下なら双線形 1 点。
    taps: vec2<f32>,
    // 画像の置き場所 [x, y, w, h]（レイヤーの座標）。
    place: vec4<f32>,
    // 切り抜いた後の矩形の左上と右下。削らない辺は遠い値。
    crop: vec4<f32>,
    masks: array<MaskItem, 4>,
}

@group(0) @binding(0) var<uniform> item: Item;
@group(0) @binding(1) var layer_tex: texture_2d<f32>;
@group(0) @binding(2) var layer_sampler: sampler;

// 面積平均に使う点の数の上限（軸ごと）。これを超える縮小は、この間隔の点の平均になる。
const MAX_TAPS: u32 = 8u;
const MODE_SUBTRACT: u32 = 1u;
const MODE_INTERSECT: u32 = 2u;
const FLAG_INVERTED: u32 = 4u;

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

// 角丸の矩形までの符号つきの距離（内が負）。
fn sd_round_rect(p: vec2<f32>, lo: vec2<f32>, hi: vec2<f32>, r: f32) -> f32 {
    let q = abs(p - (lo + hi) * 0.5) - (hi - lo) * 0.5 + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// 切り抜きの被覆率。辺ごとに差を取り、削らない辺の遠い値で桁落ちしない。aa は出力の 1 画素のレイヤーの画素数。
fn crop_coverage(p: vec2<f32>, aa: f32) -> f32 {
    let inside = vec4<f32>(p - item.crop.xy, item.crop.zw - p) / aa + 0.5;
    let c = clamp(inside, vec4<f32>(0.0), vec4<f32>(1.0));
    return c.x * c.y * c.z * c.w;
}

// マスクの組み合わせ。After Effects と同じく、最初のマスクが足すなら空から、ほかなら全部から始める。
fn mask_coverage(p: vec2<f32>, aa: f32) -> f32 {
    if (item.mask_count == 0u) {
        return 1.0;
    }
    var a = select(1.0, 0.0, (item.masks[0].flags & 3u) == 0u);
    for (var i = 0u; i < item.mask_count; i++) {
        let m = item.masks[i];
        let soft = max(m.feather, aa);
        let d = sd_round_rect(p, m.rect.xy, m.rect.zw, m.radius);
        var c = clamp(0.5 - d / soft, 0.0, 1.0);
        if ((m.flags & FLAG_INVERTED) != 0u) {
            c = 1.0 - c;
        }
        c *= m.opacity;
        let mode = m.flags & 3u;
        if (mode == MODE_SUBTRACT) {
            a *= 1.0 - c;
        } else if (mode == MODE_INTERSECT) {
            a *= c;
        } else {
            a = a + c - a * c;
        }
    }
    return a;
}

fn sample_layer(uv: vec2<f32>) -> vec4<f32> {
    if (item.taps.x <= 1.0 && item.taps.y <= 1.0) {
        return textureSample(layer_tex, layer_sampler, uv);
    }
    let tex_size = vec2<f32>(textureDimensions(layer_tex));
    let n = min(vec2<u32>(ceil(max(item.taps, vec2<f32>(1.0)))), vec2<u32>(MAX_TAPS));
    // 出力の 1 画素が覆う UV の幅。この幅を n 等分した各区画の中心を標本化する。
    let footprint = item.taps / tex_size;
    var sum = vec4<f32>(0.0);
    for (var j = 0u; j < n.y; j++) {
        for (var i = 0u; i < n.x; i++) {
            let f = (vec2<f32>(f32(i), f32(j)) + 0.5) / vec2<f32>(n) - 0.5;
            sum += textureSampleLevel(layer_tex, layer_sampler, uv + f * footprint, 0.0);
        }
    }
    return sum / f32(n.x * n.y);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let p = item.place.xy + in.uv * item.place.zw;
    let w = fwidth(p);
    let aa = max(max(w.x, w.y), 1e-6);
    let coverage = crop_coverage(p, aa) * mask_coverage(p, aa);
    return sample_layer(in.uv) * (item.opacity * coverage);
}
