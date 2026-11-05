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
    /// `mm-text-glyphon` の glyphon（wgpu）の描画。GPU の文脈を使う。
    Glyphon,
    /// `mm-text-glyphon` の CPU（swash）の描画。
    Cpu,
    /// `mm-color-lut`（伝達関数の WGSL と 3D LUT。OCIO なしで動く）。
    Lut,
    /// `mm-color-ocio`（OpenColorIO）。feature `ocio` のときだけ組める。
    Ocio,
    /// `mm-audio-dsp`（音）。
    Dsp,
    /// `mm-decode-ffmpeg` と `mm-encode-ffmpeg`。feature `ffmpeg` のときだけ組める。
    Ffmpeg,
    /// `mm-decode-d3d12va`（デコードだけ）。feature `d3d12va` のときだけ組める。描画器と同じ D3D12 のデバイスが要る。
    D3d12va,
    /// `mm-decode-cuvid`（デコードだけ）。feature `cuvid` のときだけ組める。NVIDIA の GPU が要る。
    Cuvid,
    /// デコードだけ。素材ごとに `d3d12va` → `cuvid` → `ffmpeg` の順に、開いた直後の数コマで判断する。
    Auto,
}

impl Backend {
    /// 設定ファイルに書く名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::D3d12va => "d3d12va",
            Backend::Cuvid => "cuvid",
            Backend::Auto => "auto",
            Backend::Sample => mm_testkit::SAMPLE_ID,
            Backend::Wgpu => "wgpu",
            Backend::Glyphon => "glyphon",
            Backend::Cpu => "cpu",
            Backend::Lut => "lut",
            Backend::Ocio => "ocio",
            Backend::Dsp => "dsp",
            Backend::Ffmpeg => "ffmpeg",
        }
    }
}

impl Backend {
    /// この実装が GPU の文脈を使うか。GPU で復号する実装（`d3d12va`、`cuvid`、それを試す `auto`）は
    /// 描画器と同じデバイスが要る。
    pub fn uses_gpu(self) -> bool {
        matches!(
            self,
            Backend::Wgpu | Backend::D3d12va | Backend::Cuvid | Backend::Auto
        )
    }

    /// 復号に D3D12 のデバイスが要る実装か（描画器のデバイスを D3D12 のアダプターで作る）。
    pub fn needs_d3d12(self) -> bool {
        matches!(self, Backend::D3d12va | Backend::Cuvid | Backend::Auto)
    }
}

impl FromStr for Backend {
    type Err = MmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            mm_testkit::SAMPLE_ID => Ok(Backend::Sample),
            "wgpu" => Ok(Backend::Wgpu),
            "glyphon" => Ok(Backend::Glyphon),
            "cpu" => Ok(Backend::Cpu),
            "lut" => Ok(Backend::Lut),
            "ocio" => Ok(Backend::Ocio),
            "dsp" => Ok(Backend::Dsp),
            "ffmpeg" => Ok(Backend::Ffmpeg),
            "d3d12va" => Ok(Backend::D3d12va),
            "cuvid" => Ok(Backend::Cuvid),
            "auto" => Ok(Backend::Auto),
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

    /// GPU の文脈を作る要る組み合わせか。描画器、デコーダー、文字（glyphon）のどれかが GPU を使うとき。
    pub fn needs_gpu(&self) -> bool {
        self.render.uses_gpu() || self.decode.uses_gpu() || self.text == Backend::Glyphon
    }

    /// 本物の組み合わせ。描画は wgpu、文字は cpu（1080p の字幕で glyphon より速い）、色は lut、音は dsp、
    /// デコードとエンコードは ffmpeg。キャッシュは本物がまだ無いので見本のまま。
    pub fn real() -> Self {
        Self {
            render: Backend::Wgpu,
            decode: Backend::Ffmpeg,
            encode: Backend::Ffmpeg,
            audio: Backend::Dsp,
            text: Backend::Cpu,
            color: Backend::Lut,
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
    fn gpu_で復号する実装は_gpu_を使う() {
        for b in [
            Backend::D3d12va,
            Backend::Cuvid,
            Backend::Auto,
            Backend::Wgpu,
        ] {
            assert!(b.uses_gpu(), "{b:?}");
        }
        for b in [
            Backend::Sample,
            Backend::Ffmpeg,
            Backend::Dsp,
            Backend::Glyphon,
        ] {
            assert!(!b.uses_gpu(), "{b:?}");
        }
        assert!(Backend::Auto.needs_d3d12() && !Backend::Wgpu.needs_d3d12());
    }

    #[test]
    fn gpu_の要否は描画器の選択で決まる() {
        assert!(!RuntimeConfig::sample().needs_gpu());
        assert!(RuntimeConfig::real().needs_gpu());
        let decode_only = RuntimeConfig {
            decode: Backend::Wgpu,
            ..RuntimeConfig::sample()
        };
        assert!(
            decode_only.needs_gpu(),
            "デコーダーが GPU を使う選択でも文脈は要る"
        );
    }

    #[test]
    fn 名前は往復する() {
        for b in [
            Backend::Sample,
            Backend::Wgpu,
            Backend::Glyphon,
            Backend::Cpu,
            Backend::Lut,
            Backend::Ocio,
            Backend::Dsp,
            Backend::Ffmpeg,
            Backend::D3d12va,
            Backend::Cuvid,
            Backend::Auto,
        ] {
            assert_eq!(b.as_str().parse::<Backend>().unwrap(), b);
        }
    }
}
