//! レイヤーの見える範囲（切り抜きと角丸の矩形のマスク）を、合成のシェーダーに渡す形にする。
//! 効果を掛けた後の画像に、変形と一緒に合成の 1 パスで掛ける（別のテクスチャを作らない）。

use bytemuck::{Pod, Zeroable};

use mm_doc::{Layer, MaskMode};
use mm_eval::{evaluate_crop, evaluate_mask, EvaluatedMask};
use mm_types::{RationalTime, Rect};

/// シェーダーが一度に描くマスクの数の上限。超えた分（と頂点の列のマスク）は描かない。
pub(crate) const MAX_MASKS: usize = 4;
/// 削らない辺の座標。辺ごとの被覆率が 1 になる遠さで、f32 の桁落ちが起きないよう差を取らない形で使う。
const FAR: f32 = 1.0e9;
/// シェーダーの `flags` の反転のビット。下位の 2 ビットが組み合わせ方。
const FLAG_INVERTED: u32 = 4;

/// シェーダーの `MaskItem` と同じ並び（32 バイト）。
#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
pub(crate) struct MaskUniform {
    /// 左上と右下（レイヤーの座標）。
    rect: [f32; 4],
    radius: f32,
    feather: f32,
    opacity: f32,
    flags: u32,
}

/// 1 枚のレイヤーの見える範囲。
#[derive(Clone, Copy)]
pub(crate) struct Clip {
    /// 画像の置き場所 `[x, y, w, h]`（レイヤーの座標）。UV からレイヤーの座標を戻す。
    pub place: [f32; 4],
    /// 切り抜いた後の矩形の左上と右下。削らない辺は遠く。
    pub crop: [f32; 4],
    pub masks: [MaskUniform; MAX_MASKS],
    pub mask_count: u32,
}

impl Clip {
    /// 何も削らない。
    pub(crate) fn none(place: Rect) -> Self {
        Self {
            place: [place.x, place.y, place.width, place.height],
            crop: [-FAR, -FAR, FAR, FAR],
            masks: [MaskUniform::default(); MAX_MASKS],
            mask_count: 0,
        }
    }

    /// レイヤーの切り抜きとマスクを時刻 `t` で評価する。`place` は効果を掛けた後の画像の置き場所。
    pub(crate) fn of(layer: &Layer, t: RationalTime, place: Rect) -> Self {
        let mut clip = Self::none(place);
        let c = evaluate_crop(&layer.crop, t);
        let side = |v: f64, edge: f32, dir: f32| {
            if v > 0.0 {
                edge + dir * v as f32
            } else {
                edge - dir * FAR
            }
        };
        clip.crop = [
            side(c.left, place.x, 1.0),
            side(c.top, place.y, 1.0),
            side(c.right, place.x + place.width, -1.0),
            side(c.bottom, place.y + place.height, -1.0),
        ];
        let masks = layer
            .masks
            .iter()
            .filter_map(|m| evaluate_mask(m, t))
            .take(MAX_MASKS);
        for (slot, m) in clip.masks.iter_mut().zip(masks) {
            *slot = mask_uniform(&m);
            clip.mask_count += 1;
        }
        clip
    }
}

fn mask_uniform(m: &EvaluatedMask) -> MaskUniform {
    let mode = match m.mode {
        MaskMode::Add => 0,
        MaskMode::Subtract => 1,
        MaskMode::Intersect => 2,
    };
    MaskUniform {
        rect: [m.min[0], m.min[1], m.max[0], m.max[1]].map(|v| v as f32),
        radius: m.radius as f32,
        feather: m.feather as f32,
        opacity: m.opacity as f32,
        flags: mode | if m.inverted { FLAG_INVERTED } else { 0 },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Mask, MaskRect, Node, Param};
    use mm_types::ParamValue;

    fn layer() -> Layer {
        Layer::new(
            "a",
            Node::new("solid", 1),
            RationalTime::ZERO,
            RationalTime::ZERO,
            RationalTime::seconds(1),
        )
    }

    #[test]
    fn 削る辺だけ置き場所の縁から内へ寄せる() {
        let mut l = layer();
        l.crop.left = Some(Param::constant(ParamValue::Number(10.0)));
        l.crop.bottom = Some(Param::constant(ParamValue::Number(5.0)));
        let c = Clip::of(&l, RationalTime::ZERO, Rect::new(100.0, 0.0, 400.0, 300.0));
        assert_eq!(c.crop[0], 110.0);
        assert_eq!(c.crop[3], 295.0);
        assert!(c.crop[1] < -FAR / 2.0 && c.crop[2] > FAR / 2.0);
    }

    #[test]
    fn 矩形のマスクだけ数える() {
        let mut l = layer();
        l.masks
            .push(Mask::rect("m", MaskRect::new([1.0, 2.0], [3.0, 4.0], 1.0)));
        let mut path = Mask::rect("p", MaskRect::new([0.0, 0.0], [1.0, 1.0], 0.0));
        path.rect = None;
        l.masks.push(path);
        let c = Clip::of(&l, RationalTime::ZERO, Rect::new(0.0, 0.0, 10.0, 10.0));
        assert_eq!(c.mask_count, 1);
        assert_eq!(c.masks[0].rect, [1.0, 2.0, 4.0, 6.0]);
    }
}
