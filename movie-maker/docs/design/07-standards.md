# 規格への対応

| 規格 | 対応 | 段階 | 部品 | 優先度 |
|---|---|---|---|---|
| SMPTE ST 12-1 | 内部は有理数。表示と書き出しでドロップフレームを含めて変換 | 1 | `mm-types` | 必須 |
| OpenTimelineIO | `sequence` を OTIO の形に変換する。`.otio` の読み書きは JSON なので Rust で自前（Rust のバインディング `opentimelineio-bind` は Linux だけ [21]）。対応は下の「OTIO との対応」。`mm otio export` / `mm otio import`。EDL、FCPXML、AAF への変換は公式の Python のアダプターを外のツールとして呼ぶ | モデルは 1、変換は 5 | `mm-doc`（`otio`）、`mm`、外のツール | あると良い |
| OpenColorIO 2.5、ACES 2.0 | プロセスの中で直接呼ぶ。シェーダーは GLSL を出させて naga で WGSL に。自前の `bindgen`。式だけの変換は WGSL、テクスチャを持つ変換（ACES 2.0 の表示変換）は 3D LUT にベイクする（[ocio](spikes/ocio.md)）。色空間の名前 `bt709` は Rec.1886 の表示の式を指し、BT.709 の OETF は別の名前で持つ（2026-11-06 の決定） | 2 | `mm-color-ocio` | 必須 |
| ASC CDL、`.cube` | WGSL で自前 | 2 | `mm-color-lut` | 必須 |
| CLF | OCIO 経由 | 5 | `mm-color-ocio` | あると良い |
| OpenFX 1.5 | `mm-host-ofx` を Rust で書く（`openfx-sys` は C API の生のバインディングと C++ の HostSupport を含むが、Rust のホストの先行事例は見つからない [1]）。別プロセス。GPU Rendering Suite の CUDA [22] で描かせ、結果を共有テクスチャで受ける。CPU だけのプラグインは共有メモリ。パラメータは OFX の記述子から `ParamSchema` に変換する | 7 | `mm-host-ofx` | あると良い |
| VST3、CLAP | 別プロセスのホスト。CLAP は `clack-host`（安全な Rust のホストのライブラリ [23]）、VST3 は `vst3` クレート（coupler-rs。C++ のヘッダーから生成した生のバインディング。SDK は別に要る [24]）の上に書く | 7 | `mm-host-audio` | あると良い |
| H.264、HEVC、AV1 | NVDEC でデコード、NVENC でエンコード。デコードの既定は `mm-decode-d3d12va`、失敗する素材は `mm-decode-cuvid`、最後は CPU 経由（2026-11-06 の決定。[nvdec](spikes/nvdec.md)） | 0〜2 | `mm-decode-d3d12va`、`mm-decode-cuvid`、`mm-encode-nvenc` | 必須 |
| ProRes、DNxHR、MXF、DPX、OpenEXR | FFmpeg のライブラリでそのまま読む。書き出しも FFmpeg | 読みは 2、書き出しは 4 | `mm-decode-ffmpeg`、`mm-encode-ffmpeg` | 読み込みは必須 |
| PQ、HLG、Rec.2020 | 作業色空間は線形の浮動小数。入力はトーンマップを明示。出力はサーフェスを `Rgba16Float` に | 後 | `mm-render` | あると良い |
| BWF、iXML | WAV のチャンクを取り込みで読む | 3 | 取り込み | 読み込みは必須 |
| ITU-R BS.1770、EBU R128 | `ebur128` クレートのメーターと、書き出しの正規化（統合ラウドネスの目標、-1 dBTP のトゥルーピークの上限） | 2、書き出しは 4 | ミキサー | 必須 |
| SRT、WebVTT | sequence のカットの文字起こしから書き出す。MP4 の字幕のトラックは mov_text | 4 | 書き出し | 必須 |
| TTML、IMSC 1.2 | 同じトラックから XML で | 7 | 書き出し | あると良い |
| CEA-608、708 | 対応しない | | | 当面は不要 |
| OpenType、SVG | cosmic-text、resvg | 2 | `mm-text-glyphon`、`mm-nodes-basic` | 必須 |
| Lottie 1.0 | `lottie` のノードの種類 | 7 | `mm-nodes-lottie` | あると良い |
| glTF | `web.cef` の Three.js で | 7 | `mm-host-cef` | あると良い |
| USD、MaterialX | 対応しない | | | 当面は不要 |
| AAF | OTIO のアダプター経由 | 5 | 外のツール | あると良い |

OFX と VST3 を `offline` に限らなくてよい理由: 前の構成では、プラグインの結果をブラウザへ毎フレーム渡すと 8 MB/フレームの転送が要った。ネイティブのレンダラーなら、別プロセスで描いた結果を GPU の共有テクスチャのまま受けられるので、転送は要らない。`offline` は、遅いプラグインと生成 AI のための選択肢として残す。


## OTIO との対応

