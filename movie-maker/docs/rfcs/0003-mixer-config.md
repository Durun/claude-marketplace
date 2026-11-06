# RFC 0003: ミキサー設定をインターフェースに上げる

状態: 採用

## 変える理由

`MixerConfig` が実装（`mm-audio-dsp`）にあり、インターフェースの `MixerFactory::create` から設定を渡せない。
設定の出どころ（プロジェクト、CLI）が実装の型に依存してしまう。
設定の型を `mm-audio-api` に置き、全実装が同じ型で受ける。

## 新しいシグネチャ

```rust
fn create(&self, sample_rate: u32, channels: u16, config: &MixerConfig) -> Result<Box<dyn Mixer>>;
```

`MixerConfig`、`BusConfig`、`DuckingConfig` は `mm-audio-api` にあり、`Serialize` / `Deserialize` を持つ。
バスの音量とマスターは全実装が効かせる。ダッキングは省いてよい。

## 影響する実装

- `mm-audio-dsp`: 型をインターフェースから使う。`DspMixerFactory` は設定を持たない unit struct。
- `mm-testkit`: モックの `SampleMixer` がバスとマスターを効かせる。契約テストがその確認を持つ。
- `mm-render`: `MixerConfig::default()` を渡す。
- `mm-runtime`: `DspMixerFactory::default()` を `DspMixerFactory` にする。

## 移行の手順

1. `mm-audio-api` に型を置き、`create` を変える。
2. 実装（dsp、モック）を直す。
3. 呼び出し側に `&MixerConfig::default()` を渡す。
4. プロジェクト設定から渡す道は別の変更で足す。
