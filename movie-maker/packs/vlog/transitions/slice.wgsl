// スライス: 斜めの帯（差し色・白・濃紺）が段違いに右へ走り抜け、抜けた後ろから次の場面。どこにでも合う基本のワイプ（元: fx/slice.js、2 拍）。
// 先端は差し色から、後端は濃紺から順に抜ける。
const SHUTTER: f32 = 0.3;

const SLANT: f32 = 0.36;
fn band_x(p: f32) -> f32 { return mix(-760.0, 2700.0, p); }
fn slant_x() -> f32 { return P.x + (P.y - 540.0) * SLANT; }
fn row_delay() -> f32 {
    let r = floor(P.y / 216.0);
    return hash11(r * 3.71 + uSeed * 1.3) * 0.06;
}
fn lead_e(p: f32, d: f32, k: f32) -> f32 { let a = d + k * 0.04; return band_x(io3(win(p, a, a + 0.52))); }
fn tail_e(p: f32, d: f32, k: f32) -> f32 { let a = 0.30 + d + (2.0 - k) * 0.05; return band_x(io3(win(p, a, a + 0.54))); }

fn matte(uv: vec2<f32>, p: f32) -> f32 { return fill(slant_x() - tail_e(p, row_delay(), 0.0)); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let d = row_delay();
    let x = slant_x();
    var c = vec4<f32>(0.0);
    var cols = array<vec3<f32>, 3>(uA, uInk, uShade);
    for (var k = 0; k < 3; k++) {
        let fk = f32(k);
        let l = lead_e(p, d, fk);
        let t = tail_e(p, d, fk);
        let cov = fill(x - l) * fill(t - x);
        var col = cols[k];
        if (k == 2) {
            // 濃紺の帯は先端側を少し明るく、先端に細い差し色の線
            let g = sat((x - t) / max(l - t, 1.0));
            col = mix(uShade * 0.7, mix(uShade, uA, 0.18), g);
            col = mix(col, uA, fill(abs(x - l + 5.0) - 2.0) * 0.9);
        }
        c = over(c, paint(col, cov));
    }
    return c;
}

fn motion(p: f32) -> Motion2 {
    var m = still();
    m.a.x = -90.0 * in3(win(p, 0.0, 0.6));
    return m;
}
