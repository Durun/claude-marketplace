// 波形: 一本の線から音声の波形がふくらみ、波形の中から次の場面。録音、ボーカル、音の聴き比べの話題に（元: fx/waveform.js、3 拍）。
// 中央の細い線から DAW の波形表示（ピークと RMS）がふくらんで画面を満たし、波形の中に B。
const SHUTTER: f32 = 0.4;

fn fmodf(x: f32, y: f32) -> f32 { return x - y * floor(x / y); }
fn amp(p: f32) -> f32 { return mix(2.0, 2600.0, in3(win(p, 0.12, 0.72))); }
fn peak(x: f32, t: f32) -> f32 {
    let xs = x + t * 900.0; // 波形は左へ流れる
    let beat = 1920.0 / 8.0;
    let trans = exp(-fmodf(xs, beat) / 70.0); // 拍ごとのアタック
    let body = 0.25 + 0.55 * fbm(vec2<f32>(xs * 0.006, uSeed));
    let fine = 0.75 + 0.25 * vnoise(vec2<f32>(xs * 0.08, 3.0));
    return sat((0.18 + 0.5 * trans + body * 0.6) * fine);
}
fn colX() -> f32 { return floor(P.x / 5.0) * 5.0 + 2.5; }
fn drawn(p: f32) -> f32 { return 1920.0 * out3(win(p, 0.0, 0.14)); }

struct Wave { d: f32, pk: f32, a: f32 }
fn wave(p: f32) -> Wave {
    let pk = peak(colX(), p * uDur);
    let a = amp(p);
    let gap = mix(1.0, -1.0, win(p, 0.5, 0.6));
    let stripe = abs(P.x - colX()) - 1.5 - gap;
    let d = max(abs(P.y - 540.0) - a * pk, select(-1e3, stripe, p < 0.6));
    return Wave(select(d, 1e3, P.x > drawn(p) + 1.0), pk, a);
}

fn matte(uv: vec2<f32>, p: f32) -> f32 {
    if (p > 0.74) { return 1.0; }
    return fill(wave(p).d);
}

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> {
    let env = win(p, 0.0, 0.06) * (1.0 - win(p, 0.6, 0.76));
    if (env <= 0.0) { return vec4<f32>(0.0); }
    let w = wave(p);
    let inW = fill(w.d);
    let dy = abs(P.y - 540.0);
    var c = paint(uShade, 0.5 * env * (1.0 - inW));
    let rms = fill(dy - w.a * w.pk * 0.5);
    let fadeTint = 1.0 - win(p, 0.4, 0.62);
    c = over(c, paint(mix(uA, uInk, 0.25 * rms), (0.16 + 0.2 * rms) * inW * env * fadeTint));
    // 波形の縁（ピーク）
    let e = abs(dy - w.a * w.pk);
    let on = select(0.0, 1.0, P.x <= drawn(p));
    c = light(c, (mix(uA, uInk, 0.5) * fill(e - 1.2) * 0.9 + uA * glow(e, 10.0) * 0.35) * env * on * step(1.0, w.a * w.pk));
    // 中央の線（最初に左から引かれる）
    let lineOn = on * (1.0 - win(p, 0.3, 0.5));
    c = light(c, (uInk * fill(dy - 1.0) + uA * glow(dy, 6.0) * 0.6) * lineOn * env);
    let draw = win(p, 0.0, 0.14);
    c = light(c, uInk * glow(length(P - vec2<f32>(drawn(p), 540.0)), 12.0) * (1.0 - draw) * env);
    return c;
}

fn motion(p: f32) -> Motion2 { return still(); }