実装は `mm-doc` の `otio` モジュール。1 つの `sequence` が `Timeline` の映像トラック 1 本（名前 `V1`）になる。書くスキーマは Timeline.1、Stack.1、Track.1、Clip.1、Gap.1、Transition.1、ExternalReference.1、GeneratorReference.1、LinearTimeWarp.1、Marker.2（Python の opentimelineio 0.18.1 で読めることを確かめた。読むときはバージョン番号を見ないので、Clip.2 や Marker.3 のファイルも読める）。

| movie-maker | OTIO | 補足 |
|---|---|---|
| `Sequence` | `Timeline` > `Stack` > `Track`（`kind: Video`） | 取り込みは映像のトラック 1 本が `sequence` 1 つ。音声のトラックは警告を出して取り込まない |
| `Cut`（撮影素材） | `Clip` + `ExternalReference` | `target_url` は `file:///` の URL（空白と非 ASCII は `%` 符号化）。取り込みで戻す |
| `Cut` の `in` と `speed` | `source_range.start_time` と `LinearTimeWarp.time_scalar` | `source_range.duration` は配置の長さ `(out - in) / speed`。OTIO の読み手が配置を正しく並べられる。素材の出は `in + 長さ × 速さ` |
| 隙間のカット | `Gap` | 長さは配置の長さ |
| 外したカット（`enabled: false`） | `enabled: false` で長さ 0 の `Clip` | OTIO は無効な項目も長さを数えるので、長さ 0 にして配置を合わせる。別のツールが無効にした長さのある項目は、長さを保つ隙間にする |
| `transitionIn` | `Transition`（`SMPTE_Dissolve`）を前のカットとの間に置く | 編集点を中心に前後へ半分ずつ（`in_offset` = `out_offset` = 長さの半分）。取り込みは 2 つの和を長さにする。ディゾルブ以外の種類は種類の名前を `transition_type` に書き、取り込みはメタデータが無ければディゾルブにして警告 |
| 撮影素材でないノード（単色、入れ子） | `GeneratorReference`（`generator_kind` = ノードの種類） | ノード全体を `metadata` に持つ。メタデータが無い生成器と、素材が無い `MissingReference` は隙間にして警告 |
| `Asset` | `ExternalReference` | `available_range` は素材の長さ（fps が可変なら sequence の fps）。取り込みは同じ場所の素材があれば再利用し、無ければ足す（コンテンツハッシュは空、色空間は `bt709`） |
| コンポジションのマーカー | `Track.markers`（`Marker`） | sequence を置いたレイヤーの `start`、`in`、`speed` でトラックの時間軸に直す。クリップのマーカーは配置の時刻に直して取り込む。同じ時刻は 1/1000 秒ずつ後ろへずらす |
| 時刻 | `RationalTime`（`value`、`rate`） | 素材の fps の整数フレームに乗る時刻は `value` = フレーム数、乗らない時刻は `value` = 分子、`rate` = 分母。読むときは連分数で戻す（29.97 は 30000/1001） |
| 効果（OTIO 側の `Effect`、`LinearTimeWarp` 以外） | 取り込まない | 警告を出す |
| 入れ子の `Track`、`Stack` | 取り込まない | 長さを保つ隙間にして警告 |

movie-maker にしか無い値は、各オブジェクトの `metadata["movie-maker"]` に入れる。Python の opentimelineio で読み書きしても残る。

| 場所 | 入れる値 |
|---|---|
| `Track` | `sequence`（ID）、`origin`（ショートの元の `sequence`）、`composition`（sequence を置いたコンポジション全体。マーカーは `Track.markers` に出すので空） |
| `Clip`、`Gap` | `id`、`in`、`out`、`speed`、`enabled`（厳密な有理数）、`node`（撮影素材だけのノードでないとき）、`transition_in`、`origin`、`overrides`、`origin_base` |
| `ExternalReference` | `asset`（`Asset` 全体） |
| `Marker` | `id` |

メタデータは OTIO の値のコピーで、別のツールが `source_range` や `time_scalar` を変えたときは OTIO の値に従う（ID と、ショートとの関係は残す）。メタデータを使うと、`mm otio export` → `mm otio import` で `sequence`、素材、コンポジション、ショートとの関係が元のプロジェクトのデータと等しく戻る（テストで確かめている）。

取り込みは `addAsset`、`addSequence`、`addComposition` を 1 つのトランザクションで適用し、`mm undo` で取り消せる。同じ ID の sequence とコンポジションがあれば拒否し、`--as <ID>` でメタデータの ID を使わず作り直せる。

テストは、OpenTimelineIO のリポジトリの `tests/sample_data/` の実例（`fixtures/otio/official/`）と、Python の opentimelineio が書いたファイル（`fixtures/otio/python/`、`fixtures/otio/generate.py` で作り直す）を読み、Python が Rust の書いたファイルを読んで書き直したものを Rust が読み戻す（`fixtures/otio/roundtrip.py`）。CI は opentimelineio を入れて `MM_REQUIRE_OTIO=1` で必ず実行する。

---

出典の番号（[1] など）は [references.md](references.md) を見る。
