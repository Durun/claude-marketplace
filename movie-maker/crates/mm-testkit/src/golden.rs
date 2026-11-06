//! 期待画像（ゴールデン）の比較と更新。
//!
//! 期待画像は `fixtures/golden/<バックエンド>/<名前>.png`（320x180、sRGB の 8 bit）。バックエンドが違うと浮動小数の丸めと
//! フィルタの実装が違うので、バックエンドごとに持つ。描いた画像（線形の premultiplied）は灰色の市松の上に重ね、
//! sRGB の 8 bit にして比べる。許容差は同じバックエンドの 2 段: 輝度の SSIM と、チャンネルごとの最大の差。
//!
//! 更新は環境変数 `MM_GOLDEN_UPDATE` を付けて試験を回す（`cargo xtask golden-update`）。更新した PNG は目で確かめてから
//! コミットする。

use std::path::{Path, PathBuf};

use mm_types::{RgbaImage, Size};

/// 期待画像の大きさ。
pub const GOLDEN_SIZE: Size = Size::new(320, 180);
/// 輝度の SSIM の下限（同じバックエンド）。
pub const MIN_SSIM: f64 = 0.995;
/// チャンネルごとの最大の差の上限（8 bit の段数）。
pub const MAX_CHANNEL_DIFF: u8 = 2;
/// 設定すると、比べずに期待画像を書き換える。
pub const UPDATE_ENV: &str = "MM_GOLDEN_UPDATE";
/// 設定すると、検出したバックエンドがこの名前でなければ失敗にする（違うバックエンドの画像で上書きしないため）。
pub const BACKEND_ENV: &str = "MM_GOLDEN_BACKEND";
/// 設定すると、期待画像のフォルダーが無いバックエンドでもスキップせず失敗にする。
pub const REQUIRE_ENV: &str = "MM_REQUIRE_GOLDEN";

/// SSIM のブロックの一辺。
const WINDOW: usize = 8;
/// 市松の一辺（画素）と、2 色の輝度（線形）。透明な所を見分けるために敷く。
const CHECKER_SIDE: u32 = 16;
const CHECKER_DARK: f32 = 0.05;
const CHECKER_LIGHT: f32 = 0.1;

/// バックエンドの API とアダプターの名前から、期待画像のフォルダー名を決める。
/// `warp`（D3D12 の CPU ラスタライザ）、`rtx`（NVIDIA。D3D12 でも Vulkan でも同じ。API の違いは許容差に収まる）、
/// `lavapipe`（Vulkan の CPU）。それ以外は名前から作る。
pub fn backend_name(api: &str, adapter: &str) -> String {
    let (api_l, adapter_l) = (api.to_lowercase(), adapter.to_lowercase());
    if api_l.contains("dx12") && (adapter_l.contains("basic render") || adapter_l.contains("warp"))
    {
        return "warp".into();
    }
    if adapter_l.contains("nvidia") {
        return "rtx".into();
    }
    if api_l.contains("vulkan") && adapter_l.contains("llvmpipe") {
        return "lavapipe".into();
    }
    let joined = format!("{api_l}-{adapter_l}");
    let mut name = String::new();
    for c in joined.chars() {
        let c = if c.is_ascii_alphanumeric() { c } else { '-' };
        if c != '-' || !name.ends_with('-') {
            name.push(c);
        }
    }
    name.trim_matches('-').to_string()
}

/// あるバックエンドの期待画像のフォルダー。
#[derive(Debug, Clone)]
pub struct Golden {
    backend: String,
    dir: PathBuf,
    actual_dir: PathBuf,
    update: bool,
}

impl Golden {
    /// `root/<backend>/` の期待画像を開く。フォルダーが無いバックエンドは `None`（呼ぶ側がスキップする）。
    /// 更新のとき、または `MM_REQUIRE_GOLDEN` があるときは、無ければ作る・失敗にする。
    /// `actual_dir` は、比べて合わなかった画像を書き出す場所。
    pub fn open(root: &Path, backend: &str, actual_dir: &Path) -> Option<Self> {
        if let Ok(want) = std::env::var(BACKEND_ENV) {
            assert_eq!(
                want, backend,
                "{BACKEND_ENV}={want} だが、この環境のバックエンドは {backend}"
            );
        }
        let update = std::env::var_os(UPDATE_ENV).is_some();
        let dir = root.join(backend);
        if !update && !dir.is_dir() {
            if std::env::var_os(REQUIRE_ENV).is_some() {
                panic!("{REQUIRE_ENV} が有るのに {} が無い", dir.display());
            }
            eprintln!(
                "期待画像のフォルダー {} が無いのでスキップする（cargo xtask golden-update --backend {backend}）",
                dir.display()
            );
            return None;
        }
        Some(Self {
            backend: backend.to_string(),
            dir,
            actual_dir: actual_dir.join(backend),
            update,
        })
    }

