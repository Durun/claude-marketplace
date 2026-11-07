// ディゾルブ: ふつうのクロスディゾルブ。時間の経過や、同じ場所の続きをやわらかくつなぐ（元: fx/dissolve.js、2 拍）。
const SHUTTER: f32 = 0.5;

fn matte(uv: vec2<f32>, p: f32) -> f32 { return io3(p); }

fn overlay(uv: vec2<f32>, p: f32) -> vec4<f32> { return vec4<f32>(0.0); }

fn motion(p: f32) -> Motion2 { return still(); }
