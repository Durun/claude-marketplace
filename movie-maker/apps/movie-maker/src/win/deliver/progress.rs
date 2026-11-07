//! 書き出しの進み具合をキューの状態へ反映して送る。

use super::*;

/// キューの `index` 番目の進み具合を状態に反映し、`EMIT_INTERVAL_MS` ごと（と最後のフレーム）にだけ状態を送る。
/// 状態の複製は送るときだけにして、フレームごとの負担を小さくする。
pub(super) fn progress_reporter(
    status: Arc<Mutex<ExportStatus>>,
    emit: Emit,
    index: usize,
) -> ProgressFn {
    let started = Instant::now();
    let last_ms = AtomicU64::new(0);
    Arc::new(move |p: &ExportProgress| {
        let now = started.elapsed().as_millis() as u64;
        let finished = p.frames_total > 0 && p.frames_done >= p.frames_total;
        let due = now.saturating_sub(last_ms.load(Ordering::Relaxed)) >= EMIT_INTERVAL_MS;
        let mut status = status.lock().expect("export status");
        if let Some(item) = status.items.get_mut(index) {
            apply_progress(item, p);
        }
        if finished || due {
            last_ms.store(now, Ordering::Relaxed);
            let snapshot = status.clone();
            drop(status);
            emit(&snapshot);
        }
    })
}

fn apply_progress(item: &mut ExportItem, p: &ExportProgress) {
    item.frames_done = p.frames_done;
    item.frames_total = p.frames_total;
    item.elapsed = p.elapsed.as_secs_f64();
    item.fps = p.frames_done as f64 / item.elapsed.max(f64::EPSILON);
    item.eta = (p.frames_done > 0 && item.fps > 0.0)
        .then(|| p.frames_total.saturating_sub(p.frames_done) as f64 / item.fps);
}
