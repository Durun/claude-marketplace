//! 1 フレームの PNG。Claude が目で見るための出口（`GET /frame`、`mm frame`）。
//!
//! 描画は専用のスレッドが持つ `Engine` で行う。アプリの再生スレッドの `Engine` とは別なので、
//! 再生を止めず、`Core` が送る触った ID で自分のキャッシュだけを捨てる。

use std::sync::mpsc::{channel, Sender};
use std::sync::Mutex;

use mm_doc::Project;
use mm_node_api::Quality;
use mm_runtime::{assemble, Engine, RuntimeConfig};
use mm_types::RgbaImage;
use mm_types::{CompositionId, Id, MmError, RationalTime, Result, Rgba, Size};

/// 描くフレームの指定。
#[derive(Debug, Clone, PartialEq)]
pub struct FrameRequest {
    /// コンポジションの ID。省くと `main`、それも無ければ先頭。
    pub composition: Option<String>,
    /// コンポジションの時間軸での時刻。
    pub time: RationalTime,
    /// 出力の幅。省くとコンポジションの大きさ。高さは縦横比を保つ。
    pub width: Option<u32>,
}

/// フレームを描く側。
pub trait FrameSource: Send + Sync {
    /// 素材のパスが絶対の `project` の 1 フレームを PNG にする。
    fn frame_png(&self, project: &Project, request: &FrameRequest) -> Result<Vec<u8>>;

    /// 操作が触った ID に頼ったキャッシュを捨てる。キャッシュを持たない実装は何もしない。
    fn invalidate(&self, _touched: &[String]) {}
}

/// `Engine` を専用のスレッドで持つ描画。最初の要求で組み立てる。
pub struct EngineFrames {
    jobs: Mutex<Sender<Job>>,
}

enum Job {
    Render {
        project: Box<Project>,
        request: FrameRequest,
        reply: Sender<Result<Vec<u8>>>,
    },
    Invalidate(Vec<String>),
}

impl EngineFrames {
    /// 描画のスレッドを始める。`config` の組み立ては最初の要求まで遅らせる。
    pub fn spawn(config: RuntimeConfig) -> Result<Self> {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("serve-frames".into())
            .spawn(move || {
                let mut engine: Option<Engine> = None;
                while let Ok(job) = rx.recv() {
                    match job {
                        Job::Invalidate(ids) => {
                            if let Some(e) = engine.as_mut() {
                                e.invalidate_touched(&ids);
                            }
                        }
                        Job::Render {
                            project,
                            request,
                            reply,
                        } => {
                            let result = ensure(&mut engine, &config)
                                .and_then(|e| render_png(e, &project, &request));
                            let _ = reply.send(result);
                        }
                    }
                }
            })
            .map_err(MmError::Io)?;
        Ok(Self {
            jobs: Mutex::new(tx),
        })
    }

    fn send(&self, job: Job) -> Result<()> {
        self.jobs
            .lock()
            .map_err(|_| MmError::Internal("描画の列が壊れた".into()))?
            .send(job)
            .map_err(|_| MmError::Internal("描画のスレッドが終わっている".into()))
    }
}

fn ensure<'a>(engine: &'a mut Option<Engine>, config: &RuntimeConfig) -> Result<&'a mut Engine> {
    if engine.is_none() {
        *engine = Some(assemble(config)?);
    }
    engine
        .as_mut()
        .ok_or_else(|| MmError::Internal("エンジンが組み立たない".into()))
}

impl FrameSource for EngineFrames {
    fn frame_png(&self, project: &Project, request: &FrameRequest) -> Result<Vec<u8>> {
        let (reply, rx) = channel();
        self.send(Job::Render {
            project: Box::new(project.clone()),
            request: request.clone(),
            reply,
        })?;
        rx.recv()
            .map_err(|_| MmError::Internal("描画のスレッドが結果を返す前に終わった".into()))?
    }

    fn invalidate(&self, touched: &[String]) {
        let _ = self.send(Job::Invalidate(touched.to_vec()));
    }
}

/// `engine` で 1 フレームを描いて PNG にする。`mm frame` の自前の描画も同じ道を通る。
pub fn render_png(
    engine: &mut Engine,
    project: &Project,
    request: &FrameRequest,
) -> Result<Vec<u8>> {
    let comp = pick_composition(project, request.composition.as_deref())?;
    let c = project.composition(&comp)?;
    let size = scaled(Size::new(c.width, c.height), request.width);
    let image = engine.render_frame(project, &comp, request.time, size, Quality::Full)?;
    encode_png(&image)
}

