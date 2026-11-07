# RFC 0075: Mixer パネルのために足すインターフェース

状態: 採用

足すだけで、既存のメソッドと型は変えない。

## 先行事例

- トラックのメーターはフェーダーとパンの後（post-fader）のピーク。DaVinci Resolve の Fairlight、Premiere の Audio Track Mixer、Reaper の既定と同じ。
- マスターのラウドネスは EBU R128 の短期（3 秒）と統合、トゥルーピーク。測りは `ebur128`（`mm-audio-dsp` が既に使う）。

## 足すもの

`mm-audio-api`:

```rust
// Mixer に既定の実装つきで足す。直前の mix の入力ごとのピーク（音量とパンの後）。測らない実装は空。
fn input_peaks(&self) -> &[f32] { &[] }
// MixerFactory に既定の実装つきで足す。対応しない実装（モック）は None のまま。
fn master_meter(&self, sample_rate: u32, channels: u16) -> Option<Box<dyn MasterMeter>> { None }
pub struct MasterReading { pub peak: Vec<f32>, pub true_peak: Option<f32>, pub short_term_lufs: Option<f32>, pub integrated_lufs: Option<f32> }
pub trait MasterMeter: Send {
    fn feed(&mut self, block: &AudioBlock) -> Result<MasterReading>;
    fn reset(&mut self);
}
```

`mm-render-api`:

```rust
pub struct TrackLevel { pub layer: LayerId, pub peak: f32 }
// Renderer に既定の実装つきで足す。直前の render_audio の、要求したコンポジションの直下のレイヤーごとのピーク。
fn audio_levels(&self) -> Vec<TrackLevel> { Vec::new() }
// プロジェクトのデータのミキサーの設定をミキサーの設定に変える。レンダラーの実装が同じ変換を使う。
pub fn mixer_config(mixer: Option<&CompMixer>, ducking: bool) -> MixerConfig
```

`mm-doc`（名前つきの移行 `20261107_mixer`）: `Composition.mixer`（`CompMixer { master_db, buses, ducking }`）と `LayerAudio.mute`・`solo`。
`serde(default)` なので移行の前の JSON はミキサーなし、ミュートもソロもなしで読める。

## 影響する実装

- `mm-audio-dsp`: `DspMixer::input_peaks`、`DspMasterMeter`。
- `mm-render`、`mm-testkit` のレンダラー: `mixer_config` で混ぜ、ミュートとソロのレイヤーを外す（`mm_eval::audible`）。
  `mm-render` は `audio_levels` を返す。
- `mm-runtime`: `Engine::audio_levels`。
