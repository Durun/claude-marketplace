//! レイヤーの変形（位置・拡大・回転）を 2D のアフィン行列にする。
//!
//! 文書の `Layer` にはまだ変形の欄が無いので、レイヤーの動画オブジェクトのパラメータのうち
//! `transform.` で始まる名前を描画器が先に取り除いて読む（種類には渡さない）。
//! - `transform.position`（Vec2）: 動かす量。コンポジションの画素。既定は動かさない。
//! - `transform.scale`（Number か Vec2）: 倍率。1 が等倍。
//! - `transform.rotation`（Number）: 度。正が時計回り。
//! - `transform.anchor`（Vec2）: 回転と拡大の中心。既定はレイヤーの絵の中心。

use mm_types::{ParamValue, Params};

/// 描画器が読む変形のパラメータの前置き。
pub const TRANSFORM_PREFIX: &str = "transform.";

/// 2D のアフィン変換 `(x, y) -> (a x + c y + tx, b x + d y + ty)`。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    /// 恒等。
    pub const IDENTITY: Self = Self {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// 点を写す。
    pub fn apply(&self, x: f32, y: f32) -> (f32, f32) {
        (
            self.a * x + self.c * y + self.tx,
            self.b * x + self.d * y + self.ty,
        )
    }

    /// `self` の後に `next` をかけた変換。
    pub fn then(&self, next: &Self) -> Self {
        Self {
            a: next.a * self.a + next.c * self.b,
            b: next.b * self.a + next.d * self.b,
            c: next.a * self.c + next.c * self.d,
            d: next.b * self.c + next.d * self.d,
            tx: next.a * self.tx + next.c * self.ty + next.tx,
            ty: next.b * self.tx + next.d * self.ty + next.ty,
        }
    }

    fn translate(x: f32, y: f32) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::IDENTITY
        }
    }

    fn scale_rotate(sx: f32, sy: f32, degrees: f32) -> Self {
        let (s, c) = degrees.to_radians().sin_cos();
        Self {
            a: c * sx,
            b: s * sx,
            c: -s * sy,
            d: c * sy,
            tx: 0.0,
            ty: 0.0,
        }
    }
}

/// レイヤーの変形。
#[derive(Debug, Clone, PartialEq)]
pub struct LayerTransform {
    pub position: [f32; 2],
    pub scale: [f32; 2],
    pub rotation_deg: f32,
    pub anchor: Option<[f32; 2]>,
}

impl Default for LayerTransform {
    fn default() -> Self {
        Self {
            position: [0.0, 0.0],
            scale: [1.0, 1.0],
            rotation_deg: 0.0,
            anchor: None,
        }
    }
}

impl LayerTransform {
    /// `params` から `transform.` の値を取り除いて読む。
    pub fn take_from(params: &mut Params) -> Self {
        let names: Vec<String> = params
            .0
            .keys()
            .filter(|k| k.starts_with(TRANSFORM_PREFIX))
            .cloned()
            .collect();
        let mut t = Self::default();
        for name in names {
            let Some(value) = params.0.remove(&name) else {
                continue;
            };
            match (&name[TRANSFORM_PREFIX.len()..], value) {
                ("position", ParamValue::Vec2(v)) => t.position = [v[0] as f32, v[1] as f32],
                ("scale", ParamValue::Number(n)) => t.scale = [n as f32, n as f32],
                ("scale", ParamValue::Vec2(v)) => t.scale = [v[0] as f32, v[1] as f32],
                ("rotation", ParamValue::Number(n)) => t.rotation_deg = n as f32,
                ("anchor", ParamValue::Vec2(v)) => t.anchor = Some([v[0] as f32, v[1] as f32]),
                _ => {}
            }
        }
        t
    }

    /// 恒等か。
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// 長辺方向の拡大の大きさ。`vector` の種類を描く倍率の手がかり。
    pub fn max_scale(&self) -> f32 {
        self.scale[0].abs().max(self.scale[1].abs())
    }

