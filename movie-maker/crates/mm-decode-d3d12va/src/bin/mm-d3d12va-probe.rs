//! 素材の先頭の数コマを d3d12va で復号できるかを、別プロセスで確かめる。
//!
//! NVIDIA の d3d12va は一部の素材（NVENC で作った 4K の HEVC など）の復号でドライバーが内部エラーになり、
//! プロセスの D3D12 のデバイスが全部消える（同じアダプターの別のデバイスも）。描画器のデバイスを守るため、
//! 開く前にこのプロセスで試す。使い方: `mm-d3d12va-probe <素材> --luid <アダプターの LUID>`。
//! 復号できれば終了コード 0、できなければ理由を標準エラーに出して 1。

use std::path::PathBuf;
use std::process::ExitCode;

use mm_decode_api::Decoder;
use mm_decode_d3d12va::shared::adapter::dx12_request_for_luid;
use mm_decode_d3d12va::{required_features, D3d12vaDecoder, PROBE_FRAMES};
use mm_gpu::GpuContext;

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let path = PathBuf::from(args.next().ok_or("素材のパスが要る")?);
    let (flag, luid) = (args.next(), args.next());
    if flag.as_deref() != Some("--luid") {
        return Err("--luid <LUID> が要る".into());
    }
    let luid: u64 = luid.and_then(|v| v.parse().ok()).ok_or("LUID は整数")?;
    let request = dx12_request_for_luid(luid, required_features()).map_err(|e| e.to_string())?;
    let ctx = GpuContext::with(&request).map_err(|e| e.to_string())?;
    let mut dec = D3d12vaDecoder::open(&path, &ctx).map_err(|e| e.to_string())?;
    for _ in 0..PROBE_FRAMES {
        if dec.next_frame().map_err(|e| e.to_string())?.is_none() {
            break;
        }
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
