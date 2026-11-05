# 規格への対応

| 規格 | 対応 | 段階 | 部品 | 優先度 |
|---|---|---|---|---|
| SMPTE ST 12-1 | 内部は有理数。表示と書き出しでドロップフレームを含めて変換 | 1 | `mm-types` | 必須 |
| OpenTimelineIO | `sequence` を OTIO の形に写す。`.otio` の読み書きは JSON なので Rust で自前（Rust のバインディング `opentimelineio-bind` は Linux だけ [21]）。EDL、FCPXML、AAF への変換は公式の Python のアダプターを外の道具として呼ぶ | モデルは 1、変換は 5 | `mm-doc`、外の道具 | あると良い |
| OpenColorIO 2.5、ACES 2.0 | プロセスの中で直接呼ぶ。シェーダーは GLSL を出させて naga で WGSL に。自前の `bindgen`。式だけの変換は WGSL、テクスチャを持つ変換（ACES 2.0 の表示変換）は 3D LUT に焼く（[ocio](spikes/ocio.md)）。色空間の名前 `bt709` は Rec.1886 の表示の式を指し、BT.709 の OETF は別の名前で持つ（2026-11-06 の決定） | 2 | `mm-color-ocio` | 必須 |
| ASC CDL、`.cube` | WGSL で自前 | 2 | `mm-color-lut` | 必須 |
| CLF | OCIO 経由 | 5 | `mm-color-ocio` | あると良い |
| OpenFX 1.5 | `mm-host-ofx` を Rust で書く（`openfx-sys` は C API の生のバインディングと C++ の HostSupport を含むが、Rust のホストの先行事例は見つからない [1]）。別プロセス。GPU Rendering Suite の CUDA [22] で描かせ、結果を共有テクスチャで受ける。CPU だけのプラグインは共有メモリ。パラメータは OFX の記述子から `ParamSchema` に写す | 7 | `mm-host-ofx` | あると良い |
| VST3、CLAP | 別プロセスのホスト。CLAP は `clack-host`（安全な Rust のホストのライブラリ [23]）、VST3 は `vst3` クレート（coupler-rs。C++ のヘッダーから生成した生のバインディング。SDK は別に要る [24]）の上に書く | 7 | `mm-host-audio` | あると良い |
| H.264、HEVC、AV1 | NVDEC で復号、NVENC で符号化。復号の既定は `mm-decode-d3d12va`、失敗する素材は `mm-decode-cuvid`、最後は CPU 経由（2026-11-06 の決定。[nvdec](spikes/nvdec.md)） | 0〜2 | `mm-decode-d3d12va`、`mm-decode-cuvid`、`mm-encode-nvenc` | 必須 |
| ProRes、DNxHR、MXF、DPX、OpenEXR | FFmpeg のライブラリでそのまま読む。書き出しも FFmpeg | 読みは 2、書き出しは 4 | `mm-decode-ffmpeg`、`mm-encode-ffmpeg` | 読み込みは必須 |
| PQ、HLG、Rec.2020 | 作業空間は線形の浮動小数。入力はトーンマップを明示。出力は面を `Rgba16Float` に | 後 | `mm-render` | あると良い |
| BWF、iXML | WAV のチャンクを取り込みで読む | 3 | 取り込み | 読み込みは必須 |
| ITU-R BS.1770、EBU R128 | `ebur128` クレートのメーターと、書き出しの正規化 | 2 | ミキサー | 必須 |
| SRT、WebVTT | 字幕のトラックから書き出す | 4 | 書き出し | 必須 |
| TTML、IMSC 1.2 | 同じトラックから XML で | 7 | 書き出し | あると良い |
| CEA-608、708 | 対応しない | | | 当面は不要 |
| OpenType、SVG | cosmic-text、resvg | 2 | `mm-text-glyphon`、`mm-nodes-basic` | 必須 |
| Lottie 1.0 | `lottie` の種類 | 7 | `mm-nodes-lottie` | あると良い |
| glTF | `web.cef` の Three.js で | 7 | `mm-host-cef` | あると良い |
| USD、MaterialX | 対応しない | | | 当面は不要 |
| AAF | OTIO のアダプター経由 | 5 | 外の道具 | あると良い |

OFX と VST3 を `offline` に限らなくてよい理由: 前の構成では、プラグインの結果をブラウザへ毎コマ渡すと 8 MB/コマの転送が要った。ネイティブの描画器なら、別プロセスで描いた結果を GPU の共有テクスチャのまま受けられるので、転送は要らない。`offline` は、遅いプラグインと生成 AI のための選択肢として残す。


---

出典の番号（[1] など）は [references.md](references.md) を見る。
