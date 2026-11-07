// vlog のトランジションに共通のコード。DTM_Vlog の tools/transitions/engine.js（GLSL_PRELUDE と描画）と
// index.html（COMP、前後の場面と重ねる絵の合成）を WGSL に移したもの。パックの展開がトランジションごとのコードの前に付ける。
//
// トランジションのファイルは次を定義する。座標は 1920x1080 を基準にした画素（P、上が 0）と、その 0..1（uv）。
//   const SHUTTER: f32               シャッターの開いている長さ（コマの割合）。重ねる絵とマットのモーションブラー
//   fn matte(uv, p) -> f32           次の場面（B）を見せる所。1 = B、0 = 前の場面（A）
//   fn overlay(uv, p) -> vec4<f32>   A と B の上に重ねる絵。乗算済みアルファ
//   fn motion(p) -> Motion2          A と B の映像そのものの動き（ずらし、拡大、回転、ぼかし、不透明度）
// 合成は元と同じく sRGB の値で行い、最後に線形の光へ戻す（重ねる絵の色と光のにじみが元と同じに見える）。

const REF: vec2<f32> = vec2<f32>(1920.0, 1080.0);
const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;
// 元の書き出しのコマの速さ。シャッターの長さ（コマの割合）を進み具合に直すのに使う。
const FPS: f32 = 30.0;
// 1 コマのうちに重ねる絵とマットを描く回数（時刻と画素の中の位置をずらして平均する）。
const MM_SAMPLES: i32 = 8;
// 前後の場面のぼかしのタップの数。
const BLUR_TAPS: i32 = 12;
// ぼかしの半径を ぼかしの値（基準の画素）に掛ける比。元はミップのレベル log2(blur * 0.6) で引いていた。
const BLUR_RADIUS: f32 = 0.6;
// 黄金角。ぼかしのタップを円盤に散らす。
const GOLDEN_ANGLE: f32 = 2.39996323;

// 色（sRGB の 0..1）。元の makeCtx の既定（A はミクの色、B は桃色、Ink は白、Shade は濃紺、Star は星の色）。
const uA: vec3<f32> = vec3<f32>(0.22353, 0.77255, 0.73333);
const uB: vec3<f32> = vec3<f32>(1.0, 0.36078, 0.60392);
const uInk: vec3<f32> = vec3<f32>(1.0, 1.0, 1.0);
const uShade: vec3<f32> = vec3<f32>(0.043137, 0.062745, 0.086275);
const uStar: vec3<f32> = vec3<f32>(1.0, 0.90588, 0.65882);

// 描いている画素の位置（基準の画素）と、出力の 1 画素が基準の何画素か。
var<private> P: vec2<f32>;
var<private> PX: f32;
// トランジションの中の秒、長さの秒、乱数の種（p0）。
var<private> uT: f32;
var<private> uDur: f32;
var<private> uSeed: f32;

fn sat(x: f32) -> f32 { return clamp(x, 0.0, 1.0); }
fn win(p: f32, a: f32, b: f32) -> f32 { return sat((p - a) / (b - a)); }
fn in2(x: f32) -> f32 { return x * x; }
fn out2(x: f32) -> f32 { return 1.0 - (1.0 - x) * (1.0 - x); }
fn in3(x: f32) -> f32 { return x * x * x; }
fn out3(x: f32) -> f32 { let y = 1.0 - x; return 1.0 - y * y * y; }
fn io3(x: f32) -> f32 { return select(1.0 - pow(-2.0 * x + 2.0, 3.0) / 2.0, 4.0 * x * x * x, x < 0.5); }
fn out5(x: f32) -> f32 { return 1.0 - pow(1.0 - x, 5.0); }
fn in5(x: f32) -> f32 { return pow(x, 5.0); }
fn io5(x: f32) -> f32 { return select(1.0 - pow(-2.0 * x + 2.0, 5.0) / 2.0, 16.0 * pow(x, 5.0), x < 0.5); }
fn outExpo(x: f32) -> f32 { return select(1.0 - exp2(-10.0 * x), 1.0, x >= 1.0); }
fn inExpo(x: f32) -> f32 { return select(exp2(10.0 * x - 10.0), 0.0, x <= 0.0); }
fn outBack(x: f32) -> f32 { let c = 1.70158; let y = x - 1.0; return 1.0 + (c + 1.0) * y * y * y + c * y * y; }
fn bump(p: f32, a: f32, m: f32, b: f32) -> f32 { return select(1.0 - win(p, m, b), win(p, a, m), p < m); }

