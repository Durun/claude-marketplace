//! `transition.wgsl`: 利用者が書いた WGSL で 2 つの入力（出ていくカットと入ってくるカット）を混ぜるトランジションの種類。
//!
//! 名前の約束は gl-transitions（`getFromColor`、`getToColor`、`progress`、`ratio`）に揃える。利用者のコードは
//! `fn mm_transition(uv: vec2<f32>) -> vec4<f32>` を定義し、線形の premultiplied の色を返す。`uv` は左上が (0, 0)
//! （gl-transitions は左下が原点なので、上下を返す移植では `1.0 - uv.y` に読み替える）。使える名前:
//! - `mm_from(uv)`、`mm_to(uv)`: 入力 0 と 1 を双線形で引く（範囲の外は端の色）。
//! - `u.size`（出力の画素数）、`u.progress`（0..1）、`u.ratio`（幅 / 高さ）、`u.time`（トランジションの中の秒）、
//!   `u.duration`（トランジションの長さの秒）、`u.scale`（画面上の倍率）、`u.p0`（`p0`..`p3`）、`u.p1`（`p4`..`p7`）。
//!
//! `progress` と `duration` はレンダラーが境の中の位置から書く。`rewind` が正なら、レンダラーは出ていくカットを
//! 掛かり始めから逆向きにその倍率で描く（[`TRANSITION_REWIND_PARAM`]）。

use mm_gpu::WgpuGpu;
use mm_node_api::{Cost, FrameCx, FrameRef, GpuTexture, InputSchema, InputSpec, NodeKind, Traits};
use mm_types::{MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result, Size};

use crate::gpu_pass::{input_texture, InputTexture, Pass, PassCache};
use crate::transition::TRANSITION_PROGRESS_PARAM;
use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// トランジションの長さ（秒）のパラメータの名前。レンダラーが書く。
pub const TRANSITION_DURATION_PARAM: &str = "duration";
/// 出ていくカットを逆向きに描く倍率のパラメータの名前。0 なら普通に描く。レンダラーが読む。
pub const TRANSITION_REWIND_PARAM: &str = "rewind";

/// 数のパラメータの数。`p0`..`p7`。
const NUMBER_PARAMS: usize = 8;
/// `duration` の既定（秒）。
const DEFAULT_DURATION: f64 = 1.0;

/// コードの既定。線形の光の上のクロスディゾルブ。
const DEFAULT_CODE: &str = "fn mm_transition(uv: vec2<f32>) -> vec4<f32> {\n    return mix(mm_from(uv), mm_to(uv), u.progress);\n}\n";

const PRELUDE: &str = r"
struct U {
    size: vec2<f32>,
    progress: f32,
    ratio: f32,
    time: f32,
    duration: f32,
    scale: f32,
    pad: f32,
    p0: vec4<f32>,
    p1: vec4<f32>,
}

fn mm_from(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(mm_t0, mm_samp, uv, 0.0);
}

fn mm_to(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(mm_t1, mm_samp, uv, 0.0);
}

@fragment
fn mm_fs(in: MmVsOut) -> @location(0) vec4<f32> {
    return mm_transition(in.uv);
}
";

/// 利用者の WGSL で 2 つの入力を混ぜる種類。
#[derive(Debug, Default)]
pub struct TransitionWgslKind {
    cache: PassCache,
}

/// uniform（`U`）に詰める値。
struct Uniform {
    size: Size,
    progress: f32,
    duration: f32,
    scale: f32,
    numbers: [f32; NUMBER_PARAMS],
}

impl Uniform {
    /// `size`、`progress`、`ratio`、`time`、`duration`、`scale`、詰め物、`p0`..`p7` の順で 16 個。
    fn bytes(&self) -> Vec<u8> {
        let (w, h) = (self.size.width as f32, self.size.height as f32);
        let ratio = if h > 0.0 { w / h } else { 1.0 };
        let mut v = [0.0_f32; 16];
        v[..8].copy_from_slice(&[
            w,
            h,
            self.progress,
            ratio,
            self.progress * self.duration,
            self.duration,
            self.scale,
            0.0,
        ]);
        v[8..].copy_from_slice(&self.numbers);
        v.iter().flat_map(|x| x.to_le_bytes()).collect()
    }
}

