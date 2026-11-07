// ワープ: 星が光の筋になって加速し、閃光を抜けると次の場面。大きな場面転換、日をまたぐ、作業の山場の前に（元: fx/warp.js、4 拍）。
// 星が中心から放射状に流れて白い閃光の中で次の場面へ抜ける。映像も前へ突っ込むように拡大してぼけ、次の場面は奥から減速して止まる。
const SHUTTER: f32 = 0.6;

// 進み具合 → いまの速さ
fn speed(p: f32) -> f32 {
    return select(0.15 + 3.2 * (1.0 - out3((p - 0.5) / 0.5)), 0.15 + 3.2 * in3(p / 0.5), p < 0.5);
}
// 星がどこまで進んだか（速さを積分したもの）
fn travel(p: f32) -> f32 {
    var s = 0.0;
    for (var k = 0; k < 24; k++) {
        let q = (f32(k) + 0.5) / 24.0 * p;
        s += speed(q);
    }
    return s * p / 24.0;
}

fn stars(p: f32) -> vec3<f32> {
    let q = (P - vec2<f32>(960.0, 540.0)) / 540.0;
    let r = length(q);
    let ang = atan2(q.y, q.x);
    let tr = travel(p) * 0.55;
    let sp = speed(p);
    let lr = log(r);
    var col = vec3<f32>(0.0);
    for (var L = 0; L < 4; L++) {
        let fl = f32(L);
        let bins = 150.0 + fl * 110.0;
        let a = (ang + PI) / TAU * bins + fl * 0.37;
        let cid = floor(a);
        let h = hash11(cid * 1.37 + fl * 71.0 + uSeed);
        let h2 = hash11(cid * 3.11 + fl * 13.0);
        // 星の位置は log(半径) で -3（中心）〜 1（画面の外）を一様に回る
        let t = fract(h + tr * (0.8 + 0.5 * h2));
        let lr1 = mix(-3.2, 1.1, t);
        let len = 0.03 + 0.55 * sp * (0.5 + h2);
        let lr0 = lr1 - len;
        let da = abs(fract(a) - 0.5) / bins * TAU * r;
        let w = (0.0012 + 0.0016 * h2) * (0.6 + 0.8 * t);
        let on = step(lr0, lr) * step(lr, lr1) * step(0.3, h);
        let lin = sat((lr - lr0) / max(len, 1e-4));
        let b = on * (1.0 - smoothstep(w * 0.5, w * 1.6, da)) * (0.2 + 0.8 * lin * lin) * smoothstep(0.0, 0.35, t);
        let tint = mix(uInk, select(uStar, uA, h2 < 0.5), 0.3 + 0.2 * fl);
        col += tint * b * (1.0 + sp * 0.6);
    }
    return col;
}

fn matte(uv: vec2<f32>, p: f32) -> f32 { return step(0.5, p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let q = (P - vec2<f32>(960.0, 540.0)) / 540.0;
    let r = length(q);
    let env = win(p, 0.0, 0.1) * (1.0 - win(p, 0.82, 1.0));
    let sp = speed(p);
    // 周りを暗くするトンネル
    var c = paint(uShade * 0.5, sat(0.25 + 0.35 * r) * sat(sp / 1.6) * env);
    c = light(c, stars(p) * env);
    // 閃光。中心は白、外は差し色。0.5 の前後で画面を覆い切る
    var f = bump(p, 0.36, 0.5, 0.66);
    f = f * f * (3.0 - 2.0 * f);
    let a = sat(f * 1.3 - 0.3 * r * (1.0 - f));
    let fc = mix(uInk, mix(uInk, uA, 0.6), sat(r * 0.7));
    c = over(c, paint(fc, a));
    return c;
}

fn motion(p: f32) -> Motion2 {
    var m = still();
    if (p < 0.5) {
        let k = in3(p / 0.5);
        m.a.sx = 1.0 + 0.65 * k;
        m.a.sy = 1.0 + 0.65 * k;
        m.a.blur = 26.0 * k;
        m.b.op = 0.0;
    } else {
        let k = 1.0 - outExpo((p - 0.5) / 0.5);
        m.b.sx = 1.0 + 0.5 * k;
        m.b.sy = 1.0 + 0.5 * k;
        m.b.blur = 26.0 * k;
    }
    return m;
}
