//! 組み立ての設定。インターフェースごとにどの実装を使うかを名前で選ぶ。

use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use mm_types::{GpuAdapterId, MmError};

/// 実装の名前。`FromStr` で設定ファイルと環境変数から読む。
/// インターフェースごとに選べる名前は決まっていて、合わない組み合わせは `assemble` が `Unsupported` で拒否する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// `mm-testkit` のモック。
    #[default]
    Sample,
    /// `mm-render`（描画）。
    Wgpu,
    /// `mm-text-glyphon` の glyphon（wgpu）の描画。GPU のコンテキストを使う。
    Glyphon,
    /// `mm-text-cef` の CSS の描画（行組みは cosmic-text）。CEF の配布物とホストの実行ファイルが要る。
    Cef,
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
    /// `mm-decode-d3d12va`（デコードだけ）。feature `d3d12va` のときだけ組める。レンダラーと同じ D3D12 のデバイスが要る。
    D3d12va,
    /// `mm-decode-cuvid`（デコードだけ）。feature `cuvid` のときだけ組める。NVIDIA の GPU が要る。
    Cuvid,
    /// `mm-cache-fs`（ディスクのキャッシュ）。`cache_dir` が要る。
    Fs,
    /// `mm-encode-nvenc`（エンコードだけ）。feature `ffmpeg` のときだけ組める。NVIDIA の GPU が要る。
    Nvenc,
    /// デコードは、素材ごとに `d3d12va` → `cuvid` → `ffmpeg` の順に、開いた直後の数フレームで判断する。
    /// エンコードは、コーデックごとに `nvenc` → `ffmpeg` の順に選び、`h264_nvenc` が使えなければ `h264` で書く。
    Auto,
}

impl Backend {
    /// 設定ファイルに書く名前。
    pub fn as_str(self) -> &'static str {
        match self {
            Backend::D3d12va => "d3d12va",
            Backend::Cuvid => "cuvid",
            Backend::Nvenc => "nvenc",
            Backend::Auto => "auto",
            Backend::Fs => "fs",
            Backend::Sample => mm_testkit::SAMPLE_ID,
            Backend::Wgpu => "wgpu",
            Backend::Glyphon => "glyphon",
            Backend::Cef => "cef",
            Backend::Cpu => "cpu",
            Backend::Lut => "lut",
            Backend::Ocio => "ocio",
            Backend::Dsp => "dsp",
            Backend::Ffmpeg => "ffmpeg",
        }
    }
}

impl Backend {
    /// この実装が GPU のコンテキストを使うか。GPU でデコードする実装（`d3d12va`、`cuvid`、それを試す `auto`）は
    /// レンダラーと同じデバイスが要る。
    pub fn uses_gpu(self) -> bool {
        matches!(
            self,
            Backend::Wgpu | Backend::D3d12va | Backend::Cuvid | Backend::Auto
        )
    }

    /// GPU のコンテキストが無いと組めない実装か。`auto` はコンテキストが無ければ ffmpeg で動くので含まない。
    pub fn requires_gpu(self) -> bool {
        self.uses_gpu() && self != Backend::Auto
    }

    /// デコードに D3D12 のデバイスが要る実装か（レンダラーのデバイスを D3D12 のアダプターで作る）。
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
            "cef" => Ok(Backend::Cef),
            "cpu" => Ok(Backend::Cpu),
            "lut" => Ok(Backend::Lut),
            "ocio" => Ok(Backend::Ocio),
            "dsp" => Ok(Backend::Dsp),
            "ffmpeg" => Ok(Backend::Ffmpeg),
            "d3d12va" => Ok(Backend::D3d12va),
            "cuvid" => Ok(Backend::Cuvid),
            "nvenc" => Ok(Backend::Nvenc),
            "auto" => Ok(Backend::Auto),
            "fs" => Ok(Backend::Fs),
            other => Err(MmError::Unsupported(format!("実装 {other} は無い"))),
        }
    }
}

