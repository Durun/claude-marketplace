// ヘックス: 六角形のタイルが波のように裏返って次の場面へ。SF の部屋、システムの画面、ツールの切り替えに（元: fx/hex.js、3 拍）。
// 宇宙船の画面のような六角形のタイルが、走査の光に続いて左から次々と裏返り、次の場面を見せる。
const SHUTTER: f32 = 0.4;

const HR: f32 = 64.0;

fn fmodv(x: vec2<f32>, y: vec2<f32>) -> vec2<f32> { return x - y * floor(x / y); }

struct HexCell { cen: vec2<f32>, local: vec2<f32> }
// 六角形の格子（尖った頭が上）の、この画素のタイルの中心と、中心からの位置
fn hexCell() -> HexCell {
    let s = vec2<f32>(1.7320508 * HR, 1.5 * HR);
    let per = vec2<f32>(s.x, 2.0 * s.y);
    let a = fmodv(P, per) - per * 0.5;
    let b = fmodv(P - vec2<f32>(s.x * 0.5, s.y), per) - per * 0.5;
    let g = select(b, a, dot(a, a) < dot(b, b));
    return HexCell(P - g, g);
}

fn trigAt(c: vec2<f32>) -> f32 {
    return 0.06 + 0.55 * (c.x / 1920.0) + 0.1 * hash21(floor(c / 10.0)) + 0.04 * sin(c.y * 0.01);
}

struct Tile { d: f32, cen: vec2<f32>, a: f32 }
fn tile(p: f32) -> Tile {
    let hc = hexCell();
    let t0 = trigAt(hc.cen);
    let a = win(p, t0, t0 + 0.16);
    let s = outBack(a);
    let g = win(p, 0.82, 0.94);
    // 尖った頭が上の六角形（sdHex は平らな頭が上なので回す）
    let lr = vec2<f32>(hc.local.y, hc.local.x);
    let size = mix(HR * 0.866 - 3.0, HR * 0.866 + 2.0, g) * s;
    return Tile(select(sdHex(lr, size), 1e3, a <= 0.0), hc.cen, a);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.95) { return 1.0; }
    return fill(tile(p).d);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.06) * (1.0 - win(p, 0.85, 0.96));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let tl = tile(p);
    let sd = tl.d;
    let a = tl.a;
    let inT = fill(sd);
    let hc = hexCell();
    let emptyD = sdHex(vec2<f32>(hc.local.y, hc.local.x), HR * 0.866 - 3.0);
    let t0 = trigAt(tl.cen);
    // 裏返る前に枠が点く
    let pre = win(p, t0 - 0.12, t0) * (1.0 - inT);
    var c = paint(uShade, 0.35 * env * (1.0 - inT) * win(p, t0 - 0.2, t0));
    c = over(c, paint(uA, fill(abs(emptyD) - 1.2) * pre * 0.9 * env));
    // 裏返った瞬間の光と、残る縁
    let flash = exp(-a * 7.0) * step(0.001, a);
    c = over(c, paint(mix(uA, uInk, 0.6), flash * 0.75 * inT * env));
    let rim = inT * (1.0 - fill(sd + 3.0));
    c = over(c, paint(mix(uA, uInk, 0.3), rim * (0.15 + 0.85 * flash) * env * (1.0 - win(p, 0.8, 0.92))));
    // 走査の光（波の先頭）
    let fx = (p - 0.06 - 0.02) / 0.55 * 1920.0;
    let dx = P.x - fx;
    c = light(c, (uA * glow(abs(dx), 30.0) * 0.35 + uInk * fill(abs(dx) - 1.0) * 0.5) * env * (1.0 - win(p, 0.6, 0.7)));
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
