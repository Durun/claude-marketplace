//! 出力のリングバッファと再生位置の時計。デバイスに依存しない純粋な状態で、時刻は呼び出し側が渡す。
//!
//! 時計は「いま耳に届いている音の位置」を返す。コールバックごとに（そのバッファが鳴り始める時刻、
//! その時点までに消費したフレーム数）をアンカーにして、次のコールバックまで一定の速さで外挿する。
//! 外挿は消費済みのフレーム数を超えない（コールバックが遅れても、音が尽きた先へは進まない）。

use std::collections::VecDeque;

/// 時計のアンカー。コールバック 1 回分。
#[derive(Debug, Clone, Copy, PartialEq)]
struct Anchor {
    /// このコールバックのバッファが鳴り始める時刻（ストリームの基準からの秒）。
    plays_at: f64,
    /// このバッファの直前までに消費したフレーム数。
    frames_before: u64,
    /// このバッファの末尾までに消費したフレーム数。
    frames_after: u64,
}

/// リングバッファと時計。
#[derive(Debug)]
pub struct Ring {
    channels: usize,
    sample_rate: f64,
    samples: VecDeque<f32>,
    /// `reset` からの消費フレーム数。
    consumed: u64,
    /// 再生位置の起点（秒）。
    start_secs: f64,
    anchor: Option<Anchor>,
    /// 止めた時点の位置（秒）。止めている間はこれを返す。
    held: Option<f64>,
    underruns: u64,
}

impl Ring {
    pub fn new(sample_rate: u32, channels: u16) -> Self {
        Self {
            channels: usize::from(channels.max(1)),
            sample_rate: f64::from(sample_rate.max(1)),
            samples: VecDeque::new(),
            consumed: 0,
            start_secs: 0.0,
            anchor: None,
            held: None,
            underruns: 0,
        }
    }

    /// 溜めた音を捨て、再生位置を `start_secs` に置き直す。
    pub fn reset(&mut self, start_secs: f64) {
        self.samples.clear();
        self.consumed = 0;
        self.start_secs = start_secs;
        self.anchor = None;
        self.held = None;
    }

    /// 時刻 `now`（秒）で止め、止めた位置（秒）を返す。止めている間の `position` はこの値で動かない。
    /// 止めた後のコールバックがアンカーを捨てても、位置がデバイスの遅れとバッファの分だけ先へ跳ばない。
    pub fn halt(&mut self, now: f64) -> f64 {
        let at = self.position(now);
        self.held = Some(at);
        at
    }

    /// 止めた位置の固定を外す。デバイスに渡した音は戻せないので、再開の前に `reset` で置き直す。
    pub fn resume(&mut self) {
        self.held = None;
    }

    /// インターリーブのサンプルを後ろに足す。
    pub fn push(&mut self, samples: &[f32]) {
        self.samples.extend(samples.iter().copied());
    }

    /// 溜まっているフレーム数。
    pub fn buffered_frames(&self) -> usize {
        self.samples.len() / self.channels
    }

    /// 音が尽きた回数（鳴らすものが無いのにコールバックが来た回数）。
    pub fn underruns(&self) -> u64 {
        self.underruns
    }

    /// コールバック 1 回分を `out`（インターリーブ）に満たす。足りない分は無音。
    /// `plays_at` はこのバッファが鳴り始める時刻（秒）。`running` が偽なら消費せず無音にする。
    pub fn fill(&mut self, out: &mut [f32], plays_at: f64, running: bool) {
        out.fill(0.0);
        if !running {
            self.anchor = None;
            return;
        }
        let want = out.len() / self.channels;
        let have = self.buffered_frames().min(want);
        for (dst, src) in out
            .iter_mut()
            .zip(self.samples.drain(..have * self.channels))
        {
            *dst = src;
        }
        if have < want {
            self.underruns += 1;
        }
        let before = self.consumed;
        self.consumed += have as u64;
        // 音を消費しなかった呼び出しではアンカーを替えない。無音のバッファの鳴る時刻で替えると、
        // 位置がデバイスの遅れの分だけ手前に戻り、最後の音まで届かなくなる。
        if have > 0 {
            self.anchor = Some(Anchor {
                plays_at,
                frames_before: before,
                frames_after: self.consumed,
            });
        }
    }

