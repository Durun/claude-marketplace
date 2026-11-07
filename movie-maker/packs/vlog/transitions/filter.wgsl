// フィルター: ローパスフィルターのカーブが閉じていき、削られた所から次の場面。音作り、エフェクト、ミックスの話題に（元: fx/filter.js、4 拍）。
// EQ の画面が重なり、ローパスのカットオフが右から左へ閉じていく。カーブより上の削られた所が B。
const SHUTTER: f32 = 0.4;

const Y0: f32 = 260.0;
const DB: f32 = 9.0;
const Q: f32 = 3.2;

fn fmodf(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }
fn fToX(f: f32) -> f32 { return 1920.0 * log(f / 20.0) / log(1000.0); }
fn cutoff(p: f32) -> f32 {
    let x = win(p, 0.03, 0.84);
    return 16000.0 * pow(0.9 / 16000.0, mix(x, io3(x), 0.35));
}
fn xToF(x: f32) -> f32 { return 20.0 * pow(1000.0, x / 1920.0); }
fn gainDb(f: f32, fc: f32) -> f32 {
    let r = f / fc;
    let h2 = 1.0 / ((1.0 - r * r) * (1.0 - r * r) + (r / Q) * (r / Q));
    return 20.0 * log(h2) / log(10.0);
}
fn curveY(x: f32, p: f32) -> f32 { return Y0 - gainDb(xToF(x), cutoff(p)) * DB; }

struct Curve { d: f32, y: f32 }
// カーブからの距離（画素）と、その x でのカーブの y。カーブより上が負
fn curveDist(p: f32) -> Curve {
    let y = curveY(P.x, p);
    let slope = curveY(P.x + 1.0, p) - y;
    return Curve((P.y - y) / max(1.0, length(vec2<f32>(1.0, slope))), y);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.9) { return 1.0; }
    let cd = curveDist(p);
    return fill(cd.d) * step(Y0, cd.y);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.1) * (1.0 - win(p, 0.8, 0.93));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let cd = curveDist(p);
    let d = cd.d;
    let y = cd.y;
    let cut = fill(d) * step(Y0, y); // B が見えている所
    var c = paint(uShade, 0.6 * env * (1.0 - cut));
    // 周波数の目盛り（50 Hz〜20 kHz）と dB の横線
    var g = 0.0;
    var fs = array<f32, 9>(50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0, 10000.0, 20000.0);
    for (var k = 0; k < 9; k++) {
        g = max(g, fill(abs(P.x - fToX(fs[k])) - 0.6) * select(0.14, 0.28, k == 4));
    }
    g = max(g, fill(abs(fmodf(P.y - Y0, DB * 6.0)) - 0.6) * 0.1);
    g = max(g, fill(abs(P.y - Y0) - 0.8) * 0.3);
    c = over(c, paint(uInk, g * env * (1.0 - cut)));
    // 通っている帯域（カーブの下）の色
    let below = step(y, P.y) * (1.0 - cut);
    c = over(c, paint(uA, 0.16 * below * env * (1.0 - sat((P.y - y) / 600.0))));
    // カーブ
    let ad = abs(d);
    c = light(c, (mix(uA, uInk, 0.55) * fill(ad - 2.0) + uA * glow(ad, 12.0) * 0.55) * env);
    // カットオフの点（EQ のつまみ）
    let fc = cutoff(p);
    let knob = vec2<f32>(fToX(fc), Y0 - gainDb(fc, fc) * DB);
    let kd = length(P - knob);
    c = over(c, paint(uB, fill(kd - 11.0) * env));
    c = over(c, paint(uInk, fill(abs(kd - 16.0) - 2.0) * env));
    c = light(c, uB * glow(kd - 14.0, 14.0) * 0.6 * env);
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