// 端をなめらかにする（w は基準の画素）。
fn edge(d: f32, w: f32) -> f32 { return sat(0.5 - d / max(w, 1e-4)); }
fn fill(d: f32) -> f32 { return edge(d, PX * 1.2); }

fn hash11(p0: f32) -> f32 { var p = fract(p0 * 0.1031); p *= p + 33.33; p *= p + p; return fract(p); }
fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    q += dot(q, q.yzx + 33.33);
    return fract((q.x + q.y) * q.z);
}
fn hash22(p: vec2<f32>) -> vec2<f32> {
    var q = fract(vec3<f32>(p.x, p.y, p.x) * vec3<f32>(0.1031, 0.1030, 0.0973));
    q += dot(q, q.yzx + 33.33);
    return fract((q.xx + q.yz) * q.zy);
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let w = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, w.x), mix(c, d, w.x), w.y);
}
fn fbm(p0: vec2<f32>) -> f32 {
    var s = 0.0;
    var a = 0.5;
    var p = p0;
    for (var i = 0; i < 5; i++) {
        s += a * vnoise(p);
        p = p * 2.03 + 17.1;
        a *= 0.5;
    }
    return s;
}
fn rot2(a: f32) -> mat2x2<f32> { let c = cos(a); let s = sin(a); return mat2x2<f32>(c, -s, s, c); }

