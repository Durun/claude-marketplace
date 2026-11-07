// 光漏れ: フィルムの光漏れのように明るく飛んで場面が入れ替わる（元: fx/leak.js、4 拍）。
// 暖かい光と差し色の光が横切って明るく飛び、その間に場面がゆっくり入れ替わる。
const SHUTTER: f32 = 0.5;

fn blobs(p: f32) -> vec3<f32> {
    let t = p;
    var col = vec3<f32>(0.0);
    let uv = P / 1080.0;
    for (var k = 0; k < 4; k++) {
        let fk = f32(k);
        let c = vec2<f32>(mix(-0.6, 2.4, t * (0.8 + 0.25 * fk)) + 0.3 * sin(fk * 2.1), 0.5 + 0.35 * sin(fk * 1.7 + t * 2.0));
        let d = (uv - c) / vec2<f32>(0.75 + 0.3 * fk, 0.55);
        let g = exp(-dot(d, d) * 1.6);
        var tint = uB;
        if (k == 0) { tint = vec3<f32>(1.0, 0.62, 0.32); }
        else if (k == 1) { tint = uA; }
        else if (k == 2) { tint = uStar; }
        col += tint * g * select(1.0, 0.5, k == 3);
    }
    return col;
}

fn mix_at(p: f32) -> f32 { return (p - 0.5) * 2.6 + (fbm(P / 700.0 + uSeed) - 0.5) * 0.9; }

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.9) { return 1.0; }
    if (p < 0.1) { return 0.0; }
    return smoothstep(-0.25, 0.25, mix_at(p));
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    var env = bump(p, 0.0, 0.5, 1.0);
    env = env * env * (3.0 - 2.0 * env);
    if (env <= 0.0) { return vec4<f32>(0.0); }
    var b = blobs(p) * env * 0.95;
    // 横に流れるアナモルフィックの筋
    let streak = exp(-abs(P.y - 540.0 - 120.0 * sin(p * 3.0)) / 18.0) * 0.45 * env;
    b += mix(uStar, uInk, 0.4) * streak;
    // 真ん中で白く飛ぶ
    let white = sat((env - 0.75) * 4.0);
    var c = vec4<f32>(0.0);
    c = light(c, b);
    c = over(c, paint(mix(uStar, uInk, 0.6), white * 0.6));
    // フィルムの粒と埃
    let grain = hash21(floor(P / 2.0) + floor(uT * 30.0) * 7.0) - 0.5;
    c = light(c, vec3<f32>(0.06) * max(grain, 0.0) * env);
    let dust = step(0.9993, hash21(floor(P / 3.0) + floor(uT * 15.0) * 13.0));
    c = over(c, paint(uInk, dust * 0.7 * env));
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
