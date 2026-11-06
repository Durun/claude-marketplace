//! 検査のための口。共有バッファの輪の途中で失敗させ（故障の注入）、取り込んだ CUDA の資源の数を数える。
//! feature `test-hooks` のときだけ働き、本番の組み立てでは何もしない。

use mm_types::Result;

/// 故障を起こせる場所。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    /// コピーを積む前。
    Memcpy,
    /// コピーの完了のシグナルを積む前。
    Signal,
    /// 枠の CUDA への取り込み。
    SlotImport,
}

#[cfg(feature = "test-hooks")]
pub use enabled::{inject, live_imports, Fault};

#[cfg(feature = "test-hooks")]
mod enabled {
    use std::sync::atomic::{AtomicI32, AtomicIsize, AtomicU8, Ordering};

    use mm_types::{MmError, Result};

    use super::Stage;

    /// 次に 1 回だけ起こす故障。
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Fault {
        /// 次のコピーを、積む前に失敗させる。
        CopyMemcpy,
        /// 次のコピーを、完了のシグナルを積む前に失敗させる（コピーは積まれた後）。
        CopySignal,
        /// 輪を作るとき、`at` 番目（0 から）の枠の取り込みを失敗させる。
        SlotImport { at: i32 },
    }

    static KIND: AtomicU8 = AtomicU8::new(0);
    static COUNTDOWN: AtomicI32 = AtomicI32::new(0);
    static LIVE: AtomicIsize = AtomicIsize::new(0);

    fn kind_of(stage: Stage) -> u8 {
        stage as u8 + 1
    }

    /// 故障を仕掛ける。起こすと解除される。
    pub fn inject(fault: Fault) {
        let (stage, at) = match fault {
            Fault::CopyMemcpy => (Stage::Memcpy, 0),
            Fault::CopySignal => (Stage::Signal, 0),
            Fault::SlotImport { at } => (Stage::SlotImport, at),
        };
        COUNTDOWN.store(at, Ordering::SeqCst);
        KIND.store(kind_of(stage), Ordering::SeqCst);
    }

    /// いま取り込んだままの CUDA の資源（外部メモリとセマフォ）の数。
    pub fn live_imports() -> isize {
        LIVE.load(Ordering::SeqCst)
    }

    pub(super) fn fault(stage: Stage) -> Result<()> {
        if KIND.load(Ordering::SeqCst) != kind_of(stage) {
            return Ok(());
        }
        if COUNTDOWN.fetch_sub(1, Ordering::SeqCst) > 0 {
            return Ok(());
        }
        KIND.store(0, Ordering::SeqCst);
        Err(MmError::Internal(format!("注入した故障: {stage:?}")))
    }

    pub(super) fn imported() {
        LIVE.fetch_add(1, Ordering::SeqCst);
    }

    pub(super) fn released() {
        LIVE.fetch_sub(1, Ordering::SeqCst);
    }
}

/// `stage` に故障が仕掛けてあれば起こす。
pub(crate) fn fault(stage: Stage) -> Result<()> {
    #[cfg(feature = "test-hooks")]
    {
        enabled::fault(stage)
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        let _ = stage;
        Ok(())
    }
}

/// CUDA の資源を取り込んだ。
pub(crate) fn imported() {
    #[cfg(feature = "test-hooks")]
    enabled::imported();
}

/// CUDA の資源を手放した。
pub(crate) fn released() {
    #[cfg(feature = "test-hooks")]
    enabled::released();
}
