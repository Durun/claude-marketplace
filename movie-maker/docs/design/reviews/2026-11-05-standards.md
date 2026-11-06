# 業界標準の規格の調査（2026-11-05）

〔確〕は Web で確認した事実、〔知〕は調べた人の知識で出典は未取得、〔推〕は推測。実装の前に公式で確かめ直す。

| 規格 | 中身 | ライセンス | ブラウザの中で | 優先度の意見 | 出典 |
|---|---|---|---|---|---|
| OpenFX 1.5（2024-09） | 映像の効果のプラグインの API。Resolve、Fusion、Flame、Nuke、Natron が対応。1.5 で GPU Rendering Suite（Metal、CUDA、OpenCL） | BSD-3 | 不可。C の ABI の DLL と、CUDA・OpenCL が前提。ネイティブのホストが要る | あると良い（v1 は不要の意見） | https://openfx.readthedocs.io/en/main/ReleaseNotes/relnotes-1.5.html |
| OpenColorIO 2.5（2025-09） | 色の変換の API。ACES 2.0 の設定を内蔵。GPU のシェーダー（GLSL、GLSL ES、HLSL、MSL）と LUT を生成できる〔知〕 | BSD-3〔知〕 | WASM 版は見つからない。ネイティブか Python でシェーダーと 3D LUT を作ってブラウザに渡す形〔推〕 | 必須 | https://opencolorio.readthedocs.io/en/v2.5.1/ |
| ACES 2.0（2025-04、ASWF へ移管） | 色の管理の体系。AMF は変換の連鎖を書く XML | 自由〔知〕 | OCIO の設定で使う | 必須（AMF は後） | https://acescentral.com/?p=5016 |
| ASC CDL | 傾き・オフセット・べき・彩度の色補正の式 | 自由〔知〕 | GLSL で自前で書ける | 必須 | |
| .cube、CLF（ST 2136-1） | LUT の形式 | 自由〔知〕 | .cube は WebGL2 の 3D テクスチャで自前で書ける。CLF は OCIO が読む | .cube は必須、CLF はあると良い | |
| OpenTimelineIO | 編集の交換形式（ASWF）。.otio は JSON。C++ と Python。アダプター: CMX3600 EDL、FCP7 XML、FCPXML、AAF（pyaaf2。マーカーと効果は未対応） | Apache 2.0〔知〕 | 非公式の JS 実装（npm `opentimelineio`）は .otio の読み書きだけ。otio-wasm は実験。変換はサーバーの Python に任せる〔推〕 | あると良い。内部のモデルを OTIO（RationalTime、Track、Clip、Gap、Transition）と対応させて作る | https://opentimelineio.readthedocs.io/en/v0.15/tutorials/adapters.html |
| SMPTE ST 12-1 | タイムコード。29.97 のドロップフレーム | 仕様書は有料、式は公開〔知〕 | TS で自前で書ける | 必須。内部の時刻は有理数 | |
| H.264、HEVC、AV1 | 配信と収録のコーデック | HEVC は特許プール〔知〕 | WebCodecs で読める。HEVC は Chrome M130 から Windows で有効（ハードウェア次第） | 必須 | https://groups.google.com/a/chromium.org/g/blink-dev/c/YJ1QijNiHeM |
| ProRes | 中間コーデック | 仕様は非公開。FFmpeg は逆解析の実装〔知〕 | WebCodecs は不可。WASM のデコーダー TurboRes と mediabunny の拡張（1080p で約 42 fps と自称） | 読み込みは必須。書き出しは FFmpeg | https://mediabunny.dev/guide/extensions/prores |
| DNxHR、DPX、MXF | 中間コーデックと入れ物 | | WebCodecs は不可。FFmpeg は可 | あると良い | |
| OpenEXR | 浮動小数の画像（VFX） | BSD-3 | WASM 版、tinyexr、exrjs がある | あると良い（VFX なら必須） | |
| PQ（ST 2084）、HLG、Rec.2020 | HDR | 式は自由。Dolby Vision は商用 | WebGPU の canvas を rgba16float と `toneMapping: extended` にすると白を超えて出せる（Chrome 138〜）。WebGL2 の canvas は SDR のまま | あると良い。内部の作業色空間は最初から線形の浮動小数にする | https://developer.chrome.com/blog/new-in-webgpu-129 |
| VST3 | 音の効果のプラグイン | 2025-11 に MIT へ変わったという報告（二次情報、公式は未確認） | 不可。ネイティブのホスト。Node のホスト audiojs/host はプラグインが落ちるとホストごと落ちる | あると良い（ネイティブの補助プロセスが前提） | https://github.com/audiojs/host |
| CLAP | 音の効果のプラグイン | MIT | WCLAP（WASM を AudioWorklet で）の試みがある（発展途上） | あると良い | https://github.com/WebCLAP |
| BWF、iXML | 放送用の WAV のメタデータ | 自由〔知〕 | WAV のチャンクの解析なので自前で書ける | 読み込みは必須 | |
| ITU-R BS.1770、EBU R128 | ラウドネス | libebur128 は MIT | `ebur128-wasm`（Apache-2.0）。書き出しでは FFmpeg の `ebur128`、`loudnorm` も | 必須 | https://github.com/jiixyj/libebur128 |
| SRT、WebVTT | 字幕 | 自由 | 自前で書ける | 必須 | |
| TTML、IMSC 1.2（2020 W3C 勧告） | 放送・配信の納品の字幕 | W3C | XML なので JS で扱える | あると良い | https://www.w3.org/TR/2020/REC-ttml-imsc1.2-20200804/ |
| CEA-608、708 | 北米の放送の埋め込み字幕 | | FFmpeg などネイティブが要る | 当面は不要 | |
| OpenType、SVG | フォント、ベクター画像 | | ブラウザが扱える | 必須 | |
| Lottie 1.0 | ベクターのアニメーション。Lottie Animation Community（Linux Foundation の配下）が仕様を作り、IANA に登録（`video/lottie+json`） | | lottie-web などで描ける | あると良い | https://www.linuxfoundation.org/press/announcing-lottie-animation-community |
| glTF | 3D の受け渡し（Khronos） | | Three.js が読める | あると良い | |
| USD、MaterialX | 3D の場面とシェーダーのグラフ | | USD は v26.03 で WASM のビルド。MaterialX は未確認 | 当面は不要 | https://aousd.org/blog/openusd-v26-03/ |

