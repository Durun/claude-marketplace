//! CEF の起動と、コマンドラインのエントリーポイント（`bench` と `serve`）。

mod args;
mod bench;
mod browser;
mod gpu;
mod guard;
mod handlers;
mod serve;
mod share;

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;

use cef::{args::Args, *};
use mm_gpu::GpuContext;
use mm_host_api::HostSpec;

pub use args::BenchOptions;

/// Windows のタイマーの粒度（ms）。
const TIMER_PERIOD_MS: u32 = 1;

/// 起動の後に何をするか。
enum Mode {
    Bench(BenchOptions),
    Serve(HostSpec),
}

/// エントリーポイント。CEF はサブプロセス（GPU、レンダラ）にも同じ実行ファイルを使うので、最初に判定する。
pub fn run(sandbox: *mut std::ffi::c_void) -> ExitCode {
    raise_timer_resolution();
    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let args = Args::new();
    let Some(cmd) = args.as_cmd_line() else {
        eprintln!("コマンドラインを読めない");
        return ExitCode::FAILURE;
    };
    let is_browser = cmd.has_switch(Some(&CefString::from("type"))) != 1;
    let code = execute_process(Some(args.as_main_args()), None::<&mut App>, sandbox.cast());
    if !is_browser {
        return if code >= 0 {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        };
    }
    let mode = match parse_mode() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    match start(&args, mode, sandbox) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("失敗: {e}");
            ExitCode::FAILURE
        }
    }
}

/// CEF のプロセス（GPU とレンダラも同じ実行ファイル）が 1 ms のタイマーで動くようにする。
/// Windows 10 2004 以降はタイマーの粒度がプロセスごとで、既定の 15.6 ms のままだと
/// BeginFrame からフレームが来るまでが 15 ms 前後に揃ってしまう。
fn raise_timer_resolution() {
    // SAFETY: 引数は値だけで、失敗しても既定の粒度で動き続ける。
    unsafe { windows::Win32::Media::timeBeginPeriod(TIMER_PERIOD_MS) };
}

fn parse_mode() -> Result<Mode, String> {
    match std::env::args().nth(1).as_deref() {
        Some("bench") => Ok(Mode::Bench(args::parse_bench()?)),
        Some(mm_host_cef::SERVE_COMMAND) => {
            let json = std::env::var(mm_host_cef::SPEC_ENV)
                .map_err(|_| format!("環境変数 {} が無い", mm_host_cef::SPEC_ENV))?;
            let spec = serde_json::from_str(&json).map_err(|e| format!("HostSpec: {e}"))?;
            Ok(Mode::Serve(spec))
        }
        _ => Err(args::USAGE.into()),
    }
}

/// CEF に揃えさせる GPU。`use-adapter-luid` は Chromium のスイッチで、値は「HighPart,LowPart」の 10 進。
fn luid_switch(gpu: &GpuContext) -> Option<String> {
    let luid = gpu.adapter.luid?;
    let high = (luid >> 32) as u32 as i32;
    let low = luid as u32;
    Some(format!("{high},{low}"))
}

fn settings(cef_dir: &str, cache: &std::path::Path, no_sandbox: bool) -> Settings {
    let log_file = std::env::var("MM_CEF_LOG").ok();
    Settings {
        windowless_rendering_enabled: true as _,
        external_message_pump: true as _,
        no_sandbox: no_sandbox as _,
        root_cache_path: CefString::from(cache.to_string_lossy().as_ref()),
        resources_dir_path: CefString::from(cef_dir),
        locales_dir_path: CefString::from(format!("{cef_dir}/locales").as_str()),
        // 通常は警告以上だけ。MM_CEF_LOG にファイルを渡すと詳細を書く（GPU の選択の調査用）。
        log_severity: LogSeverity::from(if log_file.is_some() {
            sys::cef_log_severity_t::LOGSEVERITY_VERBOSE
        } else {
            sys::cef_log_severity_t::LOGSEVERITY_WARNING
        }),
        log_file: CefString::from(log_file.as_deref().unwrap_or("")),
        ..Default::default()
    }
}

fn start(args: &Args, mode: Mode, sandbox: *mut std::ffi::c_void) -> Result<(), String> {
    let cef_dir = std::env::var(mm_host_cef::CEF_PATH_ENV)
        .map_err(|_| "環境変数 CEF_PATH（CEF の配布物の配置先）が無い".to_string())?;
    let wanted = match &mode {
        Mode::Bench(opts) => gpu::find_adapter(&opts.adapter)?,
        Mode::Serve(spec) => spec.gpu.clone(),
    };
    let gpu = gpu::open(wanted)?;
    let (work, owned) = work_dir();
    let cache = work.join("cache");
    let mut app = handlers::HostApp::new(luid_switch(&gpu));
    if initialize(
        Some(args.as_main_args()),
        Some(&settings(&cef_dir, &cache, sandbox.is_null())),
        Some(&mut app),
        sandbox.cast(),
    ) != 1
    {
        return Err("cef::initialize が失敗".into());
    }
    let result = match mode {
        Mode::Bench(opts) => bench::run(&opts, Arc::clone(&gpu)),
        Mode::Serve(spec) => serve::run(&spec, Arc::clone(&gpu)),
    };
    shutdown();
    // 片付けに失敗しても、次の起動は別のフォルダーを使うので続ける。ランチャーのフォルダーはランチャーが消す。
    let _ = std::fs::remove_dir_all(if owned { &work } else { &cache });
    result
}

/// ホストの一時フォルダー（CEF のキャッシュとページの置き場）と、自分で作ったか。
/// ランチャーが渡したものを使い、無ければ（bench）pid で分ける。同じキャッシュのフォルダーは 1 プロセスしか使えない。
pub fn work_dir() -> (PathBuf, bool) {
    match std::env::var_os(mm_host_cef::WORK_DIR_ENV) {
        Some(dir) => (PathBuf::from(dir), false),
        None => (
            std::env::temp_dir().join(format!("mm-host-cef-{}", std::process::id())),
            true,
        ),
    }
}
