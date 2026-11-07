// レコード: 回るレコード盤が広がり、ラベルの中の次の場面が開く。曲の再生、通しで聴く、完成品を見せる前に（元: fx/vinyl.js、3 拍）。
// 盤が中心から大きくなって前の場面を覆い、ラベル（真ん中の丸）の中にある次の場面が広がって画面になる。溝の光が回る。
const SHUTTER: f32 = 0.5;

const DIAG: f32 = 1102.0;

fn fmodg(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }

fn rDisc(p: f32) -> f32 { return DIAG * 1.08 * out5(win(p, 0.0, 0.4)); }
fn rLabel(p: f32) -> f32 { return mix(rDisc(p) * 0.34, DIAG + 20.0, io3(win(p, 0.36, 0.86))); }
fn spin(p: f32) -> f32 { return TAU * (0.4 * p + 1.2 * in2(p)); }

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.9) { return 1.0; }
    return fill(length(P - vec2<f32>(960.0, 540.0)) - rLabel(p));
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let q = P - vec2<f32>(960.0, 540.0);
    let r = length(q);
    let ang = atan2(q.y, q.x);
    let rd = rDisc(p);
    let rl = rLabel(p);
    // 盤の影
    var c = paint(vec3<f32>(0.0), 0.5 * glow(r - rd, 40.0) * step(rd, r) * (1.0 - win(p, 0.8, 0.95)));
    // 盤（ラベルと縁の間）: 黒、溝、回る光
    let disc = fill(r - rd) * (1.0 - fill(r - rl));
    let grooves = 0.5 + 0.5 * sin(r * 1.6);
    // 曲間の無音の溝
    let track = fill(abs(fmodg(r - rl, 110.0) - 55.0) - 52.0);
    let sp = spin(p);
    let sheen = pow(abs(cos(ang - sp)), 14.0) + 0.6 * pow(abs(cos(ang - sp + 1.2)), 30.0);
    var vc = vec3<f32>(0.035, 0.04, 0.05) + vec3<f32>(0.03) * grooves - vec3<f32>(0.02) * track;
    vc += mix(uInk, uA, 0.4) * sheen * (0.2 + 0.1 * grooves);
    c = over(c, paint(vc, disc));
    // 縁の光
    c = light(c, mix(uInk, uA, 0.5) * fill(abs(r - rd) - 1.2) * 0.6 * step(1.0, rd));
    // ラベルの縁（差し色と 2 つ目の差し色の輪）と、ラベルに印刷した色
    let lab = 1.0 - fill(r - rl);
    let ring = fill(abs(r - rl) - 5.0);
    let ring2 = fill(abs(r - (rl - 16.0)) - 1.5);
    let fade = 1.0 - win(p, 0.55, 0.85);
    c = over(c, paint(uA, ring * sat(rd / 40.0)));
    c = over(c, paint(uB, ring2 * fade * step(20.0, rl)));
    c = over(c, paint(uA, 0.28 * (1.0 - lab) * fade * step(r, rl - 18.0) * fill(-r + rl - 18.0)));
    // ラベルの回転の目印（半径の線）
    let mk = abs(sin(ang - sp)) * r;
    c = over(c, paint(uInk, fill(mk - 2.0) * step(rl * 0.45, r) * step(r, rl - 26.0) * step(0.0, cos(ang - sp)) * 0.35 * fade));
    // 真ん中の穴
    c = over(c, paint(uShade, fill(r - 11.0) * (1.0 - win(p, 0.46, 0.55)) * step(1.0, rd)));
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
