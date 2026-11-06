//! Rec.1886 の表示の式（べき 2.4）。線形から 8 ビットの符号化値へ。
//!
//! 式は `mm-color-lut` の `linear_to_bt1886` と同じ。実装どうしを依存させないため複製している。

const GAMMA: f32 = 2.4;

/// ルックアップテーブルの分解能。1 段が符号化値の半分の幅より細かくなる暗部の下限（`DARK_LIMIT`）から上で、
/// 符号化値の丸めと一致する。
const LUT_SIZE: usize = 1 << 16;

/// これより暗い線形の値はルックアップテーブルの 1 段が符号化値の幅を超えるので、境目の二分探索で決める。
const DARK_LIMIT: f32 = 1.0e-4;

/// 線形の値を 8 ビットの符号化値へ丸める表。符号化値どうしの中点の線形値で区切る。
/// 全画素で `powf` を呼ばないので 1080p でも書き出しの足を引かない。
pub struct EncodeTable {
    /// `bounds[c]` は符号化値 `c` と `c + 1` の境目。
    bounds: [f32; 255],
    /// 線形の値を `LUT_SIZE - 1` 倍した段の符号化値。
    lut: Vec<u8>,
}

impl EncodeTable {
    pub fn new() -> Self {
        let bounds = std::array::from_fn(|c| ((c as f32 + 0.5) / 255.0).powf(GAMMA));
        let lut = (0..LUT_SIZE)
            .map(|i| search(&bounds, (i as f32 + 0.5) / (LUT_SIZE - 1) as f32))
            .collect();
        Self { bounds, lut }
    }

    /// 線形の値 → 符号化値。範囲外は端に丸め、NaN は 0。
    pub fn encode(&self, linear: f32) -> u8 {
        if linear.is_nan() || linear < DARK_LIMIT {
            return search(&self.bounds, linear);
        }
        // `as` は飽和するので 1 を超える値は最後の段になる
        self.lut[((linear * (LUT_SIZE - 1) as f32) as usize).min(LUT_SIZE - 1)]
    }
}

fn search(bounds: &[f32; 255], linear: f32) -> u8 {
    bounds.partition_point(|b| *b <= linear) as u8
}

impl Default for EncodeTable {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 符号化値の線形を戻すと元の符号化値になる() {
        let table = EncodeTable::new();
        for code in 0..=255u32 {
            let linear = (code as f32 / 255.0).powf(GAMMA);
            assert_eq!(u32::from(table.encode(linear)), code);
        }
    }

    #[test]
    fn 全域で式の丸めと高々_1_段しかずれない() {
        let table = EncodeTable::new();
        let mut v = 1.0e-7f32;
        while v < 1.0 {
            let exact = (v.powf(1.0 / GAMMA) * 255.0).round() as i32;
            let got = i32::from(table.encode(v));
            assert!((got - exact).abs() <= 1, "線形 {v}: 式 {exact} 表 {got}");
            v *= 1.003;
        }
    }

    #[test]
    fn 範囲外は端に丸める() {
        let table = EncodeTable::new();
        assert_eq!(table.encode(-1.0), 0);
        assert_eq!(table.encode(2.0), 255);
        assert_eq!(table.encode(f32::NAN), 0);
    }
}
