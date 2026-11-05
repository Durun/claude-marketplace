//! 組み立ての設定。口ごとにどの実装を使うかを名前で選ぶ。

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use mm_types::{GpuAdapterId, MmError};

/// 実装の名前。`FromStr` で設定ファイルと環境変数から読む。
/// 口ごとに選べる名前は決まっていて、合わない組み合わせは `assemble` が `Unsupported` で断る。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// `mm-testkit` の見本。
    #[default]
    Sample,
    /// `mm-render`（描画）。
    Wgpu,
    /// `mm-text-glyphon`（文字）。
    Glyphon,
    /// `mm-audio-dsp`（音）。
    Dsp,
    /// `mm-decode-ffmpeg` と `mm-encode-ffmpeg`。feature `ffmpeg` のときだけ組める。
    Ffmpeg,
}

impl Backend {
    /// 設定ファイルに書く名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Sample => mm_testkit::SAMPLE_ID,
            Backend::Wgpu => "wgpu",
            Backend::Glyphon => "glyphon",
            Backend::Dsp => "dsp",
            Backend::Ffmpeg => "ffmpeg",
        }
    }
}

impl FromStr for Backend {
    type Err = MmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            mm_testkit::SAMPLE_ID => Ok(Backend::Sample),
            "wgpu" => Ok(Backend::Wgpu),
            "glyphon" => Ok(Backend::Glyphon),
            "dsp" => Ok(Backend::Dsp),
            "ffmpeg" => Ok(Backend::Ffmpeg),
            other => Err(MmError::Unsupported(format!("実装 {other} は無い"))),
        }
    }
}

/// 口ごとの実装の選択。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub render: Backend,
    pub decode: Backend,
    pub encode: Backend,
    pub color: Backend,
    pub audio: Backend,
    pub text: Backend,
    pub cache: Backend,
    /// キャッシュの大きさの上限（バイト）。`None` は無制限。
    pub cache_limit_bytes: Option<u64>,
    /// 使う GPU。`None` なら高性能のアダプター。描画器・デコーダー・別プロセスのホストが同じものを使う。
    pub gpu: Option<GpuAdapterId>,
}

impl RuntimeConfig {
    /// 全部を見本にした設定。
    pub fn sample() -> Self {
        Self::default()
    }

    /// 段階 0 の本物の組み合わせ。描画は wgpu、文字は glyphon、音は dsp、
    /// デコードとエンコードは ffmpeg。色とキャッシュは本物がまだ無いので見本のまま。
    pub fn real() -> Self {
        Self {
            render: Backend::Wgpu,
            decode: Backend::Ffmpeg,
            encode: Backend::Ffmpeg,
            audio: Backend::Dsp,
            text: Backend::Glyphon,
            ..Self::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名前から実装を選ぶ() {
        assert_eq!("sample".parse::<Backend>().unwrap(), Backend::Sample);
        assert_eq!("ffmpeg".parse::<Backend>().unwrap(), Backend::Ffmpeg);
        assert!("nvenc".parse::<Backend>().is_err());
    }

    #[test]
    fn 名前は往復する() {
        for b in [
            Backend::Sample,
            Backend::Wgpu,
            Backend::Glyphon,
            Backend::Dsp,
            Backend::Ffmpeg,
        ] {
            assert_eq!(b.as_str().parse::<Backend>().unwrap(), b);
        }
    }
}
