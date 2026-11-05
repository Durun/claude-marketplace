//! 見本の描画器が使う、レイヤーの変形（位置・拡大・回転）の CPU 版。
//!
//! 式は `mm_doc::Transform` の約束どおり（コンポジションの画素と度、アンカーは回転と拡大の中心）。
//! 出力の画素の中心を逆に写して最近傍で引く。wgpu 版と同じ画素に乗る変形（整数の移動、
//! 画素の境目に揃う拡大）で、契約テストが見本と実装を同じ値で比べられる。

use mm_doc::Transform;
use mm_eval::evaluate_transform;
use mm_types::{RationalTime, RgbaImage};

/// `image`（コンポジションを `comp_to_out` 倍した大きさ）に変形を掛けた絵。
/// 恒等ならそのまま返す。逆行列が無い（拡大が 0）ときは透明。
pub fn apply_transform(
    image: &RgbaImage,
    doc: &Transform,
    t: RationalTime,
    comp_to_out: f64,
) -> RgbaImage {
    if doc.is_identity() {
        return image.clone();
    }
    let tr = evaluate_transform(doc, t);
    let (w, h) = (
        f64::from(image.width()) / comp_to_out,
        f64::from(image.height()) / comp_to_out,
    );
    let [ax, ay] = tr.anchor.unwrap_or([w / 2.0, h / 2.0]);
    let (sin, cos) = tr.rotation_deg.to_radians().sin_cos();
    let det = tr.scale[0] * tr.scale[1];
    let mut out = RgbaImage::transparent(image.size());
    if det == 0.0 {
        return out;
    }
    for oy in 0..image.height() {
        for ox in 0..image.width() {
            // 出力の画素の中心 → コンポジションの画素 → アンカー基準 → 逆回転 → 逆拡大。
            let px = (f64::from(ox) + 0.5) / comp_to_out - (ax + tr.position[0]);
            let py = (f64::from(oy) + 0.5) / comp_to_out - (ay + tr.position[1]);
            let rx = cos * px + sin * py;
            let ry = -sin * px + cos * py;
            let u = (rx / tr.scale[0] + ax) * comp_to_out;
            let v = (ry / tr.scale[1] + ay) * comp_to_out;
            if u >= 0.0 && v >= 0.0 {
                out.set_pixel(ox, oy, image.pixel(u as u32, v as u32));
            }
        }
    }
    out
}
