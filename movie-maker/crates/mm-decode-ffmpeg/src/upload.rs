//! CPU でデコードした YUV 4:2:0 のプレーンを、そのままレンダラーのデバイスのテクスチャに上げ、
//! シェーダーで線形の `Rgba16Float` にする（feature `gpu`）。swscale と CPU の f32 への変換を通らないので、
//! 4K でも 1 フレームの CPU の仕事はプレーンのコピーだけになる。

use ff::format::Pixel;
use ff::frame;
use ffmpeg_next as ff;
use mm_decode_api::{DecodedFrame, GpuFrameHandle};
use mm_decode_ffmpeg_core::matrix::{is_full_range, Matrix};
use mm_decode_hw::pool::FramePool;
use mm_decode_hw::render::{
    fragment_entry, texture_binding, uniform_binding, ParamsBuffer, YuvPass,
};
use mm_decode_hw::yuv::{chroma_size, YuvParams};
use mm_gpu::GpuContext;
use mm_types::{RationalTime, Result, Size};

/// Y、U、V の 3 プレーンを読むフラグメント。色差は 2 つの 1 成分のテクスチャから補間する。
const FRAGMENT: &str = r"
@group(0) @binding(0) var tex_y: texture_2d<f32>;
@group(0) @binding(1) var tex_u: texture_2d<f32>;
@group(0) @binding(2) var tex_v: texture_2d<f32>;
@group(0) @binding(3) var<uniform> params: Params;

fn chroma_at(x: u32, y: u32) -> vec2<f32> {
    let t = chroma_taps(x, y);
    let c00 = vec2<f32>(textureLoad(tex_u, vec2<u32>(t.x0, t.y0), 0).r, textureLoad(tex_v, vec2<u32>(t.x0, t.y0), 0).r);
    let c10 = vec2<f32>(textureLoad(tex_u, vec2<u32>(t.x1, t.y0), 0).r, textureLoad(tex_v, vec2<u32>(t.x1, t.y0), 0).r);
    let c01 = vec2<f32>(textureLoad(tex_u, vec2<u32>(t.x0, t.y1), 0).r, textureLoad(tex_v, vec2<u32>(t.x0, t.y1), 0).r);
    let c11 = vec2<f32>(textureLoad(tex_u, vec2<u32>(t.x1, t.y1), 0).r, textureLoad(tex_v, vec2<u32>(t.x1, t.y1), 0).r);
    return chroma_mix(c00, c10, c01, c11, t) * params.code_scale;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let xy = vec2<u32>(pos.xy);
    let y = textureLoad(tex_y, xy, 0).r * params.code_scale;
    let uv = chroma_at(xy.x, xy.y);
    return yuv_to_linear(y, uv.x, uv.y);
}
";

/// プレーンのテクスチャの組。素材の形式と大きさが変わったら作り直す。
struct Planes {
    format: wgpu::TextureFormat,
    size: Size,
    textures: [wgpu::Texture; 3],
    bind: wgpu::BindGroup,
}

/// YUV のプレーンを上げて変換する。
pub struct GpuUploader {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pass: YuvPass,
    params: ParamsBuffer,
    planes: Option<Planes>,
    pool: FramePool,
    next_id: u64,
}

impl GpuUploader {
    /// レンダラーのデバイスの上に、`size` の素材のための変換を作る。
    pub fn new(ctx: &GpuContext, size: Size) -> Self {
        let entries = [
            fragment_entry(0, texture_binding()),
            fragment_entry(1, texture_binding()),
            fragment_entry(2, texture_binding()),
            fragment_entry(3, uniform_binding()),
        ];
        Self {
            pass: YuvPass::new(
                &ctx.device,
                "yuv420 planes to linear rgba16f",
                FRAGMENT,
                &entries,
            ),
            params: ParamsBuffer::new(&ctx.device),
            planes: None,
            pool: FramePool::new(ctx.device.clone(), size),
            next_id: 0,
            device: ctx.device.clone(),
            queue: ctx.queue.clone(),
        }
    }

    /// プレーンの形式が扱えるか。8 bit は常に、10 bit はデバイスが 16 bit の正規化テクスチャを持つとき。
    fn plane_format(&self, pixel: Pixel) -> Option<(wgpu::TextureFormat, u32)> {
        match pixel {
            Pixel::YUV420P | Pixel::YUVJ420P => Some((wgpu::TextureFormat::R8Unorm, 8)),
            Pixel::YUV420P10LE
                if self
                    .device
                    .features()
                    .contains(wgpu::Features::TEXTURE_FORMAT_16BIT_NORM) =>
            {
                Some((wgpu::TextureFormat::R16Unorm, 10))
            }
            _ => None,
        }
    }

    /// 1 フレームを上げて GPU のフレームにする。扱えない形式なら `None`（CPU の経路に任せる）。
    pub fn upload(
        &mut self,
        index: i64,
        pts: RationalTime,
        src: &frame::Video,
    ) -> Result<Option<DecodedFrame>> {
        let Some((format, bits)) = self.plane_format(src.format()) else {
            return Ok(None);
        };
        let _span = tracing::debug_span!("decode.upload").entered();
        let size = Size::new(src.width(), src.height());
        if self
            .planes
            .as_ref()
            .is_none_or(|p| p.format != format || p.size != size)
        {
            self.planes = Some(self.make_planes(format, size));
        }
        let planes = self.planes.as_ref().expect("直前に作った");
        let (cw, ch) = chroma_size(size.width, size.height);
        let bytes = u32::from(bits > 8) + 1;
        let plane_sizes = [(size.width, size.height), (cw, ch), (cw, ch)];
        for (i, texture) in planes.textures.iter().enumerate() {
            let (w, h) = plane_sizes[i];
            let stride = src.stride(i);
            self.queue.write_texture(
                texture.as_image_copy(),
                &src.data(i)[..stride * (h as usize - 1) + (w * bytes) as usize],
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(h),
                },
                texture.size(),
            );
        }
        let params = YuvParams::new(
            Matrix::from_frame(src.color_space(), src.height()),
            is_full_range(src.color_range()),
            bits,
            YuvParams::texture_code_scale(bits),
            (size.width, size.height),
        );
        self.params.update(&self.queue, params);
        let target = self.pool.take();
        self.pass
            .draw(&self.device, &self.queue, &planes.bind, &target);
        let id = self.next_id;
        self.next_id += 1;
        let handle: GpuFrameHandle = self.pool.lend(target).into_handle(id);
        Ok(Some(DecodedFrame {
            index,
            pts,
            image: None,
            gpu: Some(handle),
        }))
    }

    fn make_planes(&self, format: wgpu::TextureFormat, size: Size) -> Planes {
        let (cw, ch) = chroma_size(size.width, size.height);
        let make = |w, h| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("yuv plane"),
                size: wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let textures = [make(size.width, size.height), make(cw, ch), make(cw, ch)];
        let views: Vec<wgpu::TextureView> = textures
            .iter()
            .map(|t| t.create_view(&wgpu::TextureViewDescriptor::default()))
            .collect();
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("yuv planes"),
            layout: &self.pass.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: self.params.buffer().as_entire_binding(),
                },
            ],
        });
        Planes {
            format,
            size,
            textures,
            bind,
        }
    }
}
