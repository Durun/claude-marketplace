// スリープ: 画面が光の線と点に縮んで眠り、小さな光が一度明滅してから次の場面が開く。作業の中断と再開、日をまたぐとき（元: fx/boot.js、4 拍）。
// ブラウン管を消すように映像が横一本の光に縮み、点になって、眠っている PC の小さな光のようにゆっくり一度だけ明滅する。そこから光が横に広がり、B が縦に開く。
const SHUTTER: f32 = 0.5;

// A: 0〜0.22 で縦につぶれ、0.22〜0.34 で横に縮んで点に。B: 0.62〜0.72 で横に線が伸び、0.72〜0.88 で縦に開く
fn sizeA(p: f32) -> vec2<f32> {
    let sy = mix(1.0, 0.004, in3(win(p, 0.0, 0.22)));
    let sx = (1.0 + 0.04 * win(p, 0.0, 0.22)) * (1.0 - in2(win(p, 0.22, 0.34)));
    return vec2<f32>(sx, sy);
}
fn sizeB(p: f32) -> vec2<f32> {
    let sx = out3(win(p, 0.62, 0.72));
    let t = win(p, 0.72, 0.88);
    let sy = mix(0.004, 1.0, outBack(t));
    return vec2<f32>(sx, select(0.004, sy, t > 0.0));
}

fn matte(uv: vec2<f32>, p: f32) -> f32 { return step(0.5, p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let q = P - vec2<f32>(960.0, 540.0);
    let s = select(sizeB(p), sizeA(p), p < 0.5);
    let hh = vec2<f32>(960.0, 540.0) * s;
    var c = vec4<f32>(0.0);
    // 縮んでいる映像の縁を白く焼く（細くなるほど明るい）
    let thin = sat(1.0 - hh.y / 60.0) * select(win(p, 0.6, 0.63), 1.0 - win(p, 0.3, 0.36), p < 0.5);
    let d = sdBox(q, max(hh, vec2<f32>(0.5)));
    let ln = fill(d) * thin;
    c = light(c, mix(uInk, uA, 0.25) * ln * 1.2);
    c = light(c, uA * glow(d, 16.0) * thin * 0.9 + uInk * glow(d, 4.0) * thin * 0.6);
    // 眠っている間の小さな光（点）。ゆっくり一度明滅する
    let live = win(p, 0.3, 0.36) * (1.0 - win(p, 0.62, 0.66));
    let breath = 0.35 + 0.65 * sin(PI * win(p, 0.34, 0.62));
    let r = length(q);
    let led = mix(uStar, uA, 0.35);
    c = light(c, (uInk * fill(r - 4.0) + led * glow(r - 3.0, 10.0) * 0.9 + led * glow(r, 60.0) * 0.25) * live * breath);
    // 次の場面が開いた後の、ブラウン管の走査線とちらつき
    let crt = win(p, 0.72, 0.76) * (1.0 - win(p, 0.86, 1.0));
    let flick = 0.6 + 0.4 * hash11(floor(uT * 30.0));
    c = over(c, paint(vec3<f32>(0.0), 0.28 * crt * flick * step(0.5, fract(P.y / 4.0))));
    c = light(c, uA * 0.08 * crt * flick);
    return c;
}

fn motion(p: f32) -> Motion2 {
    var m = still();
    if (p < 0.5) {
        let sa = sizeA(p);
        m.a.sx = max(sa.x, 0.0001);
        m.a.sy = sa.y;
        m.a.op = select(0.0, 1.0, p < 0.34);
        m.b.op = 0.0;
    } else {
        let sb = sizeB(p);
        m.a.op = 0.0;
        m.b.sx = max(sb.x, 0.0001);
        m.b.sy = sb.y;
        m.b.op = select(1.0, 0.0, p < 0.62);
    }
    return m;
}
