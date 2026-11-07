// ディップ: 画面をいったん濃紺に沈めてから次の場面を出す。話題の区切りや、日をまたぐとき（元: fx/dip.js、2 拍）。
const SHUTTER: f32 = 0.0;

fn matte(uv: vec2<f32>, p: f32) -> f32 { return step(0.5, p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let a = select(1.0 - io3(win(p, 0.55, 1.0)), io3(win(p, 0.0, 0.45)), p < 0.5);
    let v = 1.0 - 0.35 * length(uv - 0.5);
    return paint(uShade * v, a);
}

fn motion(p: f32) -> Motion2 { return still(); }
