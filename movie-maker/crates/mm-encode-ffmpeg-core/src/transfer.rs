//! Rec.1886 の表示の式（べき 2.4）。線形から 8 ビットの符号化値へ。
//!
//! 式は `mm-color-lut` の `linear_to_bt1886` と同じ。実装どうしを依存させないため複製している。

const GAMMA: f32 = 2.4;

/// ルックアップテーブルの分解能。1 ステップが符号化値の半分の幅より細かくなる暗部の下限（`DARK_LIMIT`）から上で、
/// 符号化値の丸めと一致する。
const LUT_SIZE: usize = 1 << 16;

/// これより暗い線形の値はルックアップテーブルの 1 ステップが符号化値の幅を超えるので、境界の二分探索で決める。
const DARK_LIMIT: f32 = 1.0e-4;

/// 線形の値を 8 ビットの符号化値へ丸める表。符号化値どうしの中点の線形値で区切る。
/// 全画素で `powf` を呼ばないので 1080p でも書き出しの足を引かない。
pub struct EncodeTable {
    /// `bounds[c]` は符号化値 `c` と `c + 1` の境界。
    bounds: [f32; 255],
    /// 線形の値を `LUT_SIZE - 1` 倍したステップの符号化値。
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
        // `as` は飽和するので 1 を超える値は最後のステップになる
        self.lut[((linear * (LUT_SIZE - 1) as f32) as usize).min(LUT_SIZE - 1)]
    }
}

/// 16 ビットの表で、これより暗い線形の値は式で求める。表の 1 ステップ（`1 / LUT_SIZE`）の符号化値の幅が、10 ビットの
/// 半ステップ（`TOLERANCE_16` = 32）を超える暗さの上限。
const DARK_LIMIT_16: f32 = 1.0e-3;
/// 16 ビットの表と式のずれの上限（16 ビットの符号化値で、10 ビットの半ステップ）。
#[cfg(test)]
const TOLERANCE_16: i32 = 32;

/// 線形の値を 16 ビットの符号化値へ。10 ビットの YUV へ縮める前の精度を保つ（8 ビットを経由すると 10 ビットの階調が出ない）。
pub struct EncodeTable16 {
    lut: Vec<u16>,
}

impl EncodeTable16 {
    pub fn new() -> Self {
        let lut = (0..LUT_SIZE)
            .map(|i| exact16((i as f32 + 0.5) / (LUT_SIZE - 1) as f32))
            .collect();
        Self { lut }
    }

    /// 線形の値 → 符号化値。範囲外は端に丸め、NaN は 0。暗部は表の 1 ステップが粗いので式で求める。
    pub fn encode(&self, linear: f32) -> u16 {
        if linear.is_nan() || linear < DARK_LIMIT_16 {
            return exact16(linear);
        }
        self.lut[((linear * (LUT_SIZE - 1) as f32) as usize).min(LUT_SIZE - 1)]
    }
}

impl Default for EncodeTable16 {
    fn default() -> Self {
        Self::new()
    }
}

fn exact16(linear: f32) -> u16 {
    if linear.is_nan() {
        return 0;
    }
    (linear.clamp(0.0, 1.0).powf(1.0 / GAMMA) * 65535.0).round() as u16
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
    fn 全域で式の丸めと高々_1_ステップしかずれない() {
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
    fn 十六ビットの表は式と_10_ビットの半ステップ以内しかずれない() {
        let table = EncodeTable16::new();
        let mut v = 1.0e-7f32;
        while v < 1.0 {
            let exact = i32::from(exact16(v));
            let got = i32::from(table.encode(v));
            assert!(
                (got - exact).abs() <= TOLERANCE_16,
                "線形 {v}: 式 {exact} 表 {got}"
            );
            v *= 1.003;
        }
        assert_eq!(table.encode(1.0), 65535);
        assert_eq!(table.encode(-1.0), 0);
        assert_eq!(table.encode(f32::NAN), 0);
    }

    #[test]
    fn 範囲外は端に丸める() {
        let table = EncodeTable::new();
        assert_eq!(table.encode(-1.0), 0);
        assert_eq!(table.encode(2.0), 255);
        assert_eq!(table.encode(f32::NAN), 0);
    }
}
