// リキッド: 液体のような境目が流れて次の場面へ。気分の切り替え、ゆったりした作業、雰囲気のある場面に（元: fx/liquid.js、4 拍）。
// ゆらぐ液体の境目が斜めに流れ、境目に差し色と 2 つ目の差し色の光の縁。
const SHUTTER: f32 = 0.5;

fn field_at(pos: vec2<f32>) -> f32 {
    let uv = pos / 1080.0;
    let w = uv + 0.22 * vec2<f32>(fbm(uv * 1.6 + uT * 0.35 + uSeed), fbm(uv * 1.6 + 7.3 - uT * 0.3));
    let n = fbm(w * 2.2);
    let dir = dot(pos / REF - 0.5, normalize(vec2<f32>(1.0, 0.45)));
    return dir * 1.25 + (n - 0.5) * 0.55;
}
fn front(p: f32) -> f32 { return mix(-1.05, 1.05, io3(win(p, 0.02, 0.96))); }

// 境目からの距離（基準の画素）。次の場面の側が負。場の勾配は画面の 1 画素あたりの差（dpdx、dpdy）を基準の画素に直して使う。
fn dist(p: f32) -> f32 {
    let f0 = field_at(P);
    let g = length(vec2<f32>(dpdx(f0), dpdy(f0))) / PX;
    return (f0 - front(p)) / max(g, 1e-5);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.97) { return 1.0; }
    if (p < 0.03) { return 0.0; }
    return fill(dist(p));
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.08) * (1.0 - win(p, 0.9, 1.0));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let d = dist(p);
    var c = vec4<f32>(0.0);
    // 前の場面の側に、差し色の光と 2 つ目の差し色の帯、白い芯
    let ahead = step(0.0, d);
    c = light(c, uA * glow(d, 26.0) * 0.8 * ahead * env);
    c = over(c, paint(uB, fill(abs(d - 16.0) - 5.0) * 0.85 * env));
    c = over(c, paint(mix(uA, uInk, 0.3), fill(abs(d - 5.0) - 3.5) * env));
    c = light(c, uInk * fill(abs(d - 1.0) - 1.0) * 0.8 * env);
    // 次の場面の側のにじみ
    c = light(c, uA * glow(-d, 18.0) * 0.25 * (1.0 - ahead) * env);
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
