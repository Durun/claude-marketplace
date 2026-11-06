//! Rec.1886 の表示の式（べき 2.4）の逆。8 ビットの符号化値から線形へ。
//!
//! 式は `mm-color-lut` の `bt1886_to_linear` と同じ。実装どうしを依存させないため複製している。

const GAMMA: f32 = 2.4;

/// 8 ビットの符号化値 0..=255 → 線形の表。全画素で `powf` を呼ばないためのルックアップテーブル。
pub fn decode_table() -> [f32; 256] {
    std::array::from_fn(|code| (code as f32 / 255.0).powf(GAMMA))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 端と中間の値が式どおりになる() {
        let t = decode_table();
        assert_eq!(t[0], 0.0);
        assert_eq!(t[255], 1.0);
        assert!((t[128] - 0.1913).abs() < 1e-3, "{}", t[128]);
        assert!(t.windows(2).all(|w| w[0] < w[1]), "単調に増える");
    }
}