/// インターフェースごとの実装の選択。
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
    /// `cache: fs` の置き場所（プロジェクトの `cache/`）。
    pub cache_dir: Option<PathBuf>,
    /// レンダラーがメモリ（VRAM）に置くフレームのキャッシュの上限（バイト）。`None` は既定（1 GiB）、0 で使わない。
    pub frame_memory_bytes: Option<u64>,
    /// 使う GPU。`None` なら高性能のアダプター。レンダラー・デコーダー・別プロセスのホストが同じものを使う。
    pub gpu: Option<GpuAdapterId>,
    /// 描いたテクスチャを同じデバイスでウィンドウのサーフェスに出すか（アプリのプレビュー）。
    /// Windows では D3D12 のアダプターを選ぶ。Vulkan のアダプターは子ウィンドウのサーフェスの形式を返さないことがある。
    pub present: bool,
}

impl RuntimeConfig {
    /// 全部をモックにした設定。
    pub fn sample() -> Self {
        Self::default()
    }

    /// GPU のコンテキストを作る要る組み合わせか。レンダラー、デコーダー、文字（glyphon）のどれかが GPU を使うとき。
    pub fn needs_gpu(&self) -> bool {
        self.render.uses_gpu() || self.decode.uses_gpu() || self.text == Backend::Glyphon
    }

    /// GPU のコンテキストが作れないと組めない組み合わせか。`needs_gpu` のうち、`decode: auto` だけが理由なら偽
    /// （GPU の無い環境では ffmpeg で動く）。
    pub fn requires_gpu(&self) -> bool {
        self.render.requires_gpu() || self.decode.requires_gpu() || self.text == Backend::Glyphon
    }

    /// 実際の実装の組み合わせ。描画は wgpu、文字は cpu（1080p の字幕で glyphon より速い）、色は lut、音は dsp、
    /// デコードは auto（素材ごとに d3d12va、cuvid、ffmpeg の順。入っていない実装と GPU の無い環境では ffmpeg）、
    /// エンコードは auto（NVENC のコーデックは nvenc、それ以外は ffmpeg。NVIDIA の無い環境では `h264_nvenc` も libx264 で書く）。キャッシュはプロジェクトの置き場所（`cache_dir`）が要るのでモックのままで、
    /// ディスクに置くには `cache: Fs` と `cache_dir` を足す。
    pub fn real() -> Self {
        Self {
            render: Backend::Wgpu,
            decode: Backend::Auto,
            encode: Backend::Auto,
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
        assert_eq!("nvenc".parse::<Backend>().unwrap(), Backend::Nvenc);
        assert!("x265".parse::<Backend>().is_err());
    }

    #[test]
    fn gpu_でデコードする実装は_gpu_を使う() {
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
    fn 実際の組み合わせはデコードが_auto() {
        let real = RuntimeConfig::real();
        assert_eq!(real.decode, Backend::Auto);
        assert!(real.requires_gpu(), "レンダラーが wgpu なので GPU は必須");
    }

    #[test]
    fn gpu_の要否はレンダラーの選択で決まる() {
        assert!(!RuntimeConfig::sample().needs_gpu());
        assert!(RuntimeConfig::real().needs_gpu());
        let decode_only = RuntimeConfig {
            decode: Backend::Wgpu,
            ..RuntimeConfig::sample()
        };
        assert!(
            decode_only.needs_gpu(),
            "デコーダーが GPU を使う選択でもコンテキストは要る"
        );
    }

    #[test]
    fn auto_だけが理由なら_gpu_は必須でない() {
        let auto_only = RuntimeConfig {
            decode: Backend::Auto,
            ..RuntimeConfig::sample()
        };
        assert!(auto_only.needs_gpu() && !auto_only.requires_gpu());
        for required in [
            RuntimeConfig {
                render: Backend::Wgpu,
                decode: Backend::Auto,
                ..RuntimeConfig::sample()
            },
            RuntimeConfig {
                decode: Backend::D3d12va,
                ..RuntimeConfig::sample()
            },
            RuntimeConfig {
                text: Backend::Glyphon,
                decode: Backend::Auto,
                ..RuntimeConfig::sample()
            },
        ] {
            assert!(required.requires_gpu(), "{required:?}");
        }
    }

    #[test]
    fn 名前は往復する() {
        for b in [
            Backend::Sample,
            Backend::Wgpu,
            Backend::Glyphon,
            Backend::Cef,
            Backend::Cpu,
            Backend::Lut,
            Backend::Ocio,
            Backend::Dsp,
            Backend::Ffmpeg,
            Backend::D3d12va,
            Backend::Cuvid,
            Backend::Nvenc,
            Backend::Auto,
            Backend::Fs,
        ] {
            assert_eq!(b.as_str().parse::<Backend>().unwrap(), b);
        }
    }
}
