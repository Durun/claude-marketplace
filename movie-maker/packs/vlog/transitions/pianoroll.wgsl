// ピアノロール: プレイヘッドが走って音符が書き込まれ、音符の中から次の場面。打ち込み、メロディ、MIDI の話題へ（元: fx/pianoroll.js、3 拍）。
// 最後に残りの隙間が音符で埋まって切り替わる。
const SHUTTER: f32 = 0.35;

const CW: f32 = 60.0;
const RH: f32 = 45.0;
const COLS: i32 = 32;
const ROWS: i32 = 24;
// プレイヘッドは 2 拍（3 拍のうち）で横切る。
const PH0: f32 = 0.04;
const PH1: f32 = 0.70666;

// 調の主音の音名（0..11）。種から決める。
fn root_pc() -> f32 { return floor(hash11(uSeed * 0.37 + 3.1) * 12.0); }
fn ph_p(col: i32) -> f32 { return PH0 + (f32(col) / f32(COLS)) * (PH1 - PH0); }
fn ph_x(p: f32) -> f32 { return 1920.0 * win(p, PH0, PH1); }
fn end_grow(p: f32) -> f32 { return win(p, 0.84, 0.96); }
fn fmodf(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }

// メロディ 8 音（16 分音符、4 列ごと）の行。主音からのペンタトニックの高さ（半音）を行に直す。
fn mel_row(j: i32) -> i32 {
    var offs = array<i32, 8>(0, 4, 9, 12, 16, 14, 19, 21);
    return ROWS - 1 - offs[j];
}
fn mel_len(j: i32) -> i32 {
    let idx = floor(hash11(f32(j) * 7.3 + uSeed * 1.7) * 4.0);
    return 2 + i32(step(1.0, idx) + step(3.0, idx));
}
fn in_scale(row: i32) -> bool {
    let off = (ROWS - 1 - row) % 12;
    return off == 0 || off == 2 || off == 4 || off == 7 || off == 9;
}
fn black_key(row: i32) -> bool {
    let pc = (i32(root_pc()) + ROWS - 1 - row + 240) % 12;
    return pc == 1 || pc == 3 || pc == 6 || pc == 8 || pc == 10;
}

// セルの音符を (始まりの列, 終わりの列, 現れる進み具合, 種類) で返す。種類 1 は書き込まれる音、2 は隙間を埋める音。
// 行ごとに左から順に音符を切り出し、メロディの音の所はメロディにする。
fn cell_note(col: i32, row: i32) -> vec4<f32> {
    var mj = -1;
    for (var j = 0; j < 8; j++) {
        if (mel_row(j) == row) { mj = j; }
    }
    var lens = array<i32, 8>(1, 2, 2, 3, 4, 4, 6, 8);
    var c0 = 0;
    for (var it = 0; it < COLS; it++) {
        if (c0 >= COLS) { break; }
        var c1 = COLS;
        var appear = 0.0;
        var kind = 2.0;
        if (mj >= 0 && c0 >= mj * 4 && c0 < mj * 4 + mel_len(mj)) {
            c1 = min(COLS, mj * 4 + mel_len(mj));
            appear = ph_p(c0);
            kind = 1.0;
        } else {
            let key = vec2<f32>(f32(row) * 1.37 + uSeed * 0.11, f32(c0) * 3.17 + 0.5);
            let h = hash21(key);
            c1 = min(COLS, c0 + lens[min(7, i32(h * 8.0))]);
            if (mj >= 0 && mj * 4 > c0) { c1 = min(c1, mj * 4); }
            let h2 = hash22(key + 11.7);
            if (in_scale(row) && h2.x < 0.3) {
                appear = ph_p(c0);
                kind = 1.0;
            } else {
                appear = 0.62 + h2.y * 0.2;
            }
        }
        if (col < c1) { return vec4<f32>(f32(c0), f32(c1), appear, kind); }
        c0 = c1;
    }
    return vec4<f32>(0.0, f32(COLS), 0.0, 2.0);
}

struct NoteR { sd: f32, d: vec4<f32>, bx: vec4<f32> }