fn sdBox(p: vec2<f32>, b: vec2<f32>) -> f32 {
    let d = abs(p) - b;
    return length(max(d, vec2<f32>(0.0))) + min(max(d.x, d.y), 0.0);
}
fn sdRBox(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 { return sdBox(p, b - r) - r; }
fn sdSeg(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = sat(dot(pa, ba) / dot(ba, ba));
    return length(pa - ba * h);
}
fn sdHex(p0: vec2<f32>, r: f32) -> f32 {
    let k = vec3<f32>(-0.866025404, 0.5, 0.577350269);
    var p = abs(p0);
    p -= 2.0 * min(dot(k.xy, p), 0.0) * k.xy;
    p -= vec2<f32>(clamp(p.x, -k.z * r, k.z * r), r);
    return length(p) * sign(p.y);
}

// 光のにじみ（距離 d の所の明るさ）。
fn glow(d: f32, r: f32) -> f32 { return exp(-max(d, 0.0) / r); }
// 乗算済みアルファの重ね合わせ（src を dst の上に）。
fn over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> { return src + dst * (1.0 - src.a); }
fn paint(c: vec3<f32>, a: f32) -> vec4<f32> { return vec4<f32>(c * a, a); }
// 足し算の光（アルファは明るさから）。
fn light(dst: vec4<f32>, c: vec3<f32>) -> vec4<f32> {
    let m = max(c.r, max(c.g, c.b));
    return vec4<f32>(dst.rgb + c, dst.a + min(m, 1.0) * (1.0 - dst.a));
}

// 映像の動き。x、y は基準の画素、rot は度、blur は基準の画素、op は不透明度。
struct Motion { x: f32, y: f32, sx: f32, sy: f32, rot: f32, blur: f32, op: f32 }
struct Motion2 { a: Motion, b: Motion }
fn rest() -> Motion { return Motion(0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0); }
fn still() -> Motion2 { return Motion2(rest(), rest()); }

// 動かした場面の中の uv（index.html の boxUV）。
fn box_uv(p0: vec2<f32>, m: Motion) -> vec2<f32> {
    var q = p0 - REF * 0.5 - vec2<f32>(m.x, m.y);
    q = rot2(-m.rot * PI / 180.0) * q;
    q /= max(vec2<f32>(m.sx, m.sy), vec2<f32>(1e-4));
    return (q + REF * 0.5) / REF;
}
fn inside(uv: vec2<f32>) -> f32 {
    return step(0.0, uv.x) * step(uv.x, 1.0) * step(0.0, uv.y) * step(uv.y, 1.0);
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(1.055 * pow(x, vec3<f32>(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3<f32>(0.0031308));
}
fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let x = max(c, vec3<f32>(0.0));
    return select(pow((x + 0.055) / 1.055, vec3<f32>(2.4)), x / 12.92, x <= vec3<f32>(0.04045));
}

fn scene_tap(which_b: bool, uv: vec2<f32>) -> vec4<f32> {
    if (which_b) { return mm_to(uv); }
    return mm_from(uv);
}

// 場面（A か B）を uv で引き、sRGB の乗算済みにする。ぼかしは円盤に散らしたタップの平均。
fn scene(which_b: bool, uv: vec2<f32>, blur: f32) -> vec4<f32> {
    var c = vec4<f32>(0.0);
    if (blur * BLUR_RADIUS < 1.0) {
        c = scene_tap(which_b, uv);
    } else {
        let r = blur * BLUR_RADIUS / REF;
        for (var i = 0; i < BLUR_TAPS; i++) {
            let k = f32(i) + 0.5;
            let a = k * GOLDEN_ANGLE;
            let d = sqrt(k / f32(BLUR_TAPS));
            c += scene_tap(which_b, uv + vec2<f32>(cos(a), sin(a)) * r * d);
        }
        c /= f32(BLUR_TAPS);
    }
    return vec4<f32>(to_srgb(c.rgb), c.a);
}

fn halton(i0: i32, b: i32) -> f32 {
    var f = 1.0;
    var r = 0.0;
    var i = i0;
    while (i > 0) {
        f /= f32(b);
        r += f * f32(i % b);
        i /= b;
    }
    return r;
}

fn mm_transition(uv: vec2<f32>) -> vec4<f32> {
    let p = u.progress;
    PX = REF.x / u.size.x;
    uDur = u.duration;
    uSeed = u.p0.x;
    let m = motion(p);
    let screen = uv * REF;
    let ua = box_uv(screen, m.a);
    let ub = box_uv(screen, m.b);
    // 重ねる絵とマットは、コマの中心を挟んでシャッターの開いている間の時刻と、画素の中の位置をずらして平均する。
    let frames = max(2.0, uDur * FPS);
    var o = vec4<f32>(0.0);
    var mk = 0.0;
    for (var s = 0; s < MM_SAMPLES; s++) {
        let ts = (f32(s) + 0.5) / f32(MM_SAMPLES) - 0.5;
        let jit = vec2<f32>(halton(s + 1, 2), halton(s + 1, 3)) - 0.5;
        let po = sat(p + ts * SHUTTER / frames);
        uT = po * uDur;
        P = screen + jit * PX;
        var c = overlay(P / REF, po);
        c.a = sat(c.a);
        c = vec4<f32>(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(c.a)), c.a);
        o += c;
        let pm = sat(p + ts * min(SHUTTER, 0.25) / frames);
        uT = pm * uDur;
        P = ub * REF + jit * PX;
        mk += sat(matte(P / REF, pm));
    }
    o /= f32(MM_SAMPLES);
    mk /= f32(MM_SAMPLES);
    var col = vec4<f32>(0.0);
    col = mix(col, scene(false, ua, m.a.blur), inside(ua) * m.a.op);
    col = mix(col, scene(true, ub, m.b.blur), inside(ub) * m.b.op * mk);
    col = o + col * (1.0 - o.a);
    return vec4<f32>(to_linear(col.rgb), col.a);
}
