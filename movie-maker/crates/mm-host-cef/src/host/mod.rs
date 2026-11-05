//! CEF の起動と、コマンドラインの入口。

mod bench;
mod gpu;
mod handlers;

use std::path::PathBuf;
use std::process::ExitCode;

use cef::{args::Args, *};

/// `mm-host-cef bench` の指定。
pub struct BenchOptions {
    pub accelerated: bool,
    pub frames: u32,
    pub warmup: u32,
    pub size: (u32, u32),
    pub fps: u32,
    pub out: Option<PathBuf>,
    pub verify: bool,
    /// 使う GPU の名前の一部。空なら wgpu の既定。
    pub adapter: String,
    /// `windowless_frame_rate`。外部 BeginFrame でも、この値で描画の間隔の下限が決まる。
    pub rate: u32,
    /// BeginFrame を送り直すまでの待ち。
    pub resend: std::time::Duration,
}

impl Default for BenchOptions {
    fn default() -> Self {
        Self {
            accelerated: true,
            frames: 300,
            warmup: 20,
            size: (1920, 1080),
            fps: 30,
            out: None,
            verify: false,
            adapter: String::new(),
            rate: 240,
            resend: std::time::Duration::from_millis(3),
        }
    }
}

fn parse_args() -> Result<BenchOptions, String> {
    let mut opts = BenchOptions::default();
    let mut it = std::env::args()
        .skip(1)
        .filter(|a| !(a.starts_with("--") && a.contains('=')));
    if it.next().as_deref() != Some("bench") {
        return Err("使い方: mm-host-cef bench [--path accel|cpu] [--frames N] [--warmup N] [--size WxH] [--fps N] [--out DIR] [--verify]".into());
    }
    while let Some(flag) = it.next() {
        // `--名前=値` は Chromium のスイッチ。CEF が自分で読むのでここでは飛ばす。
        if flag.starts_with("--") && flag.contains('=') {
            continue;
        }
        let mut value = || it.next().ok_or_else(|| format!("{flag} に値が要る"));
        match flag.as_str() {
            "--path" => opts.accelerated = value()? != "cpu",
            "--frames" => opts.frames = value()?.parse().map_err(|e| format!("--frames: {e}"))?,
            "--warmup" => opts.warmup = value()?.parse().map_err(|e| format!("--warmup: {e}"))?,
            "--fps" => opts.fps = value()?.parse().map_err(|e| format!("--fps: {e}"))?,
            "--size" => {
                let v = value()?;
                let (w, h) = v.split_once('x').ok_or("--size は WxH")?;
                opts.size = (
                    w.parse().map_err(|e| format!("--size: {e}"))?,
                    h.parse().map_err(|e| format!("--size: {e}"))?,
                );
            }
            "--out" => opts.out = Some(PathBuf::from(value()?)),
            "--verify" => opts.verify = true,
            "--adapter" => opts.adapter = value()?,
            "--rate" => opts.rate = value()?.parse().map_err(|e| format!("--rate: {e}"))?,
            "--resend-us" => {
                opts.resend = std::time::Duration::from_micros(
                    value()?.parse().map_err(|e| format!("--resend-us: {e}"))?,
                )
            }
            other => return Err(format!("知らない指定: {other}")),
        }
    }
    Ok(opts)
}

/// 入口。CEF はサブプロセス（GPU、レンダラ）にも同じ実行ファイルを使うので、最初に判定する。
pub fn run() -> ExitCode {
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let Some(cmd) = args.as_cmd_line() else {
        eprintln!("コマンドラインを読めない");
        return ExitCode::FAILURE;
    };
    let is_browser = cmd.has_switch(Some(&CefString::from("type"))) != 1;
    let code = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    if !is_browser {
        return if code >= 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let opts = match parse_args() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match start_and_bench(&args, &opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("失敗: {e}");
            ExitCode::FAILURE
        }
    }
}

fn start_and_bench(args: &Args, opts: &BenchOptions) -> Result<(), String> {
    let cef_dir = std::env::var("CEF_PATH")
        .map_err(|_| "環境変数 CEF_PATH（CEF の配布物の置き場）が無い".to_string())?;
    let cache = std::env::temp_dir().join("mm-host-cef-cache");
    let log_file = std::env::var("MM_CEF_LOG").ok();
    let settings = Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        no_sandbox: true as _,
        root_cache_path: CefString::from(cache.to_string_lossy().as_ref()),
        resources_dir_path: CefString::from(cef_dir.as_str()),
        locales_dir_path: CefString::from(format!("{cef_dir}/locales").as_str()),
        // 通常は警告以上だけ。MM_CEF_LOG にファイルを渡すと詳細を書く（GPU の選択の調査用）。
        log_severity: LogSeverity::from(if log_file.is_some() {
            sys::cef_log_severity_t::LOGSEVERITY_VERBOSE
        } else {
            sys::cef_log_severity_t::LOGSEVERITY_WARNING
        }),
        log_file: CefString::from(log_file.as_deref().unwrap_or("")),
        ..Default::default()
    };
    let mut app = handlers::HostApp::new(true);
    if initialize(
        Some(args.as_main_args()),
        Some(&settings),
        Some(&mut app),
        std::ptr::null_mut(),
    ) != 1
    {
        return Err("cef::initialize が失敗".into());
    }
    let result = bench::run(opts);
    shutdown();
    result
}
