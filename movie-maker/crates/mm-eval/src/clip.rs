//! レイヤーの見える範囲（切り抜き、角丸の矩形のマスク）と音のフェードを時刻 t で評価する。
//! レンダラーとモックのレンダラーが同じ値を読む。

use mm_doc::{Crop, Fade, FadeCurve, Layer, Mask, MaskMode, Param};
use mm_types::RationalTime;

use crate::param::eval_param;

/// dB の直線のフェードの始まりの音量（dB）。ここから 0 dB まで直線で上げる。
pub const FADE_DB_FLOOR: f64 = -60.0;

/// 時刻 t で評価した切り抜き。レイヤーの画像の縁から内側へ削る幅（画素）。負の値は 0 にする。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EvaluatedCrop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl EvaluatedCrop {
    /// どの辺も削らないか。
    pub fn is_none(&self) -> bool {
        *self == Self::default()
    }
}

fn number(p: Option<&Param>, t: RationalTime) -> f64 {
    p.and_then(|p| eval_param(p, t).as_number())
        .unwrap_or(0.0)
        .max(0.0)
}

/// 切り抜きを時刻 `t` で評価する。
pub fn evaluate_crop(crop: &Crop, t: RationalTime) -> EvaluatedCrop {
    EvaluatedCrop {
        left: number(crop.left.as_ref(), t),
        top: number(crop.top.as_ref(), t),
        right: number(crop.right.as_ref(), t),
        bottom: number(crop.bottom.as_ref(), t),
    }
}

/// 時刻 t で評価した角丸の矩形のマスク。座標はレイヤーの座標（画素）。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EvaluatedMask {
    /// 矩形の左上と右下。
    pub min: [f64; 2],
    pub max: [f64; 2],
    /// 角の半径。短い辺の半分で頭打ち。
    pub radius: f64,
    /// ぼかしの幅。
    pub feather: f64,
    /// 0..1。
    pub opacity: f64,
    pub mode: MaskMode,
    pub inverted: bool,
}

/// 角丸の矩形のマスクを時刻 `t` で評価する。形が頂点の列（`rect` が無い）なら `None`（描かない）。
pub fn evaluate_mask(mask: &Mask, t: RationalTime) -> Option<EvaluatedMask> {
    let rect = mask.rect.as_ref()?;
    let position = eval_param(&rect.position, t)
        .as_vec2()
        .unwrap_or([0.0, 0.0]);
    let size = eval_param(&rect.size, t)
        .as_vec2()
        .unwrap_or([0.0, 0.0])
        .map(|v| v.max(0.0));
    let half = size[0].min(size[1]) / 2.0;
    Some(EvaluatedMask {
        min: position,
        max: [position[0] + size[0], position[1] + size[1]],
        radius: number(Some(&rect.radius), t).min(half),
        feather: number(Some(&mask.feather), t),
        opacity: eval_param(&mask.opacity, t)
            .as_number()
            .unwrap_or(1.0)
            .clamp(0.0, 1.0),
        mode: mask.mode,
        inverted: mask.inverted,
    })
}

/// フェードの曲線で、進み具合 `p`（0..1。無音の側が 0）の振幅の倍率。
pub fn fade_curve_gain(curve: FadeCurve, p: f64) -> f64 {
    let p = p.clamp(0.0, 1.0);
    match curve {
        FadeCurve::Linear => p,
        FadeCurve::EqualPower => (p * std::f64::consts::FRAC_PI_2).sin(),
        FadeCurve::Db if p <= 0.0 => 0.0,
        FadeCurve::Db => 10f64.powf(FADE_DB_FLOOR * (1.0 - p) / 20.0),
    }
}

/// レイヤーのフェードがあるか。
pub fn has_fades(layer: &Layer) -> bool {
    layer
        .audio
        .as_ref()
        .is_some_and(|a| a.fade_in.is_some() || a.fade_out.is_some())
}

/// レイヤーの先頭と末尾のフェードの、コンポジションの時刻 `t` での振幅の倍率。重なれば小さい方。
pub fn fade_gain(layer: &Layer, t: RationalTime) -> f64 {
    let Some(audio) = &layer.audio else {
        return 1.0;
    };
    let Ok(range) = layer.range() else {
        return 1.0;
    };
    let (from_start, to_end) = (
        (t.to_f64() - range.start.to_f64()),
        (range.end.to_f64() - t.to_f64()),
    );
    let side = |fade: &Option<Fade>, elapsed: f64| {
        fade.as_ref().map_or(1.0, |f| {
            let len = f.duration.to_f64();
            let p = if len > 0.0 { elapsed / len } else { 1.0 };
            fade_curve_gain(f.curve, p)
        })
    };
    side(&audio.fade_in, from_start).min(side(&audio.fade_out, to_end))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{LayerAudio, MaskRect, Node};
    use mm_types::ParamValue;

    fn secs(n: i64) -> RationalTime {
        RationalTime::seconds(n)
    }

    fn layer_with(fade_in: Option<Fade>, fade_out: Option<Fade>) -> Layer {
        let mut l = Layer::new("a", Node::new("solid", 1), secs(10), secs(0), secs(4));
        l.audio = Some(LayerAudio {
            fade_in,
            fade_out,
            ..LayerAudio::new("voice")
        });
        l
    }

    #[test]
    fn 振幅の直線のフェードは先頭と末尾で直線に上がり下がる() {
        let l = layer_with(Some(Fade::linear(secs(2))), Some(Fade::linear(secs(1))));
        let at = |n: i128, d: u32| fade_gain(&l, RationalTime::new(n, d).unwrap());
        assert_eq!(at(10, 1), 0.0);
        assert!((at(11, 1) - 0.5).abs() < 1e-12);
        assert_eq!(at(12, 1), 1.0);
        assert!((at(27, 2) - 0.5).abs() < 1e-12, "末尾の 0.5 秒前");
        assert_eq!(at(14, 1), 0.0);
    }

    #[test]
    fn 等パワーと_db_の直線は中ほどで振幅の直線と違う() {
        assert!((fade_curve_gain(FadeCurve::EqualPower, 0.5) - 0.5f64.sqrt()).abs() < 1e-12);
        assert!((fade_curve_gain(FadeCurve::Db, 0.5) - 10f64.powf(-1.5)).abs() < 1e-12);
        assert_eq!(fade_curve_gain(FadeCurve::Db, 0.0), 0.0);
        assert_eq!(fade_curve_gain(FadeCurve::Db, 1.0), 1.0);
    }

    #[test]
    fn フェードの無いレイヤーは倍率_1() {
        let l = layer_with(None, None);
        assert!(!has_fades(&l));
        assert_eq!(fade_gain(&l, secs(10)), 1.0);
    }

    #[test]
    fn 矩形のマスクの半径は短い辺の半分で頭打ち() {
        let m = Mask::rect("m", MaskRect::new([10.0, 20.0], [100.0, 40.0], 50.0));
        let e = evaluate_mask(&m, secs(0)).unwrap();
        assert_eq!((e.min, e.max), ([10.0, 20.0], [110.0, 60.0]));
        assert_eq!(e.radius, 20.0);
        assert_eq!(e.opacity, 1.0);
    }

    #[test]
    fn 切り抜きの無い辺は_0_で負の値も_0() {
        let crop = Crop {
            left: Some(Param::constant(ParamValue::Number(12.0))),
            right: Some(Param::constant(ParamValue::Number(-3.0))),
            ..Crop::default()
        };
        let e = evaluate_crop(&crop, secs(0));
        assert_eq!((e.left, e.top, e.right, e.bottom), (12.0, 0.0, 0.0, 0.0));
    }
}