    /// 期待画像のパス。
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.png"))
    }

    /// `image` を期待画像 `name` と比べる。更新のときは書き換える。
    pub fn check(&self, name: &str, image: &RgbaImage) -> Result<(), String> {
        if image.size() != GOLDEN_SIZE {
            return Err(format!(
                "{name}: 期待画像は {}x{} だが {}x{} を渡された",
                GOLDEN_SIZE.width,
                GOLDEN_SIZE.height,
                image.width(),
                image.height()
            ));
        }
        let actual = to_srgb8(image);
        let path = self.path(name);
        if self.update {
            std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
            write_png(&path, GOLDEN_SIZE, &actual)?;
            eprintln!("期待画像を更新した: {}", path.display());
            return Ok(());
        }
        let (size, want) = read_png(&path)
            .map_err(|e| format!("{name}: 期待画像 {} を読めない: {e}", path.display()))?;
        if size != GOLDEN_SIZE {
            return Err(format!(
                "{name}: 期待画像の大きさが {}x{}",
                size.width, size.height
            ));
        }
        let report = compare(&want, &actual, GOLDEN_SIZE);
        if report.ssim >= MIN_SSIM && report.max_diff <= MAX_CHANNEL_DIFF {
            return Ok(());
        }
        let saved = self.actual_dir.join(format!("{name}.png"));
        std::fs::create_dir_all(&self.actual_dir).map_err(|e| e.to_string())?;
        write_png(&saved, GOLDEN_SIZE, &actual)?;
        Err(format!(
            "{name} ({}): SSIM {:.4}（下限 {MIN_SSIM}）、最大の差 {}/255（上限 {MAX_CHANNEL_DIFF}）。描いた画像: {}",
            self.backend,
            report.ssim,
            report.max_diff,
            saved.display()
        ))
    }
}

/// 比較の結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Comparison {
    pub ssim: f64,
    pub max_diff: u8,
}

/// 2 枚の sRGB の 8 bit（RGB の並び）を比べる。
pub fn compare(a: &[u8], b: &[u8], size: Size) -> Comparison {
    let max_diff = a
        .iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0);
    Comparison {
        ssim: ssim(&luma(a), &luma(b), size),
        max_diff,
    }
}

fn luma(rgb: &[u8]) -> Vec<f64> {
    rgb.chunks_exact(3)
        .map(|p| 0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
        .collect()
}

/// 輝度の SSIM。重ならない 8x8 のブロックの平均。
fn ssim(a: &[f64], b: &[f64], size: Size) -> f64 {
    const C1: f64 = (0.01 * 255.0) * (0.01 * 255.0);
    const C2: f64 = (0.03 * 255.0) * (0.03 * 255.0);
    let (w, h) = (size.width as usize, size.height as usize);
    let (mut total, mut windows) = (0.0, 0.0);
    for wy in 0..h / WINDOW {
        for wx in 0..w / WINDOW {
            let idx = |dy: usize, dx: usize| (wy * WINDOW + dy) * w + wx * WINDOW + dx;
            let n = (WINDOW * WINDOW) as f64;
            let (mut ma, mut mb) = (0.0, 0.0);
            for dy in 0..WINDOW {
                for dx in 0..WINDOW {
                    ma += a[idx(dy, dx)];
                    mb += b[idx(dy, dx)];
                }
            }
            let (ma, mb) = (ma / n, mb / n);
            let (mut va, mut vb, mut cov) = (0.0, 0.0, 0.0);
            for dy in 0..WINDOW {
                for dx in 0..WINDOW {
                    let (x, y) = (a[idx(dy, dx)] - ma, b[idx(dy, dx)] - mb);
                    va += x * x;
                    vb += y * y;
                    cov += x * y;
                }
            }
            let (va, vb, cov) = (va / (n - 1.0), vb / (n - 1.0), cov / (n - 1.0));
            total += ((2.0 * ma * mb + C1) * (2.0 * cov + C2))
                / ((ma * ma + mb * mb + C1) * (va + vb + C2));
            windows += 1.0;
        }
    }
    if windows == 0.0 {
        1.0
    } else {
        total / windows
    }
}

fn encode_srgb(v: f32) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

/// 線形の premultiplied を、灰色の市松の上に重ねて sRGB の 8 bit（RGB の並び）にする。
pub fn to_srgb8(image: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(image.size().area() * 3);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let p = image.pixel(x, y);
            let dark = (x / CHECKER_SIDE + y / CHECKER_SIDE).is_multiple_of(2);
            let back = if dark { CHECKER_DARK } else { CHECKER_LIGHT };
            out.extend([p.r, p.g, p.b].map(|c| encode_srgb(c + back * (1.0 - p.a))));
        }
    }
    out
}

