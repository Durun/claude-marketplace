// フラッシュ: 一瞬白く飛ばして切り替える。オチ、試聴の頭、テンポよく畳みかけたいとき（元: fx/flash.js、1 拍）。
const SHUTTER: f32 = 0.5;

fn matte(uv: vec2<f32>, p: f32) -> f32 { return step(0.5, p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let f = select(1.0 - out2(win(p, 0.5, 1.0)), in3(win(p, 0.1, 0.5)), p < 0.5);
    let r = length((P - vec2<f32>(960.0, 540.0)) / 1080.0);
    let a = sat(f * 1.25 - r * 0.25 * (1.0 - f));
    let col = mix(uInk, mix(uInk, uA, 0.25), sat(r * 0.8));
    return paint(col, a);
}

// 白く飛ぶ前に前の場面が膨らみ、明けた後に次の場面が元の大きさへ戻る。
fn motion(p: f32) -> Motion2 {
    var m = still();
    if (p < 0.5) {
        let s = 1.0 + 0.06 * in3(p * 2.0);
        m.a.sx = s;
        m.a.sy = s;
    } else {
        let s = 1.06 - 0.06 * out3((p - 0.5) * 2.0);
        m.b.sx = s;
        m.b.sy = s;
    }
    return m;
}
