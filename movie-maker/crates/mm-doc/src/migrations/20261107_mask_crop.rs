//! 切り抜きとマスクとフェード。レイヤーの切り抜き（`Layer.crop`）、角丸の矩形のマスク（`Mask.rect`。`path`、
//! `feather`、`opacity` は省ける）、音のフェード（`LayerAudio.fade_in`、`fade_out`）。

use serde_json::Value;

/// 足したフィールドは serde の既定（切り抜きなし、矩形なし、フェードなし）で埋まるので、JSON は書き換えない。
pub(crate) fn up(_value: &mut Value) {}

#[cfg(test)]
mod tests {
    use crate::{load, FadeCurve};

    fn read(path: &str) -> crate::Project {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        load(&std::fs::read_to_string(dir.join(path)).unwrap()).unwrap()
    }

    #[test]
    fn 凍結した切り抜きとマスクとフェードが読めて往復する() {
        let p = read("20261107_mask_crop/mask_crop.json");
        let layer = &p.compositions[0].layers[0];
        assert!(layer.crop.left.is_some() && layer.crop.right.is_none());
        assert_eq!(layer.crop.top.as_ref().unwrap().keyframes.len(), 2);
        let mask = &layer.masks[0];
        assert!(mask.rect.is_some());
        assert_eq!(mask.opacity.value.as_number(), Some(1.0), "省いた不透明度は 1");
        let audio = layer.audio.as_ref().unwrap();
        assert_eq!(audio.fade_in.as_ref().unwrap().curve, FadeCurve::EqualPower);
        assert_eq!(
            audio.fade_out.as_ref().unwrap().curve,
            FadeCurve::Linear,
            "省いた曲線は振幅の直線"
        );
        assert_eq!(load(&serde_json::to_string(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn 移行の前のプロジェクトのデータは切り抜きもフェードもなしで読める() {
        let p = read("20261107_mixer/mixer.json");
        let layers = &p.compositions[0].layers;
        assert!(layers.iter().all(|l| l.crop.is_empty()));
        assert!(layers
            .iter()
            .filter_map(|l| l.audio.as_ref())
            .all(|a| a.fade_in.is_none() && a.fade_out.is_none()));
        assert!(p.migrations.iter().any(|m| m == "20261107_mask_crop"));
    }
}
