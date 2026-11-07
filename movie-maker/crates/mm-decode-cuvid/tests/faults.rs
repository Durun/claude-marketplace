//! 共有リングバッファの途中で失敗させ（故障の注入）、後始末と回復を確かめる。
//! 故障と資源の数はプロセスで 1 つなので、このファイルのテストは 1 つずつ走らせ、別の実行ファイルに置く（docs/ci.md）。
#![cfg(feature = "cuvid")]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mm_decode_api::Decoder;
use mm_decode_cuvid::{gpu_request, inject, live_imports, CuvidDecoder, Fault};
use mm_decode_hw::testing::{fixtures, gpu, gpu_available};
use mm_gpu::GpuContext;
use mm_types::RationalTime;

static ONE_AT_A_TIME: Mutex<()> = Mutex::new(());

/// 回復を確かめる待ちの上限。止まるバグでは永遠に返らないので、時間で見切る。
const DEADLINE: Duration = Duration::from_secs(60);

/// GPU や CUDA が無い環境（CI）ではテストをスキップする。
fn available() -> bool {
    gpu_available(|| {
        let ctx = GpuContext::shared(&gpu_request(None)?)?;
        CuvidDecoder::open(&fixtures(&["cfr.mp4"]).join("cfr.mp4"), &ctx).map(drop)
    })
}

fn ctx() -> Arc<GpuContext> {
    gpu(gpu_request(None).expect("D3D12 のアダプター"))
}

/// `f` を別のスレッドで走らせ、`DEADLINE` までに終わらなければ失敗にする。
fn within_deadline(what: &str, f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    rx.recv_timeout(DEADLINE)
        .unwrap_or_else(|_| panic!("{what}: CUDA のストリームが止まった"));
}

/// コピーの途中で失敗しても、スロットのフェンスの回数がずれて 4 フレーム後に止まることがない。
fn recovers_after(fault: Fault) {
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let ctx = ctx();
    let path = fixtures(&["cfr.mp4"]).join("cfr.mp4");
    within_deadline("故障のあとのデコード", move || {
        let mut dec = CuvidDecoder::open(&path, &ctx).expect("開ける");
        inject(fault);
        assert!(
            dec.next_frame().is_err(),
            "仕掛けた故障で 1 フレーム目が失敗する"
        );
        // リングバッファは 4 スロット。故障のスロットを含めて何周もして、止まらず全フレームが出る。
        let mut count = 0;
        while let Some(frame) = dec.next_frame().expect("故障のあとはデコードできる") {
            drop(frame);
            count += 1;
        }
        assert!(count >= 50, "残りのフレームが出た: {count}");
        dec.seek(RationalTime::ZERO).expect("seek");
        assert!(dec.next_frame().expect("デコード").is_some());
    });
}

#[test]
fn コピーを積む前に失敗してもリングバッファが止まらない() {
    if !available() {
        return;
    }
    recovers_after(Fault::CopyMemcpy);
}

#[test]
fn コピーを積んだあとシグナルの前に失敗してもリングバッファが止まらない() {
    if !available() {
        return;
    }
    recovers_after(Fault::CopySignal);
}

/// リングバッファを作る途中で失敗しても、それまでに取り込んだ CUDA の外部メモリとセマフォを漏らさない。
#[test]
fn リングバッファの途中で失敗しても取り込みを漏らさない() {
    if !available() {
        return;
    }
    let _one = ONE_AT_A_TIME.lock().unwrap_or_else(|e| e.into_inner());
    let ctx = ctx();
    let path = fixtures(&["cfr.mp4"]).join("cfr.mp4");
    let before = live_imports();
    inject(Fault::SlotImport { at: 3 });
    assert!(
        CuvidDecoder::open(&path, &ctx).is_err(),
        "4 スロット目で失敗する"
    );
    assert_eq!(live_imports(), before, "3 スロット分の取り込みを解放した");

    let dec = CuvidDecoder::open(&path, &ctx).expect("故障のあとは開ける");
    assert!(live_imports() > before, "4 スロットが取り込まれている");
    drop(dec);
    assert_eq!(
        live_imports(),
        before,
        "デコーダーをドロップすると全部解放する"
    );
}
