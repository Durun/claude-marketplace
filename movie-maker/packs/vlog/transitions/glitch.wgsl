// グリッチ: ブロックノイズで化けながら次の場面へ。失敗、バグ、ツッコミ、テンションの切り替えに（元: fx/glitch.js、2 拍）。
// 2 コマごとに組み替わるブロックのかたまりで B へ化け、差し色のブロック、走査線、横に裂ける線が混ざる。映像も 2 コマごとにずれて跳ねる。
const SHUTTER: f32 = 0.0;

fn stepN() -> f32 { return floor(uT * 15.0); }
fn grid(s: f32) -> vec2<f32> {
    let h = hash11(s * 3.1 + uSeed);
    return select(select(select(vec2<f32>(24.0, 36.0), vec2<f32>(8.0, 18.0), h < 0.75), vec2<f32>(32.0, 18.0), h < 0.5), vec2<f32>(16.0, 9.0), h < 0.25);
}
fn prob(p: f32) -> f32 { return sat(0.04 + 1.1 * io3(win(p, 0.08, 0.8))); }
fn cellOf(s: f32) -> vec2<f32> {
    let g = grid(s);
    let uv = P / REF;
    let band = floor(uv.y * g.y * 0.5);
    let shift = (hash11(band * 7.3 + s) - 0.5) * 0.25 * step(0.6, hash11(band + s * 1.7));
    return floor(vec2<f32>(uv.x + shift, uv.y) * g);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.86) { return 1.0; }
    let s = stepN();
    let r = hash21(cellOf(s) + s * 13.1);
    return step(r, prob(p));
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let s = stepN();
    let amt = bump(p, 0.0, 0.45, 0.9);
    if (amt <= 0.0) { return vec4<f32>(0.0); }
    let cell = cellOf(s);
    let r2 = hash21(cell * 1.7 + s * 5.3);
    var c = vec4<f32>(0.0);
    // 差し色のブロック
    if (r2 < 0.09 * amt) {
        let k = hash21(cell + 9.0 + s);
        let col = select(select(select(uShade, uInk, k < 0.85), uB, k < 0.7), uA, k < 0.4);
        let scan = 0.75 + 0.25 * step(0.5, fract(P.y / 4.0));
        c = paint(col * scan, 0.92);
    }
    // 横に裂ける線
    let ly = hash11(s * 1.3 + 2.0) * 1080.0;
    let ly2 = hash11(s * 2.9 + 5.0) * 1080.0;
    var ln = max(fill(abs(P.y - ly) - 1.5), fill(abs(P.y - ly2) - 3.0) * step(0.5, hash11(s + 4.0)));
    let lx0 = hash11(s * 4.1) * 1920.0;
    ln *= step(lx0, P.x) * step(P.x, lx0 + 600.0 + 900.0 * hash11(s * 8.0));
    c = over(c, paint(mix(uInk, uA, step(0.5, hash11(s * 6.0))), ln * 0.9 * amt));
    // 色ずれの縁（ブロックの端に差し色と 2 つ目の差し色）
    let g = grid(s);
    let f = fract(P / REF * g);
    let edgeX = min(f.x, 1.0 - f.x) * 1920.0 / g.x;
    let isB = step(hash21(cell + s * 13.1), prob(p));
    let fringe = fill(edgeX - 3.0) * isB * step(0.6, hash21(cell * 3.3 + s)) * amt;
    c = over(c, paint(select(uB, uA, f.x < 0.5), fringe * 0.8));
    // 細かいノイズの粒
    let px = hash21(floor(P / 3.0) + s * 17.0);
    c = over(c, paint(uInk, step(0.996, px) * amt));
    // 全体の走査線
    c = over(c, paint(vec3<f32>(0.0), 0.18 * amt * step(0.5, fract(P.y / 3.0))));
    return c;
}

fn motion(p: f32) -> Motion2 {
    // 2 コマごとに乱数の種を替えて、映像を跳ねさせる。
    let s = floor(p * uDur * 15.0);
    let k = s * 31.0 + 7.0 + uSeed;
    let amt = select(max(0.0, 1.0 - (p - 0.45) / 0.45), p / 0.45, p < 0.45);
    let hit = hash11(k * 1.37) < 0.6 * amt;
    var m = still();
    if (hit) {
        m.a.x = (hash11(k * 2.11) - 0.5) * 2.0 * 70.0 * amt;
        m.a.y = (hash11(k * 3.17) - 0.5) * 2.0 * 8.0 * amt;
        m.a.sx = 1.0 + 0.05 * amt * hash11(k * 4.23);
        m.a.sy = 1.0 + 0.05 * amt * hash11(k * 5.29);
        m.b = m.a;
    }
    return m;
}
