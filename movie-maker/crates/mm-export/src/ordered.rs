//! 順序つきの並行実行。区間（チャンク）を複数のワーカーが先取りして作り、受け取る側が番号順に 1 つずつ受け取る。
//!
//! 受け取りが `window` 個より先へ進めないよう、ワーカーは受け取りの番号 + `window` 未満のチャンクだけを取る。
//! 作った結果が溜まる量を `window` と同時に作る数で抑えるため。どれかが失敗したら残りを止め、最初のエラーを返す。

use std::collections::HashMap;
use std::sync::{Condvar, Mutex};

use mm_types::{MmError, Result};

struct State<T> {
    /// 次に取るチャンクの番号。
    next: usize,
    /// 受け取り済みのチャンクの数。
    consumed: usize,
    done: HashMap<usize, T>,
    error: Option<MmError>,
    stopped: bool,
}

struct Shared<T> {
    state: Mutex<State<T>>,
    changed: Condvar,
}

impl<T> Shared<T> {
    fn lock(&self) -> std::sync::MutexGuard<'_, State<T>> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 最初のエラーだけを残して止める。
    fn fail(&self, error: MmError) {
        let mut s = self.lock();
        s.error.get_or_insert(error);
        s.stopped = true;
        self.changed.notify_all();
    }

    fn stop(&self) {
        self.lock().stopped = true;
        self.changed.notify_all();
    }

    /// 次に作るチャンクの番号。止まっているか、全部取り終えたら `None`。
    fn claim(&self, chunks: usize, window: usize) -> Option<usize> {
        let mut s = self.lock();
        loop {
            if s.stopped || s.next >= chunks {
                return None;
            }
            if s.next < s.consumed + window {
                s.next += 1;
                return Some(s.next - 1);
            }
            s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn publish(&self, index: usize, value: T) {
        self.lock().done.insert(index, value);
        self.changed.notify_all();
    }

    /// チャンク `index` の結果を待つ。
    fn take(&self, index: usize) -> Result<T> {
        let mut s = self.lock();
        loop {
            if let Some(error) = s.error.take() {
                return Err(error);
            }
            if let Some(value) = s.done.remove(&index) {
                s.consumed = index + 1;
                self.changed.notify_all();
                return Ok(value);
            }
            if s.stopped {
                return Err(MmError::Internal("並行の書き出しが止まった".into()));
            }
            s = self.changed.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }
}

/// 受け取る側が抜けたら（エラーでも）ワーカーを止める。
struct StopOnDrop<'a, T>(&'a Shared<T>);

impl<T> Drop for StopOnDrop<'_, T> {
    fn drop(&mut self) {
        self.0.stop();
    }
}

/// `chunks` 個のチャンクを `workers` 本のスレッドで作り、`consume` に番号順に渡す。
///
/// 各ワーカーは `make_worker` で自分の状態（レンダラーなど）を作り、`render` でチャンクを作る。
/// `window` は、受け取り済みの数より先に取ってよいチャンクの数。`workers` 以上にしないと並行にならない。
/// `consume` はこのスレッドで呼ぶ。
pub fn run_ordered<W, T: Send>(
    chunks: usize,
    workers: usize,
    window: usize,
    make_worker: impl Fn() -> Result<W> + Sync,
    render: impl Fn(&mut W, usize) -> Result<T> + Sync,
    mut consume: impl FnMut(usize, T) -> Result<()>,
) -> Result<()> {
    let shared = Shared {
        state: Mutex::new(State {
            next: 0,
            consumed: 0,
            done: HashMap::new(),
            error: None,
            stopped: false,
        }),
        changed: Condvar::new(),
    };
    let window = window.max(workers).max(1);
    std::thread::scope(|scope| {
        for _ in 0..workers.max(1) {
            scope.spawn(|| {
                let mut worker = match make_worker() {
                    Ok(w) => w,
                    Err(e) => return shared.fail(e),
                };
                while let Some(index) = shared.claim(chunks, window) {
                    match render(&mut worker, index) {
                        Ok(value) => shared.publish(index, value),
                        Err(e) => return shared.fail(e),
                    }
                }
            });
        }
        let _stop = StopOnDrop(&shared);
        for index in 0..chunks {
            consume(index, shared.take(index)?)?;
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn 作る順が入れ替わっても番号順に受け取る() {
        let mut seen = Vec::new();
        run_ordered(
            40,
            4,
            8,
            || Ok(()),
            |_, i| {
                // 小さい番号ほど遅くして、後ろのチャンクが先に出来上がるようにする
                std::thread::sleep(Duration::from_millis(((40 - i) % 7) as u64 * 3));
                Ok(i * 10)
            },
            |i, v| {
                assert_eq!(v, i * 10);
                seen.push(i);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(seen, (0..40).collect::<Vec<_>>());
    }

    #[test]
    fn 先に作る数は窓で抑える() {
        let produced = AtomicUsize::new(0);
        let max_ahead = AtomicUsize::new(0);
        let consumed = AtomicUsize::new(0);
        run_ordered(
            60,
            3,
            5,
            || Ok(()),
            |_, i| {
                let n = produced.fetch_add(1, Ordering::SeqCst) + 1;
                let ahead = n.saturating_sub(consumed.load(Ordering::SeqCst));
                max_ahead.fetch_max(ahead, Ordering::SeqCst);
                Ok(i)
            },
            |_, _| {
                std::thread::sleep(Duration::from_millis(2));
                consumed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
        )
        .unwrap();
        // 受け取り中の 1 個は、受け取り済みの数に入る前（`take` が数える）に、このテストの数えでは未受け取り
        assert!(
            max_ahead.load(Ordering::SeqCst) <= 5 + 1,
            "窓を超えて作った"
        );
    }

    #[test]
    fn ワーカーの失敗は最初のエラーで止まる() {
        let r = run_ordered(
            20,
            4,
            8,
            || Ok(()),
            |_, i| {
                if i == 7 {
                    Err(MmError::Corrupt("7".into()))
                } else {
                    Ok(i)
                }
            },
            |_, _| Ok(()),
        );
        assert!(matches!(r, Err(MmError::Corrupt(m)) if m == "7"));
    }

    #[test]
    fn 受け取りの失敗でワーカーが止まって戻る() {
        let r = run_ordered(
            1000,
            4,
            8,
            || Ok(()),
            |_, i| Ok(i),
            |i, _| {
                if i == 3 {
                    Err(MmError::Internal("止める".into()))
                } else {
                    Ok(())
                }
            },
        );
        assert!(matches!(r, Err(MmError::Internal(m)) if m == "止める"));
    }

    #[test]
    fn ワーカーを作れなければそのエラーを返す() {
        let r: Result<()> = run_ordered(
            5,
            2,
            4,
            || Err::<(), _>(MmError::Unsupported("GPU が無い".into())),
            |_, i| Ok(i),
            |_, _| Ok(()),
        );
        assert!(matches!(r, Err(MmError::Unsupported(_))));
    }
}