    /// 時刻 `now`（秒）に耳に届いている再生位置（秒）。まだ鳴っていなければ起点。
    pub fn position(&self, now: f64) -> f64 {
        if let Some(held) = self.held {
            return held;
        }
        let frames = match self.anchor {
            None => self.consumed as f64,
            Some(a) => {
                let extrapolated = a.frames_before as f64 + (now - a.plays_at) * self.sample_rate;
                extrapolated.clamp(0.0, a.frames_after as f64)
            }
        };
        self.start_secs + frames / self.sample_rate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ring_with(seconds: f64) -> Ring {
        let mut r = Ring::new(1000, 2);
        r.reset(10.0);
        r.push(&vec![0.5; (seconds * 1000.0) as usize * 2]);
        r
    }

    #[test]
    fn 鳴る前は起点を返す() {
        let r = ring_with(1.0);
        assert_eq!(r.position(0.0), 10.0);
    }

    #[test]
    fn コールバックの間は一定の速さで外挿する() {
        let mut r = ring_with(1.0);
        let mut out = vec![0.0; 100 * 2];
        // 0.100 秒のバッファを 0.020 秒後に鳴らす。
        r.fill(&mut out, 0.020, true);
        assert!((r.position(0.020) - 10.0).abs() < 1e-9, "鳴り始めは起点");
        assert!((r.position(0.070) - 10.05).abs() < 1e-9);
        assert!(
            (r.position(0.120) - 10.1).abs() < 1e-9,
            "バッファを鳴らし終えた所"
        );
    }

    #[test]
    fn 次のコールバックが遅れても消費した先へは進まない() {
        let mut r = ring_with(1.0);
        let mut out = vec![0.0; 100 * 2];
        r.fill(&mut out, 0.0, true);
        assert!((r.position(5.0) - 10.1).abs() < 1e-9);
    }

    #[test]
    fn 音が尽きたら数え_位置は鳴らした分で止まる() {
        let mut r = ring_with(0.05);
        let mut out = vec![1.0; 100 * 2];
        r.fill(&mut out, 0.0, true);
        assert_eq!(r.underruns(), 1);
        assert!((r.position(1.0) - 10.05).abs() < 1e-9);
        assert!(out[100..].iter().all(|s| *s == 0.0), "足りない分は無音");
        assert!(out[..100].iter().all(|s| *s == 0.5));
    }

    #[test]
    fn 音を使い切った後の無音のコールバックで位置が戻らない() {
        let mut r = ring_with(0.1);
        let mut out = vec![0.0; 100 * 2];
        r.fill(&mut out, 0.5, true);
        // 遅れ 40 ms のデバイスが、使い切った後も無音のバッファを先に渡し続ける。
        for i in 1..20 {
            r.fill(&mut out, 0.5 + 0.01 * f64::from(i), true);
        }
        assert!((r.position(1.0) - 10.1).abs() < 1e-9, "{}", r.position(1.0));
    }

    #[test]
    fn 止めている間は消費せず無音() {
        let mut r = ring_with(1.0);
        let mut out = vec![1.0; 20];
        r.fill(&mut out, 0.0, false);
        assert!(out.iter().all(|s| *s == 0.0));
        assert_eq!(r.buffered_frames(), 1000);
    }

    #[test]
    fn 止めた位置は後続のコールバックで跳ばない() {
        let mut r = ring_with(1.0);
        let mut out = vec![0.0; 100 * 2];
        // 0.100 秒のバッファが 0.030 秒に鳴り始め、0.050 秒に止める。
        r.fill(&mut out, 0.030, true);
        let held = r.halt(0.050);
        assert!((held - 10.02).abs() < 1e-9, "{held}");
        // 止めた後の最初のコールバックはアンカーを捨てるが、位置は止めた所のまま。
        r.fill(&mut out, 0.130, false);
        r.fill(&mut out, 0.230, false);
        assert_eq!(r.position(0.230), held);
        assert_eq!(r.position(9.0), held);
    }

    #[test]
    fn 止めるのは何度でも同じ位置を返し_リセットで固定が外れる() {
        let mut r = ring_with(1.0);
        let mut out = vec![0.0; 100 * 2];
        r.fill(&mut out, 0.0, true);
        let a = r.halt(0.05);
        assert_eq!(r.halt(0.09), a);
        r.reset(3.0);
        assert_eq!(r.position(0.5), 3.0);
        r.halt(0.5);
        r.resume();
        assert_eq!(r.position(0.5), 3.0);
    }

    #[test]
    fn リセットで位置と溜めた音を置き直す() {
        let mut r = ring_with(1.0);
        let mut out = vec![0.0; 40];
        r.fill(&mut out, 0.0, true);
        r.reset(3.0);
        assert_eq!(r.buffered_frames(), 0);
        assert_eq!(r.position(0.5), 3.0);
    }
}
