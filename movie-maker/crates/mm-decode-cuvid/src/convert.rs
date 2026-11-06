//! 共有バッファの NV12 / P010 を線形の `Rgba16Float` に変えるパス。10 bit は符号語のまま浮動小数にする。

use mm_decode_hw::render::{
    fragment_entry, storage_binding, uniform_binding, ParamsBuffer, YuvPass,
};
use mm_decode_hw::yuv::YuvParams;

use crate::share::{BufferLayout, SharedRing};

/// バッファの配置を渡す一様バッファのバイト数（`u32` 8 個分。WGSL の `Layout` の大きさ以上）。
const LAYOUT_BYTES: usize = 32;

const FRAGMENT: &str = r"
struct Layout { pitch: u32, uv_offset: u32, bytes_per_sample: u32, divisor: f32, uv_pitch: u32 }
@group(0) @binding(0) var<storage, read> yuv: array<u32>;
@group(0) @binding(1) var<uniform> params: Params;
@group(0) @binding(2) var<uniform> geom: Layout;

fn code_at(byte: u32) -> f32 {
    let word = yuv[byte >> 2u];
    if (geom.bytes_per_sample == 1u) {
        return f32((word >> ((byte & 3u) * 8u)) & 0xffu);
    }
    return f32((word >> ((byte & 2u) * 8u)) & 0xffffu) / geom.divisor;
}

fn chroma_texel(cx: u32, cy: u32) -> vec2<f32> {
    let o = geom.uv_offset + cy * geom.uv_pitch + cx * 2u * geom.bytes_per_sample;
    return vec2<f32>(code_at(o), code_at(o + geom.bytes_per_sample));
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let x = u32(pos.x);
    let y = u32(pos.y);
    let luma = code_at(y * geom.pitch + x * geom.bytes_per_sample);
    let t = chroma_taps(x, y);
    let uv = chroma_mix(
        chroma_texel(t.x0, t.y0),
        chroma_texel(t.x1, t.y0),
        chroma_texel(t.x0, t.y1),
        chroma_texel(t.x1, t.y1),
        t,
    );
    return yuv_to_linear(luma, uv.x, uv.y);
}
";

/// 共有バッファ → `Rgba16Float` の描画パス。
pub struct BufferConverter {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pass: YuvPass,
    binds: Vec<wgpu::BindGroup>,
    params: ParamsBuffer,
}

/// 配置を一様バッファの中身にする。
fn layout_bytes(layout: BufferLayout, bits: u32) -> [u8; LAYOUT_BYTES] {
    let bytes_per_sample = if bits == 8 { 1u32 } else { 2 };
    let divisor = if bits == 8 {
        1.0
    } else {
        (1u32 << (16 - bits)) as f32
    };
    let mut out = [0u8; LAYOUT_BYTES];
    out[0..4].copy_from_slice(&layout.pitch.to_le_bytes());
    out[4..8].copy_from_slice(&layout.uv_offset.to_le_bytes());
    out[8..12].copy_from_slice(&bytes_per_sample.to_le_bytes());
    out[12..16].copy_from_slice(&divisor.to_le_bytes());
    out[16..20].copy_from_slice(&layout.uv_pitch.to_le_bytes());
    out
}

impl BufferConverter {
    /// リングバッファのスロットごとに束縛を作る。`bits` は 1 成分のビット深度。
    pub fn new(queue: wgpu::Queue, ring: &SharedRing, bits: u32) -> Self {
        let device = ring.device().clone();
        let entries = [
            fragment_entry(0, storage_binding()),
            fragment_entry(1, uniform_binding()),
            fragment_entry(2, uniform_binding()),
        ];
        let pass = YuvPass::new(
            &device,
            "yuv420 buffer to linear rgba16f",
            FRAGMENT,
            &entries,
        );
        let params = ParamsBuffer::new(&device);
        let layout_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("yuv layout"),
            size: LAYOUT_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&layout_buf, 0, &layout_bytes(ring.layout(), bits));
        let binds = (0..ring.len())
            .map(|i| {
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pass.layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: ring.slot(i).buffer.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: params.buffer().as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: layout_buf.as_entire_binding(),
                        },
                    ],
                })
            })
            .collect();
        Self {
            device,
            queue,
            pass,
            binds,
            params,
        }
    }

    /// スロットの YUV を読んで `target` に描き、キューに出す。
    pub fn submit(&mut self, slot: usize, params: YuvParams, target: &wgpu::Texture) {
        self.params.update(&self.queue, params);
        self.pass
            .draw(&self.device, &self.queue, &self.binds[slot], target);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 配置のフィールドが一様バッファの並びに入る() {
        let layout = BufferLayout::new(7, 5, 1);
        let bytes = layout_bytes(layout, 8);
        let word = |i: usize| u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap());
        assert_eq!(
            (word(0), word(1), word(2), word(4)),
            (7, 35, 1, 8),
            "pitch, uv_offset, bytes_per_sample, uv_pitch"
        );
    }
}
