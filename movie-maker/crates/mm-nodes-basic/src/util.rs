//! 種類に共通の小さな道具。

use mm_node_api::NodeKind;
use mm_types::{MmError, Params, Result, Rgba, RgbaImage, Size};

/// 今の版の値だけを受ける `migrate`。版が違えば移行が無いと返す。
pub(crate) fn migrate_same_version(
    kind: &dyn NodeKind,
    old: serde_json::Value,
    from: u32,
) -> Result<Params> {
    if from != kind.version() {
        return Err(MmError::Unsupported(format!(
            "{} の版 {from} からの移行が無い",
            kind.id()
        )));
    }
    let params: Params = serde_json::from_value(old)?;
    kind.params().validate(&params)?;
    Ok(params)
}

/// `src` を縦横比を保って `target` に収め、中央に置く（余白は透明）。
/// 大きさが同じなら複製せずそのまま返す。
pub(crate) fn fit_contain(src: RgbaImage, target: Size) -> RgbaImage {
    if src.size() == target || src.width() == 0 || src.height() == 0 {
        return src;
    }
    let ratio =
        (target.width as f32 / src.width() as f32).min(target.height as f32 / src.height() as f32);
    let w = ((src.width() as f32 * ratio).round() as u32).clamp(1, target.width);
    let h = ((src.height() as f32 * ratio).round() as u32).clamp(1, target.height);
    let (ox, oy) = ((target.width - w) / 2, (target.height - h) / 2);
    let mut out = RgbaImage::transparent(target);
    for y in 0..h {
        for x in 0..w {
            let fx = (x as f32 + 0.5) / ratio - 0.5;
            let fy = (y as f32 + 0.5) / ratio - 0.5;
            out.set_pixel(ox + x, oy + y, sample_bilinear(&src, fx, fy));
        }
    }
    out
}

fn lerp(a: Rgba, b: Rgba, t: f32) -> Rgba {
    Rgba::new(
        a.r + (b.r - a.r) * t,
        a.g + (b.g - a.g) * t,
        a.b + (b.b - a.b) * t,
        a.a + (b.a - a.a) * t,
    )
}

fn sample_bilinear(img: &RgbaImage, x: f32, y: f32) -> Rgba {
    let x0 = x.floor().max(0.0);
    let y0 = y.floor().max(0.0);
    let tx = (x - x0).clamp(0.0, 1.0);
    let ty = (y - y0).clamp(0.0, 1.0);
    let at = |dx: u32, dy: u32| {
        let px = (x0 as u32 + dx).min(img.width() - 1);
        let py = (y0 as u32 + dy).min(img.height() - 1);
        img.pixel(px, py)
    };
    let top = lerp(at(0, 0), at(1, 0), tx);
    let bottom = lerp(at(0, 1), at(1, 1), tx);
    lerp(top, bottom, ty)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 同じ大きさはそのまま() {
        let img = RgbaImage::filled(Size::new(4, 2), Rgba::WHITE);
        assert_eq!(fit_contain(img.clone(), Size::new(4, 2)), img);
    }

    #[test]
    fn 縦横比を保って中央に置く() {
        let img = RgbaImage::filled(Size::new(2, 2), Rgba::WHITE);
        let out = fit_contain(img, Size::new(8, 4));
        assert_eq!(out.pixel(0, 0).a, 0.0, "左の余白は透明");
        assert_eq!(out.pixel(4, 2).a, 1.0, "中央は素材");
    }
}
