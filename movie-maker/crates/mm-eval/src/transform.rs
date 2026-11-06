//! レイヤーの変形と音の設定を時刻 t で評価する。レンダラーとモックのレンダラーが同じ値を読む。

use mm_doc::{Layer, Transform};
use mm_types::{ParamValue, RationalTime};

use crate::param::eval_param;

/// 時刻 t で評価した変形。単位は `mm_doc::Transform` のまま（コンポジションの画素と度）。
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedTransform {
    pub position: [f64; 2],
    pub scale: [f64; 2],
    pub rotation_deg: f64,
    /// 回転と拡大の中心。`None` はレイヤーの画像の中心。
    pub anchor: Option<[f64; 2]>,
}

impl Default for EvaluatedTransform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
            rotation_deg: 0.0,
            anchor: None,
        }
    }
}

/// プロジェクトのデータの変形を時刻 `t` で評価する。型が合わないパラメータは既定の値。
pub fn evaluate_transform(doc: &Transform, t: RationalTime) -> EvaluatedTransform {
    let mut out = EvaluatedTransform::default();
    if let Some(v) = eval_param(&doc.position, t).as_vec2() {
        out.position = v;
    }
    match eval_param(&doc.scale, t) {
        ParamValue::Number(n) => out.scale = [n, n],
        ParamValue::Vec2(v) => out.scale = v,
        _ => {}
    }
    if let Some(n) = eval_param(&doc.rotation, t).as_number() {
        out.rotation_deg = n;
    }
    out.anchor = doc.anchor.as_ref().and_then(|a| eval_param(a, t).as_vec2());
    out
}

/// 時刻 t で評価したレイヤーの音の設定。
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatedAudio<'a> {
    pub role: &'a str,
    pub gain_db: f64,
    pub pan: f64,
}

/// 音の設定が無いときの役割。
pub const DEFAULT_AUDIO_ROLE: &str = "voice";

/// レイヤーの音の設定（役割、dB、パン）を時刻 `t` で評価する。設定が無ければパススルー。
pub fn evaluate_audio(layer: &Layer, t: RationalTime) -> EvaluatedAudio<'_> {
    let Some(a) = &layer.audio else {
        return EvaluatedAudio {
            role: DEFAULT_AUDIO_ROLE,
            gain_db: 0.0,
            pan: 0.0,
        };
    };
    let number = |p| eval_param(p, t).as_number().unwrap_or(0.0);
    EvaluatedAudio {
        role: &a.role,
        gain_db: number(&a.gain_db),
        pan: number(&a.pan),
    }
}

/// レイヤーの不透明度を時刻 `t` で評価する。型が合わなければ 1。
pub fn evaluate_opacity(layer: &Layer, t: RationalTime) -> f64 {
    eval_param(&layer.opacity, t).as_number().unwrap_or(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Interpolation, Keyframe, Param};

    #[test]
    fn 変形を読む() {
        let doc = Transform {
            scale: Param::constant(ParamValue::Number(2.0)),
            rotation: Param::constant(ParamValue::Number(30.0)),
            ..Transform::default()
        };
        let t = evaluate_transform(&doc, RationalTime::ZERO);
        assert_eq!(t.scale, [2.0, 2.0]);
        assert_eq!(t.rotation_deg, 30.0);
        assert!(t.anchor.is_none());
    }

    #[test]
    fn 位置のキーフレームを補間する() {
        let doc = Transform {
            position: Param {
                value: ParamValue::Vec2([0.0, 0.0]),
                keyframes: vec![
                    Keyframe::new(
                        RationalTime::ZERO,
                        ParamValue::Vec2([0.0, 0.0]),
                        Interpolation::Linear,
                    ),
                    Keyframe::new(
                        RationalTime::seconds(2),
                        ParamValue::Vec2([10.0, 20.0]),
                        Interpolation::Linear,
                    ),
                ],
            },
            ..Transform::default()
        };
        let t = evaluate_transform(&doc, RationalTime::seconds(1));
        assert_eq!(t.position, [5.0, 10.0]);
    }
}
