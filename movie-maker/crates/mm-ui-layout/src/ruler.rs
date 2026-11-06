//! 目盛り。倍率に応じて、ラベルが重ならない切りのよい間隔を選ぶ。

use crate::time_view::TimeView;

/// 目盛りの間隔の候補（秒）。
const STEPS: [f64; 16] = [
    0.05, 0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
];
/// 主な目盛りの間の最小のピクセル数（ラベルが入る幅）。
const MIN_MAJOR_PX: f64 = 72.0;
/// 主な目盛りの間の細かい目盛りの数。
const MINOR_PER_MAJOR: u32 = 5;

/// 目盛り 1 つ。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tick {
    pub time: f64,
    pub x: f32,
    /// 主な目盛り（ラベルが付く）か。
    pub major: bool,
}

/// 主な目盛りの間隔（秒）。ラベルが重ならない最小の候補。
pub fn major_step(px_per_sec: f64) -> f64 {
    STEPS
        .iter()
        .copied()
        .find(|s| s * px_per_sec >= MIN_MAJOR_PX)
        .unwrap_or(STEPS[STEPS.len() - 1])
}

/// 幅 `width` px に見える範囲の目盛り。
pub fn ticks(view: &TimeView, width: f32) -> Vec<Tick> {
    let major = major_step(view.px_per_sec);
    let minor = major / f64::from(MINOR_PER_MAJOR);
    let (start, end) = view.visible(width);
    let first = (start.max(0.0) / minor).floor() as i64;
    let last = (end / minor).ceil() as i64;
    (first..=last)
        .map(|i| {
            let time = i as f64 * minor;
            Tick {
                time,
                x: view.x_of(time),
                major: i % i64::from(MINOR_PER_MAJOR) == 0,
            }
        })
        .collect()
}

/// 時刻のラベル（`m:ss`。1 秒未満の間隔なら `m:ss.d`）。
pub fn label(time: f64, step: f64) -> String {
    let total = time.max(0.0);
    let mins = (total / 60.0).floor() as u64;
    if step < 1.0 {
        format!("{mins}:{:04.1}", total - mins as f64 * 60.0)
    } else {
        format!("{mins}:{:02}", (total - mins as f64 * 60.0).floor() as u64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 倍率が上がると間隔が細かくなる() {
        assert_eq!(major_step(100.0), 1.0);
        assert_eq!(major_step(1000.0), 0.1);
        assert_eq!(major_step(10.0), 10.0);
    }

    #[test]
    fn 隣の主な目盛りはラベルの幅以上に離れる() {
        for px in [2.0, 7.0, 33.0, 120.0, 900.0, 2400.0] {
            let step = major_step(px);
            assert!(step * px >= MIN_MAJOR_PX || step == STEPS[STEPS.len() - 1]);
        }
    }

    #[test]
    fn 見える範囲の目盛りだけ返し_0_秒より前は出さない() {
        let v = TimeView {
            origin: -1.0,
            px_per_sec: 100.0,
        };
        let t = ticks(&v, 500.0);
        assert!(t.iter().all(|k| k.time >= 0.0));
        assert!(t.iter().any(|k| k.major && k.time == 0.0));
        assert!(t.iter().all(|k| k.x >= -1.0 && k.x <= 600.0));
    }

    #[test]
    fn ラベルは分と秒() {
        assert_eq!(label(65.0, 5.0), "1:05");
        assert_eq!(label(2.5, 0.5), "0:02.5");
    }
}
