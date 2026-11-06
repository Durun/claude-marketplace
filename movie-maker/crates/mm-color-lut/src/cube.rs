//! `.cube` の 3D LUT（Adobe / Resolve の形）の読み込みと、CPU での三線形補間。

use mm_color_api::TransformProgram;
use mm_types::{MmError, Result};

/// `TransformProgram::Lut3d` に入れられる最大の一辺。メモリの暴走を防ぐ。
const MAX_SIZE: u32 = 129;

/// 3D LUT。`data` は `size^3 * 3` の f32 で、R が最も速く変わる。
#[derive(Debug, Clone, PartialEq)]
pub struct Cube3d {
    pub size: u32,
    pub data: Vec<f32>,
    pub domain_min: [f32; 3],
    pub domain_max: [f32; 3],
}

impl Cube3d {
    /// `.cube` のテキストを読む。1D LUT は未対応。
    pub fn parse(text: &str) -> Result<Self> {
        let mut size = None;
        let mut domain_min = [0.0; 3];
        let mut domain_max = [1.0; 3];
        let mut data = Vec::new();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() || line.starts_with("TITLE") {
                continue;
            }
            let mut words = line.split_whitespace();
            match words.next() {
                Some("LUT_3D_SIZE") => size = Some(parse_size(words.next())?),
                Some("LUT_1D_SIZE") => return Err(MmError::Unsupported("1D LUT".into())),
                Some("DOMAIN_MIN") => domain_min = parse_triple(words)?,
                Some("DOMAIN_MAX") => domain_max = parse_triple(words)?,
                Some("LUT_3D_INPUT_RANGE") => {
                    // Resolve の書き方で、3 チャンネル共通の DOMAIN_MIN / DOMAIN_MAX と同じ意味。
                    let low = parse_number(words.next())?;
                    let high = parse_number(words.next())?;
                    domain_min = [low; 3];
                    domain_max = [high; 3];
                }
                // 知らないキーワードの行はスキップする。値の行は数字・符号・小数点で始まる。
                Some(first) if first.starts_with(|c: char| c.is_ascii_alphabetic()) => {}
                Some(first) => {
                    let row = parse_triple(std::iter::once(first).chain(words))?;
                    data.extend_from_slice(&row);
                }
                None => {}
            }
        }
        let size = size.ok_or_else(|| MmError::Corrupt("LUT_3D_SIZE が無い".into()))?;
        let expected = (size as usize).pow(3) * 3;
        if data.len() != expected {
            return Err(MmError::Corrupt(format!(
                "LUT の行数が合わない: {} 値、期待 {expected}",
                data.len()
            )));
        }
        Ok(Self {
            size,
            data,
            domain_min,
            domain_max,
        })
    }

    /// 入力を 0..=1 に切るインターフェースの契約に合わせ、領域が 0..=1 の LUT だけ渡せる。
    pub fn to_program(&self) -> Result<TransformProgram> {
        if self.domain_min != [0.0; 3] || self.domain_max != [1.0; 3] {
            return Err(MmError::Unsupported(
                "DOMAIN が 0..1 でない LUT（呼び出し側で正規化する）".into(),
            ));
        }
        Ok(TransformProgram::Lut3d {
            size: self.size,
            data: self.data.clone(),
        })
    }

    /// 三線形補間で 1 画素に適用する。領域の外は端に切る。
    pub fn sample(&self, rgb: [f32; 3]) -> [f32; 3] {
        sample_trilinear(self.size, &self.data, self.normalize(rgb))
    }

    fn normalize(&self, rgb: [f32; 3]) -> [f32; 3] {
        std::array::from_fn(|i| {
            let span = self.domain_max[i] - self.domain_min[i];
            ((rgb[i] - self.domain_min[i]) / span).clamp(0.0, 1.0)
        })
    }
}

