// 巻き戻し: 大きな「<」の矢印が右から左へ流れ、画面を覆っている間に場面が切り替わる（元: design/templates/rewind.html、
// 1.6 秒）。差し色と白の筋を、前の縁は 1〜2 コマ先に、後ろの縁は 1〜2 コマ遅れて流す（主役は 2 コマ遅れ）。
// 主役が覆っている間に ◀◀ を出す。前の場面を逆向きに再生する見た目は、パラメータ rewind（倍率）を見てレンダラーが作る。
// 元の、戻っていく時刻（t0 → t7）の表示は文字が要るので描かない。
const SHUTTER: f32 = 0.0;

// 元の長さ（秒）。元の時刻を進み具合に直す。
const TOTAL: f32 = 1.6;
// 矢印が流れ切る長さと、流れる距離（基準の画素）。
const SWEEP: f32 = 1.4;
const TRAVEL: f32 = 5760.0;
// 場面を切り替える時刻（元の theme.yaml の rewind.cut_at）。
const CUT_AT: f32 = 0.77;
// ◀◀ を出している時刻。
const OSD_ON: f32 = 0.483;
const OSD_OFF: f32 = 1.05;

// CSS の cubic-bezier(0.2, 0.8, 0.8, 0.2)。x(s) = t をニュートン法で解いて y(s) を返す。
fn bez(s: f32, a: f32, b: f32) -> f32 {
    let r = 1.0 - s;
    return 3.0 * r * r * s * a + 3.0 * r * s * s * b + s * s * s;
}
fn bez_d(s: f32, a: f32, b: f32) -> f32 {
    let r = 1.0 - s;
    return 3.0 * r * r * a + 6.0 * r * s * (b - a) + 3.0 * s * s * (1.0 - b);
}
fn ease(t: f32) -> f32 {
    var s = t;
    for (var i = 0; i < 6; i++) {
        s = clamp(s - (bez(s, 0.2, 0.8) - t) / max(bez_d(s, 0.2, 0.8), 1e-3), 0.0, 1.0);
    }
    return bez(s, 0.8, 0.2);
}

// 矢印 1 枚の符号つき距離（基準の画素、内側が負）。delay 秒遅れて動き出す。
fn arrow(secs: f32, delay: f32) -> f32 {
    let k = ease(sat((secs - delay) / SWEEP));
    let local = P - vec2<f32>(1920.0 - TRAVEL * k, 0.0);
    let dy = abs(local.y - 540.0);
    let d = max(dy - local.x, local.x - 3300.0 - dy) * 0.70710678;
    return max(d, max(-local.y, local.y - 1080.0));
}

// 左向きの三角（先端が x0、幅 w、高さ h）の符号つき距離。
fn tri_left(q: vec2<f32>, x0: f32, w: f32, h: f32) -> f32 {
    let lx = q.x - x0;
    let slope = h * 0.5 / w;
    let side = (abs(q.y) - lx * slope) / sqrt(1.0 + slope * slope);
    return max(side, lx - w);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 { return step(CUT_AT / TOTAL, p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let secs = p * TOTAL;
    var c = vec4<f32>(0.0);
    c = over(c, paint(uA, fill(arrow(secs, 0.0))));
    c = over(c, paint(uInk, fill(arrow(secs, 0.033))));
    c = over(c, paint(uA, fill(arrow(secs, 0.133))));
    c = over(c, paint(uInk, fill(arrow(secs, 0.1))));
    // 主役: 横のグラデーション（#06090d → Shade → #16222d）の矢印。
    let k = ease(sat((secs - 0.067) / SWEEP));
    let gx = sat((P.x - (1920.0 - TRAVEL * k)) / 3840.0);
    let dark0 = vec3<f32>(0.023529, 0.035294, 0.050980);
    let dark1 = vec3<f32>(0.086275, 0.133333, 0.176471);
    let shade = select(mix(uShade, dark1, (gx - 0.3) / 0.7), mix(dark0, uShade, gx / 0.3), gx < 0.3);
    c = over(c, paint(shade, fill(arrow(secs, 0.067))));
    // ◀◀（220px の太字の 2 文字を、字間を詰めて並べた大きさ）。
    let osd = step(OSD_ON, secs) * step(secs, OSD_OFF);
    let q = P - vec2<f32>(960.0, 540.0);
    let rew = min(tri_left(q, -150.0, 150.0, 170.0), tri_left(q, 0.0, 150.0, 170.0));
    c = over(c, paint(uA, fill(rew) * osd));
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
