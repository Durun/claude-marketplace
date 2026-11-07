//! プレート（角丸の地、縁、ずらした影）を描く `shader.wgsl` のコード。
//!
//! 色は文字のスタイルごとに定数としてコードに埋め、位置と大きさは数のパラメータで渡す。同じ文字のスタイルのプレートは全部
//! 同じコードになるので、パイプラインは文字のスタイルごとに 1 つで済む。位置はコンポジションの幅と高さに対する比（0..1）、長さはコンポジションの高さに
//! 対する比で渡し、プレビューの倍率に依らず同じ形に描く。

use mm_types::{ParamValue, Result, Rgba};

use crate::style::PlateSpec;
use crate::theme::Theme;

/// プレートの形を渡すパラメータ。`p0`..`p3` が左上と大きさ、`p4` が角の半径、`p5` が縁の太さ、`p6` が影のずらし。
pub(crate) const RECT_PARAMS: [&str; 4] = ["p0", "p1", "p2", "p3"];
pub(crate) const RADIUS_PARAM: &str = "p4";
pub(crate) const BORDER_PARAM: &str = "p5";
pub(crate) const SHADOW_PARAM: &str = "p6";

const BODY: &str = r"
fn pk_box(p: vec2<f32>, half: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half + vec2<f32>(r);
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn pk_cover(d: f32) -> f32 {
    return clamp(0.5 - d, 0.0, 1.0);
}

fn mm_main(uv: vec2<f32>) -> vec4<f32> {
    let px = uv * u.size;
    let k = u.size.y;
    let half = u.p0.zw * u.size * 0.5;
    let center = u.p0.xy * u.size + half;
    let r = min(u.p1.x * k, min(half.x, half.y));
    let line = u.p1.y * k;
    let off = vec2<f32>(u.p1.z * k);
    let outer = pk_cover(pk_box(px - center, half, r));
    let inner = pk_cover(pk_box(px - center, half - vec2<f32>(line), max(r - line, 0.0)));
    let shadow = PK_SHADOW * pk_cover(pk_box(px - center - off, half, r));
    let plate = PK_FILL * inner + PK_LINE * (outer - inner);
    return plate + shadow * (1.0 - plate.a);
}
";

/// プレートの WGSL。地、縁、影の色を定数にする。
pub(crate) fn code(theme: &Theme, plate: &PlateSpec) -> Result<String> {
    let fill = theme.color(&plate.color)?.scaled(plate.opacity as f32);
    let line = match &plate.border {
        Some(b) => theme.color(&b.color)?,
        None => Rgba::TRANSPARENT,
    };
    let shadow = match &plate.shadow {
        Some(s) => theme.color(&s.color)?,
        None => Rgba::TRANSPARENT,
    };
    Ok(format!(
        "const PK_FILL = {};\nconst PK_LINE = {};\nconst PK_SHADOW = {};\n{BODY}",
        vec4(fill),
        vec4(line),
        vec4(shadow)
    ))
}

fn vec4(c: Rgba) -> String {
    format!("vec4<f32>({:.6}, {:.6}, {:.6}, {:.6})", c.r, c.g, c.b, c.a)
}

/// プレートの形のパラメータ（名前と値）。`rect` は左上と大きさ（画素）、`comp_size` はコンポジションの幅と高さ。
pub(crate) fn shape_params(
    plate: &PlateSpec,
    rect: [f64; 4],
    comp_size: [f64; 2],
) -> Vec<(&'static str, ParamValue)> {
    let [w, h] = comp_size;
    let normalized = [rect[0] / w, rect[1] / h, rect[2] / w, rect[3] / h];
    let mut out: Vec<(&'static str, ParamValue)> = RECT_PARAMS
        .iter()
        .zip(normalized)
        .map(|(name, v)| (*name, ParamValue::Number(v)))
        .collect();
    let border = plate.border.as_ref().map_or(0.0, |b| b.width);
    let shadow = plate.shadow.as_ref().map_or(0.0, |s| s.offset);
    out.push((RADIUS_PARAM, ParamValue::Number(plate.radius / h)));
    out.push((BORDER_PARAM, ParamValue::Number(border / h)));
    out.push((SHADOW_PARAM, ParamValue::Number(shadow / h)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn theme() -> Theme {
        serde_json::from_value(serde_json::json!({
            "colors": {"paper": "#F5F8FF", "ink": "#0B1230"},
            "motion": {"in_max": 0.8, "out_max": 0.3},
            "canvas": {"width": 1920, "height": 1080, "fps": 30}
        }))
        .unwrap()
    }

    fn plate() -> PlateSpec {
        serde_json::from_value(serde_json::json!({
            "color": "paper", "padding": [20, 10], "radius": 8,
            "border": {"color": "ink", "width": 4}, "shadow": {"color": "ink", "offset": 6}
        }))
        .unwrap()
    }

    #[test]
    fn 同じスタイルのプレートは同じコードになり_形はパラメータで渡す() {
        let a = code(&theme(), &plate()).unwrap();
        assert!(a.contains("const PK_FILL"));
        assert!(a.contains("fn mm_main"));
        let params = shape_params(&plate(), [192.0, 108.0, 960.0, 54.0], [1920.0, 1080.0]);
        let get = |n: &str| {
            params
                .iter()
                .find(|(k, _)| *k == n)
                .unwrap()
                .1
                .as_number()
                .unwrap()
        };
        assert!((get("p0") - 0.1).abs() < 1e-12);
        assert!((get("p3") - 0.05).abs() < 1e-12);
        assert!((get(BORDER_PARAM) - 4.0 / 1080.0).abs() < 1e-12);
    }
}
