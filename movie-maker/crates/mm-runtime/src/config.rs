//! 組み立ての設定。口ごとにどの実装を使うかを名前で選ぶ。

use std::str::FromStr;

use serde::{Deserialize, Serialize};

use mm_types::MmError;

/// 実装の名前。`FromStr` で設定ファイルと環境変数から読む。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// `mm-testkit` の見本。
    #[default]
    Sample,
}

impl Backend {
    /// 設定ファイルに書く名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::Sample => mm_testkit::SAMPLE_ID,
        }
    }
}

impl FromStr for Backend {
    type Err = MmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            mm_testkit::SAMPLE_ID => Ok(Backend::Sample),
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
}

impl RuntimeConfig {
    /// 全部を見本にした設定。
    pub fn sample() -> Self {
        Self::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 名前から実装を選ぶ() {
        assert_eq!("sample".parse::<Backend>().unwrap(), Backend::Sample);
        assert!("ffmpeg".parse::<Backend>().is_err());
    }
}
