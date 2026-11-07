//! 再生の主の時計。音の出力があればその位置（耳に届いている位置）を使い、無ければ壁時計で進める。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

use mm_audio_cpal::AudioOutput;

/// 再生位置（コンポジションの時間軸の秒）を返す時計。映像はこれに合わせる。
pub trait PlayClock: Send + Sync {
    /// 時計の種類（`audio`、`wall`）。
    fn kind(&self) -> &'static str;
    /// 止めたまま位置を `to` に置く。
    fn seek(&self, to: f64);
    /// 進め始める。
    fn run(&self);
    /// 止めて、止めた位置を返す。
    fn halt(&self) -> f64;
    /// いまの位置。
    fn position(&self) -> f64;
    /// 音が尽きて無音を出した回数。音の出力が無い時計は 0。
    fn underruns(&self) -> u64 {
        0
    }
    /// 時計の元（音の出力のストリームなど）が壊れて、位置が進まなくなったか。
    fn failed(&self) -> bool {
        false
    }
}

/// 音の出力の位置を時計にする。音は `Player` が `AudioOutput` に足す。
pub struct AudioClock(pub Arc<AudioOutput>);

impl PlayClock for AudioClock {
    fn kind(&self) -> &'static str {
        "audio"
    }

    fn seek(&self, to: f64) {
        self.0.halt();
        self.0.reset(to);
    }

    fn run(&self) {
        self.0.run();
    }

    fn halt(&self) -> f64 {
        self.0.halt()
    }

    fn position(&self) -> f64 {
        self.0.position_secs()
    }

    fn underruns(&self) -> u64 {
        self.0.underruns()
    }

    fn failed(&self) -> bool {
        self.0.stream_failed()
    }
}

/// 音の出力が無いときの時計。
#[derive(Default)]
pub struct WallClock {
    state: Mutex<WallState>,
}

#[derive(Default)]
struct WallState {
    base: f64,
    started: Option<Instant>,
}

impl WallState {
    fn now(&self) -> f64 {
        self.base + self.started.map_or(0.0, |t| t.elapsed().as_secs_f64())
    }
}

impl PlayClock for WallClock {
    fn kind(&self) -> &'static str {
        "wall"
    }

    fn seek(&self, to: f64) {
        *self.state.lock().expect("wall clock") = WallState {
            base: to,
            started: None,
        };
    }

    fn run(&self) {
        let mut s = self.state.lock().expect("wall clock");
        if s.started.is_none() {
            s.started = Some(Instant::now());
        }
    }

    fn halt(&self) -> f64 {
        let mut s = self.state.lock().expect("wall clock");
        let now = s.now();
        *s = WallState {
            base: now,
            started: None,
        };
        now
    }

    fn position(&self) -> f64 {
        self.state.lock().expect("wall clock").now()
    }
}

/// 主の時計を後から壁時計に替えられるラッパー。音の出力が途中で壊れても、同じ位置から映像を進め続ける。
/// 共有状態には最初からこれを置くので、替えても読む側は気づかない。
pub struct SwitchClock {
    inner: RwLock<Arc<dyn PlayClock>>,
    running: AtomicBool,
}

impl SwitchClock {
    pub fn new(inner: Arc<dyn PlayClock>) -> Self {
        Self {
            inner: RwLock::new(inner),
            running: AtomicBool::new(false),
        }
    }

    fn current(&self) -> Arc<dyn PlayClock> {
        self.inner.read().expect("switch clock").clone()
    }

    /// 元の時計が壊れていたら、止めた位置から壁時計に替える。替えたら真。
    pub fn fall_back_if_failed(&self) -> bool {
        let mut inner = self.inner.write().expect("switch clock");
        if !inner.failed() {
            return false;
        }
        let wall = WallClock::default();
        wall.seek(inner.halt());
        if self.running.load(Ordering::SeqCst) {
            wall.run();
        }
        *inner = Arc::new(wall);
        true
    }
}

impl PlayClock for SwitchClock {
    fn kind(&self) -> &'static str {
        self.current().kind()
    }

    fn seek(&self, to: f64) {
        self.running.store(false, Ordering::SeqCst);
        self.current().seek(to);
    }

    fn run(&self) {
        self.running.store(true, Ordering::SeqCst);
        self.current().run();
    }

    fn halt(&self) -> f64 {
        self.running.store(false, Ordering::SeqCst);
        self.current().halt()
    }

    fn position(&self) -> f64 {
        self.current().position()
    }

    fn underruns(&self) -> u64 {
        self.current().underruns()
    }

    fn failed(&self) -> bool {
        self.current().failed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 位置を手で決める時計。`failed` を立てると壊れた音の時計になる。
    #[derive(Default)]
    struct Stuck {
        at: Mutex<f64>,
        broken: AtomicBool,
    }

    impl PlayClock for Stuck {
        fn kind(&self) -> &'static str {
            "audio"
        }
        fn seek(&self, to: f64) {
            *self.at.lock().unwrap() = to;
        }
        fn run(&self) {}
        fn halt(&self) -> f64 {
            *self.at.lock().unwrap()
        }
        fn position(&self) -> f64 {
            *self.at.lock().unwrap()
        }
        fn failed(&self) -> bool {
            self.broken.load(Ordering::SeqCst)
        }
    }

    #[test]
    fn 音の時計が壊れたら再生中なら同じ位置から壁時計で進む() {
        let stuck = Arc::new(Stuck::default());
        let clock = SwitchClock::new(stuck.clone());
        clock.seek(4.0);
        clock.run();
        assert!(!clock.fall_back_if_failed(), "壊れていなければ替えない");
        stuck.broken.store(true, Ordering::SeqCst);
        assert!(clock.fall_back_if_failed());
        assert_eq!(clock.kind(), "wall");
        assert!(!clock.failed());
        let a = clock.position();
        assert!((4.0..4.05).contains(&a), "止まった位置から続く: {a}");
        std::thread::sleep(Duration::from_millis(40));
        assert!(clock.position() > a + 0.03, "壁時計が進める");
        assert!(!clock.fall_back_if_failed(), "替えた後は 2 度替えない");
    }

    #[test]
    fn 止めている間に壊れたら替えても止めたまま() {
        let stuck = Arc::new(Stuck::default());
        let clock = SwitchClock::new(stuck.clone());
        clock.seek(2.0);
        stuck.broken.store(true, Ordering::SeqCst);
        assert!(clock.fall_back_if_failed());
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(clock.position(), 2.0);
        clock.run();
        std::thread::sleep(Duration::from_millis(30));
        assert!(clock.position() > 2.02, "再生を始めれば進む");
    }

    #[test]
    fn 壁時計は止めている間は進まず_走らせると進む() {
        let c = WallClock::default();
        c.seek(3.0);
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(c.position(), 3.0);
        c.run();
        std::thread::sleep(Duration::from_millis(30));
        let moved = c.halt();
        assert!(moved > 3.02, "{moved}");
        std::thread::sleep(Duration::from_millis(20));
        assert_eq!(c.position(), moved);
    }
}
