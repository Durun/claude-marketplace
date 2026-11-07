// ステップシーケンサー: 16 ステップのパッドが列ごとに点灯して次の場面へ（元: fx/stepseq.js、3 拍）。
// 16 列 x 9 行のパッドが、プレイヘッドの列ごとにポンと点いて次の場面を見せる。
const SHUTTER: f32 = 0.35;

const SQ_S: f32 = 120.0;
// プレイヘッドの走る長さは 170 BPM の 2 拍ぶん。
const SQ_BEAT: f32 = 60.0 / 170.0;

// プレイヘッドの走る範囲（進み具合の始めと終わり）。
fn sq_range() -> vec2<f32> {
    let s0 = 0.04;
    return vec2<f32>(s0, min(0.8, s0 + 2.0 * SQ_BEAT / max(uDur, 1e-3)));
}
fn fill_end(p: f32) -> f32 { return win(p, 0.8, 0.93); }

// セルごとの値。x: 点く進み具合（列ごとに順に点く）、y: 打ち込まれたステップか、z: 色の振り分け。
fn cell_data(cell: vec2<i32>) -> vec4<f32> {
    let r = sq_range();
    let k = vec2<f32>(cell) + vec2<f32>(uSeed * 13.0, uSeed * 7.0);
    let at = r.x + (f32(cell.x) / 16.0) * (r.y - r.x) + hash21(k + 3.7) * 0.035;
    let lit = select(0.0, 1.0, hash21(k + 11.3) < 0.28);
    return vec4<f32>(at, lit, hash21(k + 23.9), 0.0);
}

struct Pad { sd: f32, d: vec4<f32>, a: f32 }

fn sq_cell() -> vec2<i32> {
    return vec2<i32>(i32(clamp(floor(P.x / SQ_S), 0.0, 15.0)), i32(clamp(floor(P.y / SQ_S), 0.0, 8.0)));
}

fn pad(p: f32) -> Pad {
    let cell = sq_cell();
    let d = cell_data(cell);
    let a = win(p, d.x, d.x + 0.07);
    let s = outBack(a);
    let g = fill_end(p);
    let c = (vec2<f32>(cell) + 0.5) * SQ_S;
    let h = mix(SQ_S * 0.5 - 7.0, SQ_S * 0.5 + 1.0, g) * s;
    let sd = select(sdRBox(P - c, vec2<f32>(h), 14.0 * (1.0 - g) * s), 1e3, a <= 0.0);
    return Pad(sd, d, a);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.95) { return 1.0; }
    return fill(pad(p).sd);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.08) * (1.0 - win(p, 0.84, 0.95));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let pd = pad(p);
    let d = pd.d;
    let a = pd.a;
    let sd = pd.sd;
    let in_p = fill(sd);
    let cell = sq_cell();
    let lc = P - (vec2<f32>(cell) + 0.5) * SQ_S;
    // まだ点いていないパッドの枠と、暗い地（4 列ごとに色を変える）
    let empty = sdRBox(lc, vec2<f32>(SQ_S * 0.5 - 7.0), 14.0);
    var c = paint(uShade, 0.66 * env * (1.0 - in_p));
    let group_tint = select(0.05, 0.10, (cell.x / 4) % 2 == 0);
    c = over(c, paint(uInk, (group_tint * fill(empty) + 0.28 * fill(abs(empty) - 1.0)) * env * (1.0 - in_p)));
    // プレイヘッドの列
    let r = sq_range();
    let col = floor(16.0 * win(p, r.x, r.y));
    let on_col = select(0.0, 1.0, f32(cell.x) == col && p < r.y + 0.02);
    c = light(c, uA * 0.16 * on_col * env);
    // 点いたパッド: 白く光ってから、打ち込まれたステップは色が残る
    let flash = exp(-a * 9.0) * step(0.0, a) * sat(a * 40.0);
    let lit = select(uA, mix(uA, uB, step(0.5, d.z)), d.y > 0.5);
    let keep = select(0.16, 0.45, d.y > 0.5);
    c = over(c, paint(mix(lit, uInk, flash), (keep + 0.6 * flash) * in_p * env));
    let rim = in_p * (1.0 - fill(sd + 3.0));
    c = over(c, paint(mix(uA, uInk, 0.4), rim * 0.9 * env * (1.0 - fill_end(p))));
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