fn write_png(path: &Path, size: Size, rgb: &[u8]) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), size.width, size.height);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(rgb))
        .map_err(|e| e.to_string())
}

fn read_png(path: &Path) -> Result<(Size, Vec<u8>), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .map_err(|e| e.to_string())?;
    let mut buf = vec![0; reader.output_buffer_size().ok_or("大きさが決まらない")?];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    if info.color_type != png::ColorType::Rgb || info.bit_depth != png::BitDepth::Eight {
        return Err("RGB の 8 bit ではない".into());
    }
    buf.truncate(info.buffer_size());
    Ok((Size::new(info.width, info.height), buf))
}

#[cfg(test)]
mod tests {
    use mm_types::Rgba;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("mm-golden-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn picture(offset: u32) -> RgbaImage {
        let mut img = RgbaImage::transparent(GOLDEN_SIZE);
        for y in 0..GOLDEN_SIZE.height {
            for x in 0..GOLDEN_SIZE.width {
                let v = ((x + offset) % 64) as f32 / 64.0;
                img.set_pixel(x, y, Rgba::new(v, y as f32 / 180.0, 0.3, 1.0));
            }
        }
        img
    }

    fn golden_with(root: &Path, backend: &str, update: bool) -> Golden {
        Golden {
            backend: backend.into(),
            dir: root.join(backend),
            actual_dir: root.join("actual").join(backend),
            update,
        }
    }

    #[test]
    fn バックエンドの名前を決める() {
        assert_eq!(
            backend_name("Dx12", "Microsoft Basic Render Driver"),
            "warp"
        );
        assert_eq!(backend_name("Dx12", "NVIDIA GeForce RTX 5090"), "rtx");
        assert_eq!(backend_name("Vulkan", "NVIDIA GeForce RTX 5090"), "rtx");
        assert_eq!(
            backend_name("Vulkan", "llvmpipe (LLVM 17.0.6, 256 bits)"),
            "lavapipe"
        );
        assert_eq!(
            backend_name("Dx12", "AMD Radeon(TM) Graphics"),
            "dx12-amd-radeon-tm-graphics"
        );
    }

    #[test]
    fn 更新した画像と同じ画像は合う() {
        let root = scratch("same");
        golden_with(&root, "t", true)
            .check("a", &picture(0))
            .unwrap();
        golden_with(&root, "t", false)
            .check("a", &picture(0))
            .unwrap();
    }

    #[test]
    fn ずれた画像は合わず描いた画像を書き出す() {
        let root = scratch("shift");
        golden_with(&root, "t", true)
            .check("a", &picture(0))
            .unwrap();
        let err = golden_with(&root, "t", false)
            .check("a", &picture(5))
            .unwrap_err();
        assert!(err.contains("SSIM"), "{err}");
        assert!(root.join("actual/t/a.png").is_file());
    }

    #[test]
    fn 期待画像が無ければ失敗する() {
        let root = scratch("missing");
        std::fs::create_dir_all(root.join("t")).unwrap();
        let err = golden_with(&root, "t", false)
            .check("none", &picture(0))
            .unwrap_err();
        assert!(err.contains("読めない"), "{err}");
    }

    #[test]
    fn 大きさが違う画像は拒否する() {
        let root = scratch("size");
        let small = RgbaImage::transparent(Size::new(8, 8));
        assert!(golden_with(&root, "t", false).check("a", &small).is_err());
    }

    #[test]
    fn わずかな丸めの差は許す() {
        let a = to_srgb8(&picture(0));
        let mut b = a.clone();
        b[10] = b[10].saturating_add(1);
        let c = compare(&a, &b, GOLDEN_SIZE);
        assert!(
            c.ssim >= MIN_SSIM && c.max_diff <= MAX_CHANNEL_DIFF,
            "{c:?}"
        );
    }
}
