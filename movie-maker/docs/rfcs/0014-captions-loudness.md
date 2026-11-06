# RFC 0014: 字幕・チャプター・ラウドネスの正規化のために足すインターフェース

状態: 採用

足すだけで、既存のメソッドと型は変えない。

## 先行事例

- ラウドネスの正規化: ffmpeg の `loudnorm`（2 パスで測ってから倍率をかける）、EBU R128 の運用（統合ラウドネスと -1 dBTP）。
  測りは `ebur128` クレート（`mm-audio-dsp` が既に使う）。
- 字幕とチャプターのトラック: FFmpeg の mov_text と `AVChapter`（`ffmpeg-next` の `Output::add_chapter`）。

## 足すもの

`mm-audio-api`:

```rust
pub struct NormalizeSpec { pub target_lufs: f32, pub ceiling_dbtp: f32 }
pub struct NormalizeReport { measured_lufs, final_lufs, gain_db, true_peak_dbtp, limited }
pub trait LoudnessNormalizer: Send {
    fn normalize(&self, block: &mut AudioBlock, spec: &NormalizeSpec) -> Result<NormalizeReport>;
}
// MixerFactory に既定の実装つきで足す。対応しない実装（モック）は None のまま。
fn loudness_normalizer(&self) -> Option<Box<dyn LoudnessNormalizer>> { None }
```

`mm-decode-api`:

```rust
pub struct SubtitleCue { pub start: RationalTime, pub end: RationalTime, pub text: String }
pub struct ChapterMark { pub start: RationalTime, pub end: RationalTime, pub title: String }
pub struct ExtraTracks { pub subtitles: Vec<SubtitleCue>, pub chapters: Vec<ChapterMark> }
// Encoder に既定の実装つきで足す。`begin` の前に呼ぶ。対応しない実装は Unsupported を返す。
fn set_extras(&mut self, extras: ExtraTracks) -> Result<()>
```

`mm-doc`（スキーマ v7）: `RenderTarget.delivery`（`Delivery { subtitles, chapters, true_peak_dbtp }`）。
`serde(default)` なので v6 の JSON は何も添えない設定で読める。

## 影響する実装

- `mm-audio-dsp`: `DspLoudnessNormalizer`（測り、倍率、先読みのリミッター）。`DspMixerFactory` が返す。
- `mm-encode-ffmpeg`: `set_extras` で mov_text のトラックと MP4 のチャプターを書く。
- `mm-export`: `ExportOptions::transcript`、`ExportSummary::delivery`。
- `mm-testkit` のモックは変えない（既定の実装で足りる）。
