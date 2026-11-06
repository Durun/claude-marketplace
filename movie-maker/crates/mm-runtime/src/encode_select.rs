//! エンコードの選択肢の切り替え。NVENC を先に試し、使えなければ libx264 に落とす。
//!
//! [`AutoEncoderFactory`] は `begin` の設定のコーデックで実装を決める。`h264_nvenc` が頼まれたのに NVIDIA の GPU が無い
//! （または開けない）ときは、同じ H.264 の `h264`（libx264）で書く。HEVC は代わりが無いのでそのままエラーにする。

use std::sync::Arc;

use mm_audio_api::AudioBlock;
use mm_decode_api::{
    EncodeReport, EncodeSettings, Encoder, EncoderFactory, ExtraTracks, Nv12Frame,
};
use mm_types::{MmError, Result, RgbaImage};

/// NVENC のコーデックが使えないときの代わり。
const FALLBACKS: [(&str, &str); 1] = [("h264_nvenc", "h264")];

/// 実装の名前（設定の `encode: auto`）。
pub const AUTO_ID: &str = "auto";

/// 前から順に、そのコーデックを扱える実装を選ぶファクトリー。
pub struct AutoEncoderFactory {
    candidates: Vec<Arc<dyn EncoderFactory>>,
}

impl AutoEncoderFactory {
    /// `candidates` を前から順に試す。
    pub fn new(candidates: Vec<Arc<dyn EncoderFactory>>) -> Self {
        Self { candidates }
    }
}

fn fallback_of(codec: &str) -> Option<&'static str> {
    FALLBACKS
        .iter()
        .find(|(from, _)| *from == codec)
        .map(|(_, to)| *to)
}

impl EncoderFactory for AutoEncoderFactory {
    fn id(&self) -> &'static str {
        AUTO_ID
    }

    fn supports(&self, codec: &str) -> bool {
        let any = |name: &str| self.candidates.iter().any(|c| c.supports(name));
        any(codec) || fallback_of(codec).is_some_and(any)
    }

    fn create(&self) -> Result<Box<dyn Encoder>> {
        Ok(Box::new(AutoEncoder {
            candidates: self.candidates.clone(),
            extras: None,
            inner: None,
        }))
    }
}

/// `begin` で選んだ実装に任せるエンコーダー。
struct AutoEncoder {
    candidates: Vec<Arc<dyn EncoderFactory>>,
    /// `begin` の前に渡された字幕とチャプター。選んだ実装へ渡す。
    extras: Option<ExtraTracks>,
    inner: Option<Box<dyn Encoder>>,
}

impl AutoEncoder {
    /// `codec` を扱う実装を前から順に開く。全部だめなら最後のエラー。
    fn open(&self, settings: &EncodeSettings, codec: &str) -> Result<Box<dyn Encoder>> {
        let settings = EncodeSettings {
            codec: codec.to_owned(),
            ..settings.clone()
        };
        let mut last = MmError::Unsupported(format!("コーデック {codec} を扱える実装が無い"));
        for factory in self.candidates.iter().filter(|f| f.supports(codec)) {
            let mut encoder = factory.create()?;
            if let Some(extras) = &self.extras {
                encoder.set_extras(extras.clone())?;
            }
            match encoder.begin(&settings) {
                Ok(()) => return Ok(encoder),
                Err(e) => {
                    tracing::warn!(encoder = factory.id(), codec, error = %e, "エンコーダーを開けない。次の実装を試す");
                    last = e;
                }
            }
        }
        Err(last)
    }

    fn inner_mut(&mut self, call: &str) -> Result<&mut Box<dyn Encoder>> {
        self.inner
            .as_mut()
            .ok_or_else(|| MmError::InvalidArgument(format!("begin の前に {call}")))
    }
}

impl Encoder for AutoEncoder {
    fn set_extras(&mut self, extras: ExtraTracks) -> Result<()> {
        if self.inner.is_some() {
            return Err(MmError::InvalidArgument("begin の後に set_extras".into()));
        }
        self.extras = Some(extras);
        Ok(())
    }

    fn begin(&mut self, settings: &EncodeSettings) -> Result<()> {
        if self.inner.is_some() {
            return Err(MmError::InvalidArgument("begin を 2 回呼んだ".into()));
        }
        let first = self.open(settings, &settings.codec);
        self.inner = Some(match (first, fallback_of(&settings.codec)) {
            (Ok(encoder), _) => encoder,
            (Err(e), Some(fallback)) => {
                tracing::warn!(codec = %settings.codec, fallback, error = %e, "NVENC を使えないので libx264 で書く");
                self.open(settings, fallback)?
            }
            (Err(e), None) => return Err(e),
        });
        Ok(())
    }

    fn passes(&self) -> u32 {
        self.inner.as_ref().map_or(1, |e| e.passes())
    }

    fn begin_pass(&mut self, pass: u32) -> Result<()> {
        self.inner_mut("begin_pass")?.begin_pass(pass)
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        self.inner_mut("push_frames")?.push_frames(frames)
    }

    fn push_nv12_frames(&mut self, frames: &[Nv12Frame]) -> Result<()> {
        self.inner_mut("push_nv12_frames")?.push_nv12_frames(frames)
    }

    fn push_audio(&mut self, block: &AudioBlock) -> Result<()> {
        self.inner_mut("push_audio")?.push_audio(block)
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        let mut inner = self
            .inner
            .take()
            .ok_or_else(|| MmError::InvalidArgument("begin の前に finish".into()))?;
        inner.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_testkit::decode::SampleEncoderFactory;

    /// 名前だけで扱えるかを決める、書かないファクトリー。
    struct Named(&'static [&'static str]);

    impl EncoderFactory for Named {
        fn id(&self) -> &'static str {
            "named"
        }
        fn supports(&self, codec: &str) -> bool {
            self.0.contains(&codec)
        }
        fn create(&self) -> Result<Box<dyn Encoder>> {
            SampleEncoderFactory.create()
        }
    }

    fn auto(names: &[&'static [&'static str]]) -> AutoEncoderFactory {
        AutoEncoderFactory::new(
            names
                .iter()
                .map(|n| Arc::new(Named(n)) as Arc<dyn EncoderFactory>)
                .collect(),
        )
    }

    #[test]
    fn nvenc_が無ければ_h264_は_libx264_に落ちる() {
        let only_x264 = auto(&[&["h264", "prores_422hq"]]);
        assert!(only_x264.supports("h264_nvenc"), "h264 に落ちるので頼める");
        assert!(only_x264.supports("prores_422hq"));
        assert!(!only_x264.supports("hevc_nvenc"), "HEVC に代わりは無い");
        assert!(!only_x264.supports("vp9"));
    }

    #[test]
    fn 前の実装が扱うコーデックは前の実装を使う() {
        let both = auto(&[&["h264_nvenc"], &["h264"]]);
        assert!(both.supports("h264_nvenc") && both.supports("h264"));
    }
}
