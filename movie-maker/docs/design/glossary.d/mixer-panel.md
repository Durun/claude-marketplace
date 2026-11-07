## 再生と Timeline

| 用語 | 定義 | 識別子 |
|---|---|---|
| ミキサーの設定 | コンポジションの、役割ごとのバスの音量、マスターの音量、ダッキング | `CompMixer`、`Composition.mixer`、`setMixer` |
| バス | 同じ役割のレイヤーの音を足す道。役割の名前で呼ぶ（`voice`、`bgm`、`sfx`） | `MixerBus`、`LayerAudio.role` |
| 送り先のバス | レイヤーの音を足すバス。レイヤーの音の設定の役割 | `LayerAudio.role` |
| トラック（Mixer の） | Mixer の 1 行。音を持てるレイヤー（撮影素材、入れ子、sequence）1 つ | `MixerTrack` |
| ダッキング | きっかけのバスが鳴っている間、対象のバスを下げること | `Ducking`、`DuckingConfig` |
| ミュート / ソロ | レイヤーの音を鳴らさない / ソロのレイヤーだけを鳴らす。プレビューと書き出しの両方に効き、ソロはそのコンポジションの中だけで効く | `LayerAudio.mute`、`solo`、`mm_eval::audible` |
| トラックのメーター | 音量とパンの後のピーク | `TrackLevel`、`Mixer::input_peaks`、`Renderer::audio_levels` |
| マスターのメーター | ミキサーの出力のピーク、短期・統合ラウドネス、トゥルーピーク。シークで測り直す | `MasterMeter`、`MasterReading`、`mm:mixer-meters` |
