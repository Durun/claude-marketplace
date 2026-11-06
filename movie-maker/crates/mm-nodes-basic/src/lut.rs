//! 3D LUT。`.cube` の読み込み、CPU の三線形補間、GPU のテクスチャ。
//!
//! `mm-color-lut` と同じ形式だが、実装のクレート同士は依存しないので、ノードの種類に要る分だけをここに持つ。
//! 補間の式は `mm-color-lut` の `sample_trilinear` と試験で突き合わせる。

use std::sync::{Arc, Mutex, Weak};

use mm_gpu::GpuContext;
use mm_types::{MmError, Result};

/// 一辺の上限。メモリの暴走を防ぐ。
const MAX_SIZE: u32 = 129;

/// GPU で 3D LUT を引く WGSL。`mm_lut_sample` の入力は 0..=1 に切る。
pub(crate) const LUT_WGSL: &str = r"
fn mm_lut_at(i: vec3<i32>) -> vec3<f32> {
    return textureLoad(mm_lut, i, 0).rgb;
}

fn mm_lut_sample(c: vec3<f32>) -> vec3<f32> {
    let n = i32(textureDimensions(mm_lut).x);
    let p = clamp(c, vec3<f32>(0.0), vec3<f32>(1.0)) * f32(n - 1);
    let fl = floor(p);
    let f = p - fl;
    let i0 = vec3<i32>(fl);
    let i1 = min(i0 + vec3<i32>(1), vec3<i32>(n - 1));
    let x00 = mix(mm_lut_at(vec3<i32>(i0.x, i0.y, i0.z)), mm_lut_at(vec3<i32>(i1.x, i0.y, i0.z)), f.x);
    let x10 = mix(mm_lut_at(vec3<i32>(i0.x, i1.y, i0.z)), mm_lut_at(vec3<i32>(i1.x, i1.y, i0.z)), f.x);
    let x01 = mix(mm_lut_at(vec3<i32>(i0.x, i0.y, i1.z)), mm_lut_at(vec3<i32>(i1.x, i0.y, i1.z)), f.x);
    let x11 = mix(mm_lut_at(vec3<i32>(i0.x, i1.y, i1.z)), mm_lut_at(vec3<i32>(i1.x, i1.y, i1.z)), f.x);
    return mix(mix(x00, x10, f.y), mix(x01, x11, f.y), f.z);
}
";

/// GPU に上げたテクスチャと、上げた時のコンテキスト。
struct Uploaded {
    ctx: Weak<GpuContext>,
    texture: Arc<wgpu::Texture>,
}

/// 3D LUT。`data` は `size^3 * 3` の f32 で、R が最も速く変わる。
#[derive(Debug)]
pub(crate) struct Lut {
    pub size: u32,
    pub data: Vec<f32>,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
    uploaded: Mutex<Option<Uploaded>>,
}

impl std::fmt::Debug for Uploaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Uploaded")
    }
}

impl Lut {
    /// 値の列から。長さが `size^3 * 3` でなければ `Corrupt`。
    pub(crate) fn new(
        size: u32,
        data: Vec<f32>,
        domain_min: [f32; 3],
        domain_max: [f32; 3],
    ) -> Result<Self> {
        if !(2..=MAX_SIZE).contains(&size) {
            return Err(MmError::Corrupt(format!("LUT の一辺 {size} は使えない")));
        }
        let expected = (size as usize).pow(3) * 3;
        if data.len() != expected {
            return Err(MmError::Corrupt(format!(
                "LUT の値が {} 個、期待 {expected}",
                data.len()
            )));
        }
        Ok(Self {
            size,
            data,
            domain_min,
            domain_max,
            uploaded: Mutex::new(None),
        })
    }