/// `Lut3d` の内容を三線形補間で引く。入力は 0..=1 に切る。
pub fn sample_trilinear(size: u32, data: &[f32], rgb: [f32; 3]) -> [f32; 3] {
    let n = size as usize;
    let max = (n - 1) as f32;
    let p: [f32; 3] = std::array::from_fn(|i| rgb[i].clamp(0.0, 1.0) * max);
    let base: [usize; 3] = std::array::from_fn(|i| p[i].floor() as usize);
    let frac: [f32; 3] = std::array::from_fn(|i| p[i] - base[i] as f32);
    let at = |r: usize, g: usize, b: usize| -> [f32; 3] {
        let i = ((b * n + g) * n + r) * 3;
        [data[i], data[i + 1], data[i + 2]]
    };
    let mut out = [0.0_f32; 3];
    for corner in 0..8_usize {
        let (dr, dg, db) = (corner & 1, (corner >> 1) & 1, (corner >> 2) & 1);
        let weight = [dr, dg, db]
            .iter()
            .zip(frac)
            .map(|(&d, f)| if d == 1 { f } else { 1.0 - f })
            .product::<f32>();
        let v = at(
            (base[0] + dr).min(n - 1),
            (base[1] + dg).min(n - 1),
            (base[2] + db).min(n - 1),
        );
        for (o, c) in out.iter_mut().zip(v) {
            *o += weight * c;
        }
    }
    out
}

fn parse_size(word: Option<&str>) -> Result<u32> {
    let size: u32 = word
        .and_then(|w| w.parse().ok())
        .ok_or_else(|| MmError::Corrupt("LUT_3D_SIZE の値が読めない".into()))?;
    if !(2..=MAX_SIZE).contains(&size) {
        return Err(MmError::Corrupt(format!("LUT_3D_SIZE {size} が範囲外")));
    }
    Ok(size)
}

fn parse_number(word: Option<&str>) -> Result<f32> {
    word.and_then(|w| w.parse().ok())
        .ok_or_else(|| MmError::Corrupt("数が読めない行".into()))
}

fn parse_triple<'a>(mut words: impl Iterator<Item = &'a str>) -> Result<[f32; 3]> {
    let mut out = [0.0_f32; 3];
    for slot in &mut out {
        *slot = words
            .next()
            .and_then(|w| w.parse().ok())
            .ok_or_else(|| MmError::Corrupt("数が 3 つ読めない行".into()))?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 恒等の LUT を作る。
    pub(crate) fn identity(size: u32) -> Cube3d {
        let n = size as usize;
        let mut data = Vec::with_capacity(n * n * n * 3);
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    data.extend([r, g, b].map(|v| v as f32 / (n - 1) as f32));
                }
            }
        }
        Cube3d {
            size,
            data,
            domain_min: [0.0; 3],
            domain_max: [1.0; 3],
        }
    }

    fn to_text(lut: &Cube3d) -> String {
        let mut s = format!("TITLE \"t\"\n# c\nLUT_3D_SIZE {}\n", lut.size);
        for row in lut.data.chunks(3) {
            s.push_str(&format!("{} {} {}\n", row[0], row[1], row[2]));
        }
        s
    }

    #[test]
    fn 書いて読むと同じになる() {
        let lut = identity(3);
        assert_eq!(Cube3d::parse(&to_text(&lut)).unwrap(), lut);
    }

    #[test]
    fn 恒等のlutは補間しても値を変えない() {
        let lut = identity(5);
        for rgb in [[0.1, 0.5, 0.9], [0.33, 0.77, 0.01], [1.0, 0.0, 0.5]] {
            let out = lut.sample(rgb);
            for (a, b) in out.iter().zip(rgb) {
                assert!((a - b).abs() < 1e-5, "{a} {b}");
            }
        }
    }

    #[test]
    fn resolveの入力範囲の行は領域として読み知らない行はスキップする() {
        let text = to_text(&identity(2)).replace(
            "LUT_3D_SIZE 2",
            "LUT_3D_SIZE 2\nLUT_3D_INPUT_RANGE 0.0 2.0\nFOO bar",
        );
        let lut = Cube3d::parse(&text).unwrap();
        assert_eq!(lut.domain_max, [2.0; 3]);
        assert_eq!(lut.data.len(), 24);
    }

    #[test]
    fn 壊れた入力はcorruptで返す() {
        assert!(matches!(
            Cube3d::parse("LUT_3D_SIZE 2\n0 0 0\n"),
            Err(MmError::Corrupt(_))
        ));
        assert!(matches!(
            Cube3d::parse("LUT_1D_SIZE 4\n"),
            Err(MmError::Unsupported(_))
        ));
        assert!(matches!(Cube3d::parse("0 0 0\n"), Err(MmError::Corrupt(_))));
    }
}
