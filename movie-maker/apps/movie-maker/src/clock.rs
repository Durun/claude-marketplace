//! 再生の主の時計。音の出力があればその位置（耳に届いている位置）を使い、無ければ壁時計で進める。

use std::sync::{Arc, Mutex};
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
}

/// 音の出力の位置を時計にする。音は `Player` が `AudioOutput` に足す。
pub struct AudioClock(pub Arc<AudioOutput>);

impl PlayClock for AudioClock {
    fn kind(&self) -> &'static str {
        "audio"
    }

    fn seek(&self, to: f64) {
        self.0.set_running(false);
        self.0.reset(to);
    }

    fn run(&self) {
        self.0.set_running(true);
    }

    fn halt(&self) -> f64 {
        self.0.set_running(false);
        self.0.position_secs()
    }

    fn position(&self) -> f64 {
        self.0.position_secs()
    }

    fn underruns(&self) -> u64 {
        self.0.underruns()
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

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