fn number_spec(name: &str, min: Option<f64>, max: Option<f64>, default: f64) -> ParamSpec {
    ParamSpec::new(
        name,
        ParamType::Number { min, max },
        ParamValue::Number(default),
    )
}

fn release(gpu: &mut WgpuGpu, inputs: &[&InputTexture]) {
    for tex in inputs.iter().filter(|t| t.made) {
        gpu.release_id(tex.id);
    }
}

impl TransitionWgslKind {
    fn mix_gpu(
        &self,
        gpu: &mut WgpuGpu,
        (from, to): (FrameRef, FrameRef),
        code: &str,
        uniform: &Uniform,
    ) -> Result<FrameRef> {
        let a = input_texture(gpu, from)?;
        let b = match input_texture(gpu, to) {
            Ok(b) => b,
            Err(e) => {
                release(gpu, &[&a]);
                return Err(e);
            }
        };
        if a.size != b.size {
            release(gpu, &[&a, &b]);
            return Err(MmError::InvalidArgument(
                "トランジションの 2 つの入力の大きさが違う".into(),
            ));
        }
        let body = format!("{PRELUDE}\n{code}");
        let result = self.cache.run(
            gpu,
            &Pass {
                body: &body,
                uniform: &Uniform {
                    size: a.size,
                    ..*uniform
                }
                .bytes(),
                inputs: &[a.id, b.id],
                lut: None,
                size: a.size,
            },
        );
        release(gpu, &[&a, &b]);
        Ok(FrameRef::Gpu(GpuTexture {
            id: result?,
            size: a.size,
        }))
    }
}

impl NodeKind for TransitionWgslKind {
    fn id(&self) -> &'static str {
        "transition.wgsl"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        let mut params = vec![
            ParamSpec::new(
                "code",
                ParamType::Text,
                ParamValue::Text(DEFAULT_CODE.into()),
            ),
            number_spec(TRANSITION_PROGRESS_PARAM, Some(0.0), Some(1.0), 0.5),
            number_spec(TRANSITION_DURATION_PARAM, Some(0.0), None, DEFAULT_DURATION),
            number_spec(TRANSITION_REWIND_PARAM, Some(0.0), None, 0.0),
        ];
        params.extend((0..NUMBER_PARAMS).map(|i| number_spec(&format!("p{i}"), None, None, 0.0)));
        ParamSchema::new(params)
    }

    fn inputs(&self) -> InputSchema {
        let input = |name: &str| InputSpec {
            name: name.into(),
            optional: false,
        };
        InputSchema {
            inputs: vec![input("from"), input("to")],
        }
    }

    fn traits(&self) -> Traits {
        Traits {
            cost: Cost::Medium,
            ..Traits::PIXEL
        }
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let code = cx
            .params
            .get("code")
            .and_then(ParamValue::as_text)
            .unwrap_or(DEFAULT_CODE)
            .to_string();
        let uniform = Uniform {
            size: cx.size,
            progress: cx
                .params
                .number_or(TRANSITION_PROGRESS_PARAM, 0.5)
                .clamp(0.0, 1.0) as f32,
            duration: cx
                .params
                .number_or(TRANSITION_DURATION_PARAM, DEFAULT_DURATION)
                .max(0.0) as f32,
            scale: cx.scale,
            numbers: std::array::from_fn(|i| cx.params.number_or(&format!("p{i}"), 0.0) as f32),
        };
        let t = cx.time;
        let from = cx.inputs.frame(0, t)?;
        let to = cx.inputs.frame(1, t)?;
        let Some(gpu) = cx.gpu.as_any().downcast_mut::<WgpuGpu>() else {
            return Err(MmError::Unsupported(
                "transition.wgsl は wgpu の GPU が要る".into(),
            ));
        };
        self.mix_gpu(gpu, (from, to), &code, &uniform)
    }
}
