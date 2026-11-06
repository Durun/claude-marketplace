//! `transition.dissolve`: クロスディゾルブ。カットの境のトランジションの種類。
//!
//! 入力 0（出ていくカット）と入力 1（入ってくるカット）を、線形の光の上で `progress` の比率で混ぜる。
//! `progress` はレンダラーが境の中の位置（0..1）に直して渡す。

use mm_gpu::WgpuGpu;
use mm_node_api::{
    FrameCx, FrameRef, GpuTexture, InputSchema, InputSpec, NodeKind, Traits,
};
use mm_types::{
    MmError, ParamSchema, ParamSpec, ParamType, ParamValue, Params, Result, Rgba, RgbaImage,
};

use crate::gpu_pass::{input_texture, Pass, PassCache};
use crate::util::migrate_same_version;
use crate::KIND_VERSION;

/// 進み具合のパラメータの名前。レンダラーが書く。
pub const TRANSITION_PROGRESS_PARAM: &str = "progress";

const DISSOLVE_WGSL: &str = r"
struct U { a: vec4<f32> }

@fragment
fn mm_fs(in: MmVsOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(in.pos.xy);
    let from = textureLoad(mm_t0, p, 0);
    let to = textureLoad(mm_t1, p, 0);
    return mix(from, to, u.a.x);
}
";

/// クロスディゾルブ。
#[derive(Debug, Default)]
pub struct DissolveKind {
    cache: PassCache,
}

fn mix_images(from: &RgbaImage, to: &RgbaImage, k: f32) -> Result<RgbaImage> {
    if from.size() != to.size() {
        return Err(MmError::InvalidArgument(
            "トランジションの 2 つの入力の大きさが違う".into(),
        ));
    }
    let mut out = RgbaImage::transparent(from.size());
    for y in 0..from.height() {
        for x in 0..from.width() {
            let (a, b) = (from.pixel(x, y), to.pixel(x, y));
            let m = |p: f32, q: f32| p + (q - p) * k;
            out.set_pixel(x, y, Rgba::new(m(a.r, b.r), m(a.g, b.g), m(a.b, b.b), m(a.a, b.a)));
        }
    }
    Ok(out)
}

impl DissolveKind {
    fn mix_gpu(
        &self,
        gpu: &mut WgpuGpu,
        from: FrameRef,
        to: FrameRef,
        progress: f32,
    ) -> Result<FrameRef> {
        let a = input_texture(gpu, from)?;
        let b = match input_texture(gpu, to) {
            Ok(b) => b,
            Err(e) => {
                if a.made {
                    gpu.release_id(a.id);
                }
                return Err(e);
            }
        };
        let result = if a.size == b.size {
            let mut uniform = [0.0_f32; 4];
            uniform[0] = progress;
            let bytes: Vec<u8> = uniform.iter().flat_map(|v| v.to_le_bytes()).collect();
            self.cache.run(
                gpu,
                &Pass {
                    body: DISSOLVE_WGSL,
                    uniform: &bytes,
                    inputs: &[a.id, b.id],
                    lut: None,
                    size: a.size,
                },
            )
        } else {
            Err(MmError::InvalidArgument(
                "トランジションの 2 つの入力の大きさが違う".into(),
            ))
        };
        for tex in [&a, &b].into_iter().filter(|t| t.made) {
            gpu.release_id(tex.id);
        }
        Ok(FrameRef::Gpu(GpuTexture {
            id: result?,
            size: a.size,
        }))
    }
}

impl NodeKind for DissolveKind {
    fn id(&self) -> &'static str {
        "transition.dissolve"
    }

    fn version(&self) -> u32 {
        KIND_VERSION
    }

    fn params(&self) -> ParamSchema {
        ParamSchema::new(vec![ParamSpec::new(
            TRANSITION_PROGRESS_PARAM,
            ParamType::Number {
                min: Some(0.0),
                max: Some(1.0),
            },
            ParamValue::Number(0.5),
        )])
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
        Traits::PIXEL
    }

    fn migrate(&self, old: serde_json::Value, from: u32) -> Result<Params> {
        migrate_same_version(self, old, from)
    }

    fn is_identity(&self, cx: &FrameCx<'_>) -> bool {
        cx.params.number_or(TRANSITION_PROGRESS_PARAM, 0.5) <= 0.0
    }

    fn render(&self, cx: &mut FrameCx<'_>) -> Result<FrameRef> {
        let progress = cx
            .params
            .number_or(TRANSITION_PROGRESS_PARAM, 0.5)
            .clamp(0.0, 1.0) as f32;
        let t = cx.time;
        let from = cx.inputs.frame(0, t)?;
        let to = cx.inputs.frame(1, t)?;
        if progress <= 0.0 {
            return Ok(from);
        }
        if progress >= 1.0 {
            return Ok(to);
        }
        if let Some(gpu) = cx.gpu.as_any().downcast_mut::<WgpuGpu>() {
            return self.mix_gpu(gpu, from, to, progress);
        }
        let (a, b) = (cx.gpu.download(&from)?, cx.gpu.download(&to)?);
        Ok(FrameRef::cpu(mix_images(&a, &b, progress)?))
    }
}
