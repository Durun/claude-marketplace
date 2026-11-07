// パンチ: 一気にズームして白く飛び、次の場面へ（元: fx/punch.js、1.5 拍）。
// 前の場面へ一気にズームして白く飛び、次の場面が勢いで引いて止まる。周りに集中線。
const SHUTTER: f32 = 0.5;

fn matte(uv: vec2<f32>, p: f32) -> f32 { return step(0.45, p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let q = (P - vec2<f32>(960.0, 540.0)) / 540.0;
    let r = length(q);
    let ang = atan2(q.y, q.x);
    var c = vec4<f32>(0.0);
    // 集中線（2 コマごとに組み替える）
    let amt = bump(p, 0.1, 0.45, 0.85);
    let s = floor(uT * 15.0);
    let bins = 90.0;
    let a = (ang + PI) / TAU * bins;
    let id = floor(a);
    let h = hash11(id * 3.7 + s * 11.0);
    let inn = 0.55 + 0.5 * hash11(id * 9.1 + s * 3.0);
    let wdt = (0.10 + 0.25 * hash11(id + s)) * sat((r - inn) * 3.0);
    let line_d = abs(fract(a) - 0.5);
    let on_line = step(0.45, h) * step(inn, r) * (1.0 - smoothstep(wdt * 0.5, wdt * 0.5 + 0.08, line_d));
    c = over(c, paint(uInk, on_line * amt * 0.85));
    // 白い閃光
    let f = select(1.0 - out3(win(p, 0.45, 0.8)), in3(win(p, 0.2, 0.45)), p < 0.45);
    c = over(c, paint(mix(uInk, uA, sat(r * 0.3) * 0.4), sat(f * 1.15)));
    return c;
}

fn motion(p: f32) -> Motion2 {
    var m = still();
    if (p < 0.45) {
        let k = in3(p / 0.45);
        m.a.sx = 1.0 + 1.3 * k;
        m.a.sy = 1.0 + 1.3 * k;
        m.a.blur = 30.0 * k;
    } else {
        let k = 1.0 - outExpo((p - 0.45) / 0.55);
        m.b.sx = 1.0 + 0.55 * k;
        m.b.sy = 1.0 + 0.55 * k;
        m.b.blur = 30.0 * k;
    }
    return m;
}
