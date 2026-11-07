// 星座: 夜空に八分音符の星座が結ばれ、星から次の場面が開く（元: fx/stars.js、6 拍）。
// 夜空が降りて星がまたたき、明るい星が線でつながって ♪ になり、星から広がる光の円が次の場面を見せる。
const SHUTTER: f32 = 0.5;

const NS: i32 = 8;
var<private> ST_POS: array<vec2<f32>, 8> = array<vec2<f32>, 8>(
    vec2<f32>(835.0, 715.0), vec2<f32>(905.0, 758.0), vec2<f32>(972.0, 690.0), vec2<f32>(900.0, 648.0),
    vec2<f32>(976.0, 500.0), vec2<f32>(982.0, 318.0), vec2<f32>(1068.0, 392.0), vec2<f32>(1100.0, 488.0));
var<private> ST_LINE: array<vec2<i32>, 8> = array<vec2<i32>, 8>(
    vec2<i32>(0, 1), vec2<i32>(1, 2), vec2<i32>(2, 3), vec2<i32>(3, 0),
    vec2<i32>(2, 4), vec2<i32>(4, 5), vec2<i32>(5, 6), vec2<i32>(6, 7));

fn star_sp(i: i32) -> vec2<f32> { return vec2<f32>(960.0, 520.0) + (ST_POS[i] - vec2<f32>(968.0, 538.0)) * 1.35; }
fn star_at(i: i32) -> f32 { return 0.14 + f32(i) * 0.035; }
fn line_at(i: i32) -> f32 { return 0.2 + f32(i) * 0.036; }

// 次の場面を見せる円（星座の星と、散らばった明るい星）。
fn reveal(p: f32) -> f32 {
    var m = 0.0;
    for (var i = 0; i < NS; i++) {
        let t = 0.56 + f32(i) * 0.018;
        let r = 2300.0 * in2(win(p, t, t + 0.36));
        m = max(m, fill(length(P - star_sp(i)) - r));
    }
    for (var k = 0; k < 14; k++) {
        let c = vec2<f32>(hash11(f32(k) * 3.1 + 1.0), hash11(f32(k) * 7.7 + 2.0)) * REF;
        let t = 0.6 + 0.12 * hash11(f32(k) * 5.3);
        let r = 1400.0 * in2(win(p, t, t + 0.32));
        m = max(m, fill(length(P - c) - r));
    }
    return m;
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.97) { return 1.0; }
    return reveal(p);
}

fn sparkle(d: vec2<f32>, size: f32) -> vec3<f32> {
    let r = length(d);
    let core = glow(r, size * 0.5) + fill(r - size * 0.35);
    let rays = glow(abs(d.x), 1.2) * glow(abs(d.y), size * 4.0) + glow(abs(d.y), 1.2) * glow(abs(d.x), size * 4.0);
    return vec3<f32>(core + 0.8 * rays);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let m = select(reveal(p), 1.0, p > 0.97);
    let env = win(p, 0.0, 0.22);
    // 夜空（上が深い藍、下が少し紫）
    var sky = mix(vec3<f32>(0.02, 0.035, 0.09), vec3<f32>(0.09, 0.05, 0.16), sat(uv.y * 1.1));
    sky += uA * 0.05 * glow(abs(uv.y - 0.72), 0.12);
    let sky_a = 0.94 * in2(env) * (1.0 - m);
    var c = paint(sky, sky_a);
    // またたく小さな星
    let cell = floor(P / 34.0);
    let h = hash22(cell);
    let sp = (cell + 0.2 + 0.6 * h) * 34.0;
    let tw = 0.5 + 0.5 * sin(uT * (3.0 + 5.0 * h.x) + h.y * 40.0);
    let tiny = step(0.72, hash21(cell + 11.0)) * glow(length(P - sp), 0.9 + 1.2 * h.y) * (0.35 + 0.65 * tw);
    let appear = win(p, 0.04 + 0.2 * h.x, 0.14 + 0.2 * h.x);
    c = light(c, mix(uInk, uStar, h.y) * tiny * appear * (1.0 - m));
    // 星座の線（順に引かれる）
    for (var i = 0; i < 8; i++) {
        let ab = ST_LINE[i];
        let a = star_sp(ab.x);
        let b = star_sp(ab.y);
        let t = win(p, line_at(i), line_at(i) + 0.05);
        if (t <= 0.0) { continue; }
        let e = mix(a, b, out3(t));
        let d = sdSeg(P, a, e);
        let fade_l = 1.0 - win(p, 0.62, 0.8);
        c = light(c, (mix(uA, uInk, 0.4) * fill(d - 1.1) * 0.8 + uA * glow(d, 7.0) * 0.4) * fade_l);
    }
    // 星座の星（点いた瞬間に強く光る）。明滅
    let pulse = 1.0 + 0.6 * bump(p, 0.5, 0.56, 0.64);
    for (var i = 0; i < NS; i++) {
        let t = win(p, star_at(i), star_at(i) + 0.03);
        if (t <= 0.0) { continue; }
        let pop = 1.0 + 2.5 * exp(-(p - star_at(i)) * 25.0);
        let s = sparkle(P - star_sp(i), 7.0 * pop) * t * pulse;
        c = light(c, mix(uStar, uInk, 0.5) * s * (1.0 - win(p, 0.75, 0.92)));
    }
    // 広がる円の縁
    let rim = m * (1.0 - m) * 4.0;
    c = light(c, mix(uStar, uA, 0.4) * rim * 0.9);
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