    /// `.cube`（3D）のテキストを読む。1D LUT は未対応。
    pub(crate) fn parse_cube(text: &str) -> Result<Self> {
        let mut size = None;
        let mut domain_min = [0.0; 3];
        let mut domain_max = [1.0; 3];
        let mut data = Vec::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            let mut words = line.split_whitespace();
            match words.next() {
                None => {}
                Some("LUT_3D_SIZE") => size = Some(parse_number::<u32>(words.next())?),
                Some("LUT_1D_SIZE") => return Err(MmError::Unsupported("1D LUT".into())),
                Some("DOMAIN_MIN") => domain_min = parse_triple(words)?,
                Some("DOMAIN_MAX") => domain_max = parse_triple(words)?,
                Some(first) if first.starts_with(|c: char| c.is_ascii_alphabetic()) => {}
                Some(first) => data.extend(parse_triple(std::iter::once(first).chain(words))?),
            }
        }
        let size = size.ok_or_else(|| MmError::Corrupt("LUT_3D_SIZE が無い".into()))?;
        Self::new(size, data, domain_min, domain_max)
    }

    /// 入力 `rgb`（定義域の値）を引く。定義域の外は端に切る。
    pub(crate) fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        let n = self.size as usize;
        let p: [f32; 3] = std::array::from_fn(|i| {
            let span = (self.domain_max[i] - self.domain_min[i]).max(f32::EPSILON);
            ((rgb[i] - self.domain_min[i]) / span).clamp(0.0, 1.0) * (n - 1) as f32
        });
        let i0: [usize; 3] = p.map(|v| v.floor() as usize);
        let f: [f32; 3] = std::array::from_fn(|i| p[i] - i0[i] as f32);
        let at = |x: usize, y: usize, z: usize| -> [f32; 3] {
            let base = ((z * n + y) * n + x) * 3;
            [self.data[base], self.data[base + 1], self.data[base + 2]]
        };
        let i1: [usize; 3] = std::array::from_fn(|i| (i0[i] + 1).min(n - 1));
        let lerp = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
            std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
        };
        let x00 = lerp(at(i0[0], i0[1], i0[2]), at(i1[0], i0[1], i0[2]), f[0]);
        let x10 = lerp(at(i0[0], i1[1], i0[2]), at(i1[0], i1[1], i0[2]), f[0]);
        let x01 = lerp(at(i0[0], i0[1], i1[2]), at(i1[0], i0[1], i1[2]), f[0]);
        let x11 = lerp(at(i0[0], i1[1], i1[2]), at(i1[0], i1[1], i1[2]), f[0]);
        lerp(lerp(x00, x10, f[1]), lerp(x01, x11, f[1]), f[2])
    }

    /// GPU のテクスチャ。コンテキストごとに 1 回だけ上げて使い回す。
    pub(crate) fn texture(&self, ctx: &Arc<GpuContext>) -> Result<Arc<wgpu::Texture>> {
        let mut guard = self
            .uploaded
            .lock()
            .map_err(|_| MmError::Internal("LUT のテクスチャの取り置きが壊れた".into()))?;
        let current = Arc::downgrade(ctx);
        if let Some(up) = guard.as_ref().filter(|u| Weak::ptr_eq(&u.ctx, &current)) {
            return Ok(up.texture.clone());
        }
        let texture = Arc::new(self.upload(ctx));
        *guard = Some(Uploaded {
            ctx: current,
            texture: texture.clone(),
        });
        Ok(texture)
    }

    /// RGB を RGBA の f32 にして 3D テクスチャへ書く（textureLoad で引くので補間の対応は要らない）。
    fn upload(&self, ctx: &GpuContext) -> wgpu::Texture {
        let n = self.size;
        let texture = ctx.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("mm-nodes-basic lut"),
            size: wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: n,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D3,
            format: wgpu::TextureFormat::Rgba32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let mut rgba = Vec::with_capacity(self.data.len() / 3 * 4);
        for px in self.data.chunks_exact(3) {
            rgba.extend_from_slice(px);
            rgba.push(1.0);
        }
        let bytes: Vec<u8> = rgba.iter().flat_map(|v| v.to_le_bytes()).collect();
        ctx.queue.write_texture(
            texture.as_image_copy(),
            &bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(n * 16),
                rows_per_image: Some(n),
            },
            wgpu::Extent3d {
                width: n,
                height: n,
                depth_or_array_layers: n,
            },
        );
        texture
    }
}

fn parse_number<T: std::str::FromStr>(word: Option<&str>) -> Result<T> {
    word.and_then(|w| w.parse().ok())
        .ok_or_else(|| MmError::Corrupt("LUT の数が読めない".into()))
}

fn parse_triple<'a>(mut words: impl Iterator<Item = &'a str>) -> Result<[f32; 3]> {
    Ok([
        parse_number(words.next())?,
        parse_number(words.next())?,
        parse_number(words.next())?,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CUBE: &str = "# 色を反転する 2 点の LUT\nTITLE \"invert\"\nLUT_3D_SIZE 2\n\
        1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n";

    #[test]
    fn cube_を読んで補間する() {
        let lut = Lut::parse_cube(CUBE).unwrap();
        assert_eq!(lut.size, 2);
        assert_eq!(lut.sample([0.0, 0.0, 0.0]), [1.0, 1.0, 1.0]);
        let mid = lut.sample([0.5, 0.5, 0.5]);
        assert!(mid.iter().all(|v| (v - 0.5).abs() < 1e-6), "{mid:?}");
    }

    #[test]
    fn 行数が合わない_cube_は拒否する() {
        let err = Lut::parse_cube("LUT_3D_SIZE 2\n0 0 0\n").unwrap_err();
        assert!(matches!(err, MmError::Corrupt(_)), "{err:?}");
    }

    #[test]
    fn 一次元の_lut_は未対応() {
        let err = Lut::parse_cube("LUT_1D_SIZE 4\n").unwrap_err();
        assert!(matches!(err, MmError::Unsupported(_)), "{err:?}");
    }

    #[test]
    fn 三線形補間は_mm_color_lut_と一致する() {
        let ours = Lut::parse_cube(CUBE).unwrap();
        let theirs = mm_color_lut::Cube3d::parse(CUBE).unwrap();
        for rgb in [[0.1, 0.7, 0.3], [0.9, 0.2, 0.5], [0.0, 1.0, 0.25]] {
            let a = ours.sample(rgb);
            let b = mm_color_lut::sample_trilinear(theirs.size, &theirs.data, rgb);
            for (x, y) in a.iter().zip(b) {
                assert!((x - y).abs() < 1e-6, "{rgb:?}: {a:?} vs {b:?}");
            }
        }
    }
}
