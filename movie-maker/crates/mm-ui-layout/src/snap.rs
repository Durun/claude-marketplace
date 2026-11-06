//! スナップ。ドラッグ中の時刻を、近い目標（再生位置、ほかのレイヤーの端、マーカー）に吸い付ける。

/// 吸い付く距離（px）。
pub const SNAP_PX: f64 = 8.0;

/// スナップの結果。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Snapped {
    /// 吸い付いた後の時刻。
    pub time: f64,
    /// 吸い付いた目標。どれにも近くなければ `None`。
    pub target: Option<f64>,
}

/// `time` を `targets` のうち `threshold_px` 以内で一番近いものへ寄せる。
pub fn snap(time: f64, targets: &[f64], px_per_sec: f64, threshold_px: f64) -> Snapped {
    let limit = threshold_px / px_per_sec;
    let best = targets
        .iter()
        .copied()
        .filter(|t| (t - time).abs() <= limit)
        .min_by(|a, b| (a - time).abs().total_cmp(&(b - time).abs()));
    match best {
        Some(t) => Snapped {
            time: t,
            target: Some(t),
        },
        None => Snapped { time, target: None },
    }
}

/// 時刻を `fps` のフレームの境目に丸める。
pub fn quantize(time: f64, fps: f64) -> f64 {
    if fps <= 0.0 {
        return time;
    }
    (time * fps).round() / fps
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 近い目標へ吸い付く() {
        // 100 px/s で 8 px は 0.08 秒。
        let s = snap(2.05, &[0.0, 2.0, 5.0], 100.0, SNAP_PX);
        assert_eq!(s.time, 2.0);
        assert_eq!(s.target, Some(2.0));
    }

    #[test]
    fn 遠ければそのまま() {
        let s = snap(3.5, &[0.0, 2.0, 5.0], 100.0, SNAP_PX);
        assert_eq!((s.time, s.target), (3.5, None));
    }

    #[test]
    fn 二つの目標の間では近い方を選ぶ() {
        let s = snap(1.04, &[1.0, 1.06], 100.0, SNAP_PX);
        assert_eq!(s.time, 1.06);
    }

    #[test]
    fn フレームの境目に丸める() {
        assert!((quantize(0.51, 30.0) - 15.0 / 30.0).abs() < 1e-12);
        assert_eq!(quantize(1.23, 0.0), 1.23);
    }
}