    /// 大きさ `(w, h)` の絵（左上がコンポジションの原点）を、コンポジションの画素の上に置く変換。
    pub fn to_comp(&self, w: f32, h: f32) -> Affine {
        let [ax, ay] = self.anchor.unwrap_or([w / 2.0, h / 2.0]);
        Affine::translate(-ax, -ay)
            .then(&Affine::scale_rotate(
                self.scale[0],
                self.scale[1],
                self.rotation_deg,
            ))
            .then(&Affine::translate(
                ax + self.position[0],
                ay + self.position[1],
            ))
    }
}

/// 絵の UV（0..1）を出力の NDC に写す、列優先の 4x4 行列。
/// `comp_to_out` はコンポジションの画素から出力の画素への倍率、`out` は出力の大きさ。
pub fn uv_to_ndc(
    transform: &LayerTransform,
    layer_size: (f32, f32),
    comp_to_out: f32,
    out: (f32, f32),
) -> [f32; 16] {
    let (w, h) = layer_size;
    let uv_to_comp = Affine {
        a: w,
        d: h,
        ..Affine::IDENTITY
    };
    let comp_to_ndc = Affine {
        a: comp_to_out * 2.0 / out.0,
        d: -comp_to_out * 2.0 / out.1,
        tx: -1.0,
        ty: 1.0,
        ..Affine::IDENTITY
    };
    let m = uv_to_comp.then(&transform.to_comp(w, h)).then(&comp_to_ndc);
    [
        m.a, m.b, 0.0, 0.0, //
        m.c, m.d, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        m.tx, m.ty, 0.0, 1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f32, f32), b: (f32, f32)) -> bool {
        (a.0 - b.0).abs() < 1e-5 && (a.1 - b.1).abs() < 1e-5
    }

    fn corner(m: &[f32; 16], u: f32, v: f32) -> (f32, f32) {
        (m[0] * u + m[4] * v + m[12], m[1] * u + m[5] * v + m[13])
    }

    #[test]
    fn 恒等なら絵が画面全体に重なる() {
        let m = uv_to_ndc(
            &LayerTransform::default(),
            (320.0, 180.0),
            1.0,
            (320.0, 180.0),
        );
        assert!(close(corner(&m, 0.0, 0.0), (-1.0, 1.0)));
        assert!(close(corner(&m, 1.0, 1.0), (1.0, -1.0)));
    }

    #[test]
    fn 出力が半分でも画面全体に重なる() {
        let m = uv_to_ndc(
            &LayerTransform::default(),
            (320.0, 180.0),
            0.5,
            (160.0, 90.0),
        );
        assert!(close(corner(&m, 1.0, 0.0), (1.0, 1.0)));
    }

    #[test]
    fn 位置は画素で動かす() {
        let t = LayerTransform {
            position: [160.0, 0.0],
            ..LayerTransform::default()
        };
        let m = uv_to_ndc(&t, (320.0, 180.0), 1.0, (320.0, 180.0));
        assert!(close(corner(&m, 0.0, 0.0), (0.0, 1.0)), "半分右へ");
    }

    #[test]
    fn 二倍の拡大は中心を保つ() {
        let t = LayerTransform {
            scale: [2.0, 2.0],
            ..LayerTransform::default()
        };
        let m = uv_to_ndc(&t, (320.0, 180.0), 1.0, (320.0, 180.0));
        assert!(close(corner(&m, 0.5, 0.5), (0.0, 0.0)));
        assert!(close(corner(&m, 0.0, 0.0), (-2.0, 2.0)));
    }

    #[test]
    fn 正の回転は時計回り() {
        let t = LayerTransform {
            rotation_deg: 90.0,
            anchor: Some([0.0, 0.0]),
            ..LayerTransform::default()
        };
        let a = t.to_comp(10.0, 10.0);
        assert!(close(a.apply(10.0, 0.0), (0.0, 10.0)), "右が下へ回る");
    }

    #[test]
    fn パラメータから読んで取り除く() {
        let mut p = Params::new();
        p.set("transform.scale", ParamValue::Number(2.0));
        p.set("transform.rotation", ParamValue::Number(30.0));
        p.set("color", ParamValue::Number(1.0));
        let t = LayerTransform::take_from(&mut p);
        assert_eq!(t.scale, [2.0, 2.0]);
        assert_eq!(t.rotation_deg, 30.0);
        assert_eq!(p.0.len(), 1, "種類に渡す分だけ残る");
    }
}
