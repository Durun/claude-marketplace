// スペクトラム: アナライザーのバーが下から伸びて次の場面を見せる。ミックス、EQ、音作りの話題に（元: fx/spectrum.js、3 拍）。
// 32 本のバーが踊りながら伸び、バーの中に次の場面。ピークの印が上に残って落ちる。
const SHUTTER: f32 = 0.4;

const NBAR: f32 = 32.0;
const BAR_W: f32 = 1920.0 / 32.0;

// GLSL の mod（床関数基準）。画素の位置が負でも周期が崩れない。
fn fmodg(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }

fn barH(i: f32, p: f32) -> f32 {
    let t = p * uDur;
    let dance = 0.62 + 0.38 * vnoise(vec2<f32>(i * 0.45, t * 4.5 + uSeed));
    // 低い方が高い（音楽のスペクトラムらしく）
    let tilt = 1.0 - 0.35 * (i / NBAR);
    let rise = in3(win(p, 0.08 + 0.12 * hash11(i * 7.1), 0.62));
    let idle = 30.0 + 60.0 * vnoise(vec2<f32>(i * 0.5, t * 9.0));
    return idle * win(p, 0.0, 0.1) + rise * 2300.0 * dance * tilt;
}
fn peakCap(i: f32, p: f32) -> f32 {
    var m = 0.0;
    for (var k = 0; k < 10; k++) {
        let dt = f32(k) * 0.025;
        m = max(m, barH(i, p - dt) - 9000.0 * dt * dt);
    }
    return m;
}
fn closeGap(p: f32) -> f32 { return win(p, 0.55, 0.72); }

struct BarInfo { sd: f32, i: f32, h: f32 }
fn bar(p: f32) -> BarInfo {
    let i = floor(P.x / BAR_W);
    let lx = P.x - (i + 0.5) * BAR_W;
    let h = barH(i, p);
    let hw = mix(BAR_W * 0.5 - 5.0, BAR_W * 0.5 + 1.0, closeGap(p));
    return BarInfo(max(abs(lx) - hw, (1080.0 - h) - P.y), i, h);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.76) { return 1.0; }
    return fill(bar(p).sd);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.08) * (1.0 - win(p, 0.6, 0.78));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let b = bar(p);
    let inB = fill(b.sd);
    var c = paint(uShade, 0.55 * env * (1.0 - inB));
    c = over(c, paint(uInk, 0.08 * fill(abs(fmodg(P.y, 90.0) - 45.0) - 0.6) * env * (1.0 - inB)));
    // バー: 下から差し色 → 2 つ目の差し色のグラデーション。LED のすき間
    let y = 1.0 - P.y / 1080.0;
    let col = mix(uA, uB, sat(y * 1.6 - 0.4));
    let led = fill(abs(fmodg(P.y, 16.0) - 8.0) - 6.0);
    c = over(c, paint(col, (0.14 + 0.22 * led) * inB * env * (1.0 - closeGap(p) * 0.7)));
    let tl = abs(P.y - (1080.0 - b.h));
    c = light(c, (uInk * fill(tl - 1.5) + col * glow(tl, 8.0) * 0.5) * inB * env);
    // ピークの印
    let cp = 1080.0 - peakCap(b.i, p) - 10.0;
    let lx = P.x - (b.i + 0.5) * BAR_W;
    let capD = max(abs(lx) - (BAR_W * 0.5 - 5.0), abs(P.y - cp) - 3.0);
    c = over(c, paint(uInk, fill(capD) * 0.95 * env));
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