## ブラウザの描画のままでは対応できない規格

OFX、VST3、ProRes・DNxHR・MXF・DPX のデコード（WASM のデコーダーか FFmpeg が要る）、OCIO の本体、AAF と CEA-608・708、HDR の PQ・HLG の出力（WebGPU の拡張のモードだけ）。

## 先行事例のある構成

- **A. ブラウザで描画 + ネイティブの補助プロセス**: 取り込みで FFmpeg がプロキシを作り、ブラウザで編集する。重い処理（OFX、VST3、OCIO のシェーダーの生成、OTIO の変換、書き出し）はネイティブが受け持つ。MasterSelects は WebGPU の描画と、小さな Rust の補助プロセスを組み合わせている〔確〕。
- **B. Rust の共通の核 + wgpu で、ネイティブと WASM の両方**: OpenCut は GPU の合成、効果、マスクを含む Rust の核に WASM の口を付け、ネイティブの版も作っている（MIT、TypeScript から移行中）〔確〕。同じ描画をネイティブでも動かせ、OFX、OCIO、FFmpeg をネイティブ側で直接組み込める。移行の手間が大きく、成熟した先行事例は少ない。
- **C. ネイティブのレンダラーがホストを持つ従来の形**: Resolve、Nuke の形。OFX のホストの実装は Natron（GPLv2）が参考になるが、流用すると GPL が及ぶ。
- 調べた人の意見〔推〕: A で始め、時刻（有理数）と色（線形の浮動小数）のモデルを規格に合わせておけば、後で B に移れる。
