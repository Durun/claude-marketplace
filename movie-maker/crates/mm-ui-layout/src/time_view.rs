//! Timeline の表示の範囲。時刻（秒）と画面の x の変換、拡大縮小、移動。

/// 1 秒あたりの最小のピクセル数。
pub const MIN_PX_PER_SEC: f64 = 2.0;
/// 1 秒あたりの最大のピクセル数。60 fps の 1 フレームが 40 px 以上になる。
pub const MAX_PX_PER_SEC: f64 = 2400.0;

/// 表示の範囲。左端の時刻と倍率で決まる。x は Timeline の矩形の左端を 0 とする。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimeView {
    /// 左端の時刻（秒）。
    pub origin: f64,
    /// 1 秒のピクセル数。
    pub px_per_sec: f64,
}

impl TimeView {
    /// `duration` 秒が `width` px にちょうど収まる表示（左右に `margin` px の余白）。
    pub fn fit(duration: f64, width: f32, margin: f32) -> Self {
        let usable = (f64::from(width) - 2.0 * f64::from(margin)).max(1.0);
        let px = (usable / duration.max(1e-6)).clamp(MIN_PX_PER_SEC, MAX_PX_PER_SEC);
        Self {
            origin: -f64::from(margin) / px,
            px_per_sec: px,
        }
    }

    /// 時刻 → x。
    pub fn x_of(&self, t: f64) -> f32 {
        ((t - self.origin) * self.px_per_sec) as f32
    }

    /// x → 時刻。
    pub fn t_of(&self, x: f32) -> f64 {
        f64::from(x) / self.px_per_sec + self.origin
    }

    /// 幅 `width` px で見える時刻の範囲（始め、終わり）。
    pub fn visible(&self, width: f32) -> (f64, f64) {
        (self.origin, self.t_of(width))
    }

    /// `anchor_x` の下の時刻を動かさずに `factor` 倍へ拡大する。倍率は範囲に収める。
    pub fn zoom_at(&self, anchor_x: f32, factor: f64) -> Self {
        let px = (self.px_per_sec * factor).clamp(MIN_PX_PER_SEC, MAX_PX_PER_SEC);
        let anchor_t = self.t_of(anchor_x);
        Self {
            origin: anchor_t - f64::from(anchor_x) / px,
            px_per_sec: px,
        }
    }

    /// 右へ `dx` px 動かした表示（つまみを右へ引くと内容も右へ動く）。
    pub fn pan_px(&self, dx: f32) -> Self {
        Self {
            origin: self.origin - f64::from(dx) / self.px_per_sec,
            ..*self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn x_と時刻は往復する() {
        let v = TimeView {
            origin: 1.5,
            px_per_sec: 80.0,
        };
        for t in [0.0, 1.5, 7.25] {
            assert!((v.t_of(v.x_of(t)) - t).abs() < 1e-4);
        }
    }

    #[test]
    fn 全体表示は余白つきで幅に収まる() {
        let v = TimeView::fit(10.0, 1000.0, 20.0);
        assert!((v.x_of(0.0) - 20.0).abs() < 1e-3);
        assert!((v.x_of(10.0) - 980.0).abs() < 1e-3);
    }

    #[test]
    fn 拡大はカーソルの下の時刻を動かさない() {
        let v = TimeView::fit(10.0, 1000.0, 0.0);
        let t = v.t_of(300.0);
        let z = v.zoom_at(300.0, 4.0);
        assert!((z.t_of(300.0) - t).abs() < 1e-6);
        assert!((z.px_per_sec - v.px_per_sec * 4.0).abs() < 1e-9);
    }

    #[test]
    fn 倍率は上下限に収まる() {
        let v = TimeView::fit(10.0, 1000.0, 0.0);
        assert_eq!(v.zoom_at(0.0, 1e9).px_per_sec, MAX_PX_PER_SEC);
        assert_eq!(v.zoom_at(0.0, 1e-9).px_per_sec, MIN_PX_PER_SEC);
    }

    #[test]
    fn 右へ引くと内容が右へ動く() {
        let v = TimeView::fit(10.0, 1000.0, 0.0);
        let p = v.pan_px(50.0);
        assert!((p.x_of(0.0) - (v.x_of(0.0) + 50.0)).abs() < 1e-3);
    }
}
