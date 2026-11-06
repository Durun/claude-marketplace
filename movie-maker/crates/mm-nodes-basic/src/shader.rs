//! `shader.wgsl`: 利用者が書いた WGSL のフラグメントシェーダーで 1 枚を描く種類。効果とトランジションもこれで書ける。
//!
//! 利用者のコードは `fn mm_main(uv: vec2<f32>) -> vec4<f32>` を定義し、線形の premultiplied の色を返す。使える名前:
//! - `u.size`（出力の画素数）、`u.time`（秒）、`u.scale`（画面上の倍率）、`u.p0`（`p0`..`p3`）、`u.p1`（`p4`..`p7`）
//! - `mm_input(uv)`: 入力 0 を双線形で引く。入力が無ければ透明。
//!
//! コンパイルに失敗したら `render` は `Err(InvalidArgument)` を返し、レンダラーがそのレイヤーだけを透明にして
//! エラーを記録する。失敗は WGSL の全文ごと覚えるので、毎フレームコンパイルし直さない。

use mm_gpu::WgpuGpu;
use mm_node_api::{Cost, FrameCx, FrameRef, GpuTexture, InputSchema, InputSpec, NodeKind, Traits};
use mm_types::{MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result, Size};

use crate::gpu_pass::{input_texture, Pass, PassCache};
use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// 数のパラメータの数。`p0`..`p7`。
const NUMBER_PARAMS: usize = 8;

/// コードの既定。UV をそのまま色にする。
const DEFAULT_CODE: &str =
    "fn mm_main(uv: vec2<f32>) -> vec4<f32> {\n    return vec4<f32>(uv, 0.5, 1.0);\n}\n";

const PRELUDE: &str = r"
struct U {
    size: vec2<f32>,
    time: f32,
    scale: f32,
    p0: vec4<f32>,
    p1: vec4<f32>,
}

@fragment
fn mm_fs(in: MmVsOut) -> @location(0) vec4<f32> {
    return mm_main(in.uv);
}
";

const INPUT_WGSL: &str = r"
fn mm_input(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(mm_t0, mm_samp, uv, 0.0);
}
";

const NO_INPUT_WGSL: &str = r"
fn mm_input(uv: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(0.0);
}
";

/// 利用者の WGSL を描く種類。
#[derive(Debug, Default)]
pub struct ShaderWgslKind {
    cache: PassCache,
}

/// uniform（`U`）の中身。`size`、`time`、`scale`、`p0`..`p7` の順で 12 個、16 個に詰める。
fn uniform_bytes(size: Size, time: f32, scale: f32, numbers: &[f32; NUMBER_PARAMS]) -> Vec<u8> {
    let mut v = [0.0_f32; 16];
    v[..4].copy_from_slice(&[size.width as f32, size.height as f32, time, scale]);
    v[4..4 + NUMBER_PARAMS].copy_from_slice(numbers);
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn code_of(params: &Params) -> &str {
    params
        .get("code")
        .and_then(ParamValue::as_text)
        .unwrap_or(DEFAULT_CODE)
}

impl NodeKind for ShaderWgslKind {
    fn id(&self) -> &'static str {
        "shader.wgsl"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        let mut params = vec![ParamSpec::new(
            "code",
            ParamType::Text,
            ParamValue::Text(DEFAULT_CODE.into()),
        )];
        for i in 0..NUMBER_PARAMS {
            params.push(ParamSpec::new(
                format!("p{i}"),
                ParamType::Number {
                    min: None,
                    max: None,
                },
                ParamValue::Number(0.0),
            ));
        }
        ParamSchema::new(params)
    }

    fn inputs(&self) -> InputSchema {
        InputSchema {
            inputs: vec![InputSpec {
                name: "source".into(),
                optional: true,
            }],
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
        let code = code_of(cx.params).to_string();
        let numbers: [f32; NUMBER_PARAMS] =
            std::array::from_fn(|i| cx.params.number_or(&format!("p{i}"), 0.0) as f32);
        let (time, scale, canvas) = (cx.time.to_f64() as f32, cx.scale, cx.size);
        let has_input = !cx.inputs.is_empty();
        let t = cx.time;
        let input = if has_input {
            Some(cx.inputs.frame(0, t)?)
        } else {
            None
        };
        let Some(gpu) = cx.gpu.as_any().downcast_mut::<WgpuGpu>() else {
            return Err(MmError::Unsupported(
                "shader.wgsl は wgpu の GPU が要る".into(),
            ));
        };
        let input = input.map(|frame| input_texture(gpu, frame)).transpose()?;
        let size = input.as_ref().map_or(canvas, |i| i.size);
        let ids: Vec<u64> = input.iter().map(|i| i.id).collect();
        let glue = if has_input { INPUT_WGSL } else { NO_INPUT_WGSL };
        let body = format!("{PRELUDE}\n{glue}\n{code}");
        let result = self.cache.run(
            gpu,
            &Pass {
                body: &body,
                uniform: &uniform_bytes(size, time, scale, &numbers),
                inputs: &ids,
                lut: None,
                size,
            },
        );
        if let Some(made) = input.filter(|i| i.made) {
            gpu.release_id(made.id);
        }
        Ok(FrameRef::Gpu(GpuTexture { id: result?, size }))
    }
}
