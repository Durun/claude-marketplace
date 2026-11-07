// ホイップ: カメラを振ったように横へ流れて次の場面。同じ時間の別の場所、別の画面への移動に（元: fx/whip.js、2 拍）。
// 前の場面が左へ流れ、次の場面が右から滑り込む。速さに合わせてぼけ、横の光の筋が走る。
const SHUTTER: f32 = 0.5;

fn pan(p: f32) -> f32 { return io5(win(p, 0.04, 0.96)); }
fn speed(p: f32) -> f32 { return (pan(p + 0.01) - pan(p - 0.01)) / 0.02; }

fn matte(uv: vec2<f32>, p: f32) -> f32 { return 1.0; }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let sp = sat(speed(p) / 4.0);
    if (sp <= 0.01) { return vec4<f32>(0.0); }
    var c = vec4<f32>(0.0);
    // 横の光の筋（行ごとに長さと明るさが違う）
    let row = floor(P.y / 5.0);
    let h = hash11(row * 1.91 + uSeed);
    let on = step(0.72, h);
    let x0 = fract(h * 13.7 - uT * 3.0) * 2400.0 - 240.0;
    let len = 200.0 + 900.0 * hash11(row * 7.7);
    let s = sat(1.0 - abs(P.x - x0) / len);
    let col = mix(uInk, uA, step(0.86, h));
    c = light(c, col * s * s * on * sp * 0.55);
    // 前後の場面の境目の光
    let seam = 1920.0 - 1920.0 * pan(p);
    let d = abs(P.x - seam);
    c = light(c, (uInk * glow(d, 6.0) * 0.8 + uA * glow(d, 40.0) * 0.4) * sp);
    return c;
}

fn motion(p: f32) -> Motion2 {
    // 元は進み具合の ±0.5/30 の位置の差を速さにする。
    let s = pan(p) * 1920.0;
    let v = abs(pan(p + 0.5 / 30.0) - pan(p - 0.5 / 30.0)) * 1920.0;
    let blur = min(60.0, v * 0.22);
    var m = still();
    m.a.x = -s;
    m.a.blur = blur;
    m.b.x = 1920.0 - s;
    m.b.blur = blur;
    return m;
}