// この画素の音符の形（符号つき距離）と、音符の情報。
fn note(p: f32) -> NoteR {
    let col = i32(clamp(floor(P.x / CW), 0.0, f32(COLS - 1)));
    let row = i32(clamp(floor(P.y / RH), 0.0, f32(ROWS - 1)));
    var r: NoteR;
    r.sd = 1e3;
    r.d = cell_note(col, row);
    r.bx = vec4<f32>(0.0);
    let g = end_grow(p);
    var x0 = r.d.x * CW;
    var x1 = r.d.y * CW;
    var y0 = f32(row) * RH;
    var y1 = y0 + RH;
    if (r.d.w < 1.5) {
        let ph = ph_x(p);
        if (ph <= x0 && g <= 0.0) { return r; }
        x1 = mix(min(x1, max(ph, x0)), x1, g);
    } else {
        let s = outBack(win(p, r.d.z, r.d.z + 0.09));
        if (s <= 0.0) { return r; }
        let c = vec2<f32>(x0 + x1, y0 + y1) * 0.5;
        let h = vec2<f32>(x1 - x0, y1 - y0) * 0.5 * s;
        x0 = c.x - h.x; x1 = c.x + h.x; y0 = c.y - h.y; y1 = c.y + h.y;
    }
    let inset = mix(2.5, -1.5, g);
    let c = vec2<f32>(x0 + x1, y0 + y1) * 0.5;
    let h = max(vec2<f32>(x1 - x0, y1 - y0) * 0.5 - inset, vec2<f32>(0.0));
    r.bx = vec4<f32>(x0, y0, x1, y1);
    r.sd = sdRBox(P - c, h, 5.0 * (1.0 - g));
    return r;
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.97) { return 1.0; }
    return fill(note(p).sd);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.1) * (1.0 - win(p, 0.86, 0.95));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let nr = note(p);
    let d = nr.d;
    let b = nr.bx;
    let sd = nr.sd;
    let inN = fill(sd);
    let row = i32(clamp(floor(P.y / RH), 0.0, f32(ROWS - 1)));
    // 背景（音符のない所）: 濃紺の地、黒鍵の行は少し暗く、拍の線と 16 分の線
    let bk = select(0.0, 1.0, black_key(row));
    var bg = mix(uShade * 1.25, uShade * 0.8, bk);
    let gx = fmodf(P.x, CW);
    let gy = fmodf(P.y, RH);
    let beat_line = select(0.0, 0.20, fmodf(P.x, CW * 4.0) < 2.0);
    let lines = max(beat_line, max(select(0.0, 0.08, gx < 1.0), select(0.0, 0.07, gy < 1.0)));
    bg = mix(bg, uInk, lines);
    var c = paint(bg, 0.8 * env * (1.0 - inN));
    // 音符: 書き込まれた音は差し色、埋める音は少し沈めた色。縁と上の光
    let played = select(0.0, 1.0, d.w < 1.5);
    let nc = mix(mix(uA, uShade, 0.35), uA, played);
    let top_hi = sat(1.0 - (P.y - b.y) / 14.0);
    let rim = inN * (1.0 - fill(sd + 2.5));
    let na = (0.30 + 0.18 * played + 0.25 * top_hi) * inN;
    c = over(c, paint(mix(nc, uInk, 0.35 * top_hi), na * env));
    c = over(c, paint(mix(uA, uInk, 0.55 * played), rim * 0.95 * env));
    // プレイヘッドが通った瞬間の光
    let ph = ph_x(p);
    if (played > 0.5) { c = light(c, uInk * inN * 0.55 * exp(-max(ph - b.x, 0.0) / 50.0) * env); }
    // プレイヘッド
    let pe = env * (1.0 - win(p, PH1, PH1 + 0.04));
    let dx = abs(P.x - ph);
    c = light(c, (uInk * fill(dx - 1.2) + uA * glow(dx, 4.0) * 0.6) * pe);
    let tri = P - vec2<f32>(ph, 0.0);
    c = over(c, paint(uInk, fill(max(abs(tri.x) - (18.0 - tri.y), tri.y - 18.0)) * pe));
    // 左の鍵盤。プレイヘッドの下で鳴っている行は差し色
    let kx = -84.0 * (1.0 - out3(win(p, 0.0, 0.12))) - 84.0 * in3(win(p, 0.8, 0.9));
    let lx = P.x - kx;
    if (lx < 84.0 && lx > -2.0) {
        let pd = cell_note(i32(clamp(floor(ph / CW), 0.0, f32(COLS - 1))), row);
        let sounding = pd.w < 1.5 && pd.x * CW <= ph && ph < pd.y * CW && pe > 0.0;
        var key = select(vec3<f32>(0.90, 0.93, 0.95), vec3<f32>(0.09, 0.11, 0.13), bk > 0.5);
        if (sounding) { key = mix(uA, uInk, 0.25); }
        let sep = select(0.0, 0.35, gy < 1.5);
        let key_w = select(84.0, 58.0, bk > 0.5);
        c = over(c, paint(select(key, vec3<f32>(0.9, 0.93, 0.95), bk > 0.5), fill(lx - 84.0)));
        c = over(c, paint(key, fill(lx - key_w) * (1.0 - sep)));
        c = over(c, paint(vec3<f32>(0.0), fill(abs(lx - 84.0) - 1.0) * 0.6));
    }
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