fn pick_composition(project: &Project, wanted: Option<&str>) -> Result<CompositionId> {
    if let Some(id) = wanted {
        let id: CompositionId = Id::new(id);
        project.composition(&id)?;
        return Ok(id);
    }
    let main: CompositionId = Id::new("main");
    if project.composition(&main).is_ok() {
        return Ok(main);
    }
    project
        .compositions
        .first()
        .map(|c| c.id.clone())
        .ok_or_else(|| MmError::NotFound("コンポジションが 1 つも無い".into()))
}

/// 幅を決めて縦横比を保つ。0 と元の大きさより大きい指定は元の大きさにする。
fn scaled(size: Size, width: Option<u32>) -> Size {
    match width {
        Some(w) if w > 0 && w < size.width => {
            let h = (u64::from(size.height) * u64::from(w) / u64::from(size.width)).max(1);
            Size::new(w, h as u32)
        }
        _ => size,
    }
}

/// 時刻の指定（`1.5`、`3/2`、`-0.4`）を読む。小数は 6 桁まで。
pub fn parse_time(raw: &str) -> Result<RationalTime> {
    const MAX_DECIMALS: usize = 6;
    let bad = || MmError::InvalidArgument(format!("時刻として読めない: {raw}"));
    let s = raw.trim();
    let (num, den) = if let Some((n, d)) = s.split_once('/') {
        (
            n.trim().parse::<i128>().map_err(|_| bad())?,
            d.trim().parse::<u32>().map_err(|_| bad())?,
        )
    } else {
        let (neg, body) = s.strip_prefix('-').map_or((false, s), |r| (true, r));
        let (int, frac) = body.split_once('.').unwrap_or((body, ""));
        if frac.len() > MAX_DECIMALS || (int.is_empty() && frac.is_empty()) {
            return Err(bad());
        }
        let magnitude: i128 = format!("{int}{frac}").parse().map_err(|_| bad())?;
        (
            if neg { -magnitude } else { magnitude },
            10u32.pow(frac.len() as u32),
        )
    };
    RationalTime::new(num, den)
}

/// 市松模様の一辺（ピクセル）。透明な所が分かるように下に敷く。
const CHECKER_SIDE: u32 = 16;
const CHECKER_DARK: f32 = 0.18;
const CHECKER_LIGHT: f32 = 0.26;

/// 線形の premultiplied の画像を、灰色の市松の上に重ねた sRGB の 8 bit の PNG にする。
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>> {
    let mut rgb = Vec::with_capacity(image.size().area() * 3);
    for y in 0..image.height() {
        for x in 0..image.width() {
            let p: Rgba = image.pixel(x, y);
            let dark = (x / CHECKER_SIDE + y / CHECKER_SIDE).is_multiple_of(2);
            let back = if dark { CHECKER_DARK } else { CHECKER_LIGHT };
            rgb.extend([p.r, p.g, p.b].map(|c| encode_srgb(c + back * (1.0 - p.a))));
        }
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, image.width(), image.height());
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header()
        .and_then(|mut w| w.write_image_data(&rgb))
        .map_err(|e| MmError::Internal(format!("PNG に書けない: {e}")))?;
    Ok(out)
}

fn encode_srgb(linear: f32) -> u8 {
    let c = linear.clamp(0.0, 1.0);
    let s = if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 時刻は分数と小数を読む() {
        assert_eq!(parse_time("3/2").unwrap().to_f64(), 1.5);
        assert_eq!(parse_time("-0.4").unwrap().to_f64(), -0.4);
        assert_eq!(parse_time("2").unwrap().to_f64(), 2.0);
        assert!(parse_time("abc").is_err());
        assert!(parse_time("1.1234567").is_err());
    }

    #[test]
    fn 幅の指定は縦横比を保ち_元より大きくしない() {
        let s = Size::new(1920, 1080);
        assert_eq!(scaled(s, Some(960)), Size::new(960, 540));
        assert_eq!(scaled(s, Some(4000)), s);
        assert_eq!(scaled(s, None), s);
    }

    #[test]
    fn 画像は_png_にできる() {
        let img = RgbaImage::filled(Size::new(4, 2), Rgba::WHITE);
        let png = encode_png(&img).unwrap();
        assert_eq!(&png[1..4], b"PNG");
    }
}
