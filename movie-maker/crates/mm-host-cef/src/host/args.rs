//! `mm-host-cef bench` の指定。

use std::path::PathBuf;
use std::time::Duration;

/// 使い方。
pub const USAGE: &str = "使い方: mm-host-cef bench [--path accel|cpu] [--share] [--frames N] [--warmup N] [--size WxH] [--fps N] [--rate N] [--resend-us N] [--adapter 名前] [--page-css CSS] [--out DIR] [--verify]\n       mm-host-cef serve（環境変数 MM_HOST_SPEC に HostSpec の JSON）";

/// `windowless_frame_rate` の既定。外部 BeginFrame でもフレームを受け取る間隔の下限になる（60 だと 1 フレーム 35 ms）。
/// 240 では、4.2 ms より短い間隔で描けたフレームの受け取りが 20 ms ほど遅れる（720p で p95 26 ms）ので、1000 にする。
pub const DEFAULT_RATE: u32 = 1000;
/// BeginFrame を送り直すまでの既定の待ち。BeginFrame からフレームが来るまでの最短（約 3.5 ms）より短くすると、
/// 送った分が捨てられて却って遅くなる。
pub const DEFAULT_RESEND: Duration = Duration::from_millis(3);

/// `mm-host-cef bench` の指定。
pub struct BenchOptions {
    pub accelerated: bool,
    /// 共有テクスチャ（D3D12 の共有ヒープ）へコピーし、NT ハンドルを複製するところまで測る。
    pub share: bool,
    pub frames: u32,
    pub warmup: u32,
    pub size: (u32, u32),
    pub fps: u32,
    pub out: Option<PathBuf>,
    pub verify: bool,
    /// 使う GPU の名前の一部。空なら高性能のアダプター。
    pub adapter: String,
    /// `windowless_frame_rate`。
    pub rate: u32,
    /// BeginFrame を送り直すまでの待ち。
    pub resend: Duration,
    /// ページの先頭に足す CSS（原因の切り分け用）。
    pub page_css: String,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            accelerated: true,
            share: false,
            frames: 300,
            warmup: 20,
            size: (1920, 1080),
            fps: 30,
            out: None,
            verify: false,
            adapter: String::new(),
            rate: DEFAULT_RATE,
            resend: DEFAULT_RESEND,
            page_css: String::new(),
        }
    }
}

fn number<T: std::str::FromStr>(flag: &str, v: &str) -> Result<T, String>
where
    T::Err: std::fmt::Display,
{
    v.parse().map_err(|e| format!("{flag}: {e}"))
}

fn parse_size(v: &str) -> Result<(u32, u32), String> {
    let (w, h) = v.split_once('x').ok_or("--size は WxH")?;
    Ok((number("--size", w)?, number("--size", h)?))
}

/// コマンドラインを読む。`--名前=値` は Chromium のスイッチとして CEF が読むのでスキップする。
pub fn parse_bench() -> Result<BenchOptions, String> {
    let mut opts = BenchOptions::default();
    let mut it = std::env::args()
        .skip(2)
        .filter(|a| !(a.starts_with("--") && a.contains('=')));
    while let Some(flag) = it.next() {
        let mut value = || it.next().ok_or_else(|| format!("{flag} に値が要る"));
        match flag.as_str() {
            "--path" => opts.accelerated = value()? != "cpu",
            "--share" => opts.share = true,
            "--frames" => opts.frames = number(&flag, &value()?)?,
            "--warmup" => opts.warmup = number(&flag, &value()?)?,
            "--fps" => opts.fps = number(&flag, &value()?)?,
            "--size" => opts.size = parse_size(&value()?)?,
            "--out" => opts.out = Some(PathBuf::from(value()?)),
            "--verify" => opts.verify = true,
            "--adapter" => opts.adapter = value()?,
            "--page-css" => opts.page_css = value()?,
            "--rate" => opts.rate = number(&flag, &value()?)?,
            "--resend-us" => opts.resend = Duration::from_micros(number(&flag, &value()?)?),
            other => return Err(format!("知らない指定: {other}\n{USAGE}")),
        }
    }
    Ok(opts)
}
