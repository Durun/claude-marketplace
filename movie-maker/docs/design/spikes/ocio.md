# 難所の試作: OCIO のシェーダー

[15-roadmap.md](../15-roadmap.md#段階-0-で最初に確かめる難所) の 5 行目。コードは `crates/mm-color-ocio`（本命）と `crates/mm-color-lut`（逃げ道）。

## 結論

| 問い | 答え |
|---|---|
| OCIO を `bindgen` で Rust から呼べるか | 呼べる。OCIO は C++ の API だけなので、C の薄い橋（`native/mm_ocio.{h,cpp}`、約 150 行）を `build.rs` で `cc` ビルドし、その C のヘッダーに `bindgen` をかける。Windows 11 + MSVC で通った |
| GLSL を naga で WGSL にできるか | 式だけのシェーダー（行列、ガンマ、sRGB の曲線）は通る。ACES 2.0 の表示変換のようにテクスチャを持つシェーダーは通らない（下記） |
| WGSL と OCIO の CPU は合うか | 合う。GPU（wgpu、AMD Radeon）で実行して最大差 2.4e-5（許容差 2e-4） |
| 逃げ道は要るか | テクスチャ付きの変換には要る。CPU の変換を 3D LUT に焼いて `TransformProgram::Lut3d` で返す |

実装の方針: `OcioColor::program` は、OCIO のシェーダーがテクスチャも動的な値も持たなければ naga で WGSL にし、持つ（または naga が読めない）ときは 3D LUT に焼く。呼ぶ側は `TransformProgram` の型で見分ける。

## 実測

環境: Windows 11、MSVC 2022、Rust 1.95、OCIO 2.6.0（vcpkg）、naga / wgpu 30.0.1、GPU は AMD Radeon（内蔵）。

### 精度（WGSL を GPU で実行 vs OCIO の CPU、10 色）

見本の色は黒、18% グレー、白、原色、暗部（0.003）、彩度の高い色など。

| 変換 | OCIO のシェーダー | 最大差 |
|---|---|---|
| sRGB → 線形 Rec.709 | 式のみ | 2.4e-5 |
| 線形 Rec.709 → sRGB | 式のみ | 6.7e-6 |
| ACEScg → 線形 Rec.709 | 行列 | 0（f32 で一致） |
| 線形 Rec.709 → Rec.709 表示（Rec.1886） | 式のみ | 6.3e-6 |
| ACEScg → sRGB 表示 | 行列 + 曲線 | 8.1e-6 |
| ACES2065-1 → 線形 Rec.709 | 行列 | 1.2e-7 |
| ACEScg → 既定の表示（ACES 2.0 SDR）を 65³ に焼いた LUT | テクスチャ 2 つ | 1.3e-3 |

- OCIO の sRGB と `mm-color-lut` の自前の sRGB の差は最大 2.4e-5。
- 焼いた LUT の 1.3e-3 は線形補間の誤差で、格子を 33 → 65 に細かくすると縮む。入力は 0..1 に切られるので、1 を超える HDR の値は焼けない。
- 口の契約（`mm_testkit::color::contract`）は OCIO でも `mm-color-lut` でも通る。

### 時間（`mm-trace` のスパン、release、20 回）

| スパン | p50 | p95 | 備考 |
|---|---|---|---|
| `color_ocio.load_config`（組み込みの `ocio://default`） | 6.4〜9.5 ms | 8.4〜12.5 ms | 初回の最大 13.6 ms。起動時に 1 回 |
| `color_ocio.make_processor` | 0.15 ms | 3.6〜6.0 ms | p95 は ACES の表示変換 |
| `color_ocio.gen_glsl` | 0.08〜0.1 ms | 4.2〜14 ms | 同上。式だけの変換は 0.1 ms 前後 |
| `color_ocio.naga_convert` | 0.23〜0.3 ms | 0.4〜0.7 ms | 初回の最大 0.8〜21 ms |
| `color_ocio.bake_lut`（33³、ACES の表示変換） | 13〜15 ms | 14〜18 ms | 3.6 万回の FFI 呼び出しを含む |

2 回の実行の幅を書いた。式だけの変換は、設定を読んだ後の変換の作成から WGSL まで 1 ms 未満。ACES 2.0 の表示変換は、シェーダーの生成と LUT に焼く分で 30 ms 前後（クリップの色空間を変えたときに 1 回）。設定の読み込みは起動時に 1 回で済み、`17-instrumentation.md` の起動の予算に足してよい大きさ。

## 詰まった所

1. **naga はテクスチャ付きの OCIO のシェーダーを読めない。** OCIO は `layout(set=0, binding=1) uniform sampler1D ...` と結合サンプラーで出し、naga の GLSL 入力はこれを `NotImplemented("variable qualifier")` で断る。`texture2D` + `sampler` に分けた形は通る（`glsl.rs` の試験）。結合サンプラーをこの形に書き換えれば通る見込みだが、口の `TransformProgram::Wgsl` にはテクスチャの置き場が無いので、試作では 3D LUT に焼く道を取った。ACES 2.0 の表示変換（`ACES 2.0 - SDR 100 nits (Rec.709)`）は 1D テクスチャ 2 つ（`reach_m_table`、`gamut_cusp_table`）を持つ。
2. **naga の GLSL 入力は入口（`main`）を要る。** OCIO は関数だけを出すので、空の `main` を足して読ませ、入口を捨てて書き出し、`mm_color_transform` の包みを足す（`glsl.rs`）。
3. **契約の「白は白」はログや PQ の空間に合わない。** 契約は全 `known_spaces` で符号化値 (1,1,1) が線形の白になることを確かめるが、ACEScc / ACEScct の 1.0 は線形で約 223、PQ の 1.0 は 10000 nit になる。`OcioColor::known_spaces` はこれらを外した。OCIO の名前は `program` / `apply` ならそのまま通る。
4. **`bt709` の定義が 2 つある。** OCIO 側は映像の Rec.709 を表示側の Rec.1886（ガンマ 2.4、`Rec.1886 Rec.709 - Display`）に当て、`mm-color-lut` 側は BT.709 の OETF（見本は sRGB 曲線）。暗部で数 % ずれる。どれを `bt709` とするかは `07-standards.md` で決める必要がある。
5. **vcpkg が powershell-core の 7.6.9 を GitHub から取れなかった**（504）。`winget install Microsoft.PowerShell` で入れて `%LOCALAPPDATA%\Microsoft\WindowsApps` を PATH に足すと通った。

## 入れ方

`crates/mm-color-ocio/README.md` に手順と環境変数がある。要点:

- OCIO: `vcpkg install opencolorio:x64-windows`（2.6.0。ロードマップの 2.5 以上で、組み込み設定 `ocio://default` は 2.6 では CG 設定 v5 / ACES 2.1）。ビルドは 1.6 分。
- bindgen のために LLVM（`winget install LLVM.LLVM`）と `LIBCLANG_PATH`。
- `OCIO_ROOT` に vcpkg の `installed/x64-windows`。`cargo test -p mm-color-ocio --features native`。
- OCIO が無い既定の環境は feature を付けない。変換器の試験と `mm-color-lut` だけが走る。
- DLL は `build.rs` が `target/<profile>/` に写す。配布では `OpenColorIO_2_6.dll` と依存の 7 つ（`yaml-cpp`、`Imath`、`libexpat`、`z`、`zstd`、`liblzma`、`bz2`）を同梱する。静的リンク（`x64-windows-static-md`）は試していない。

## 逃げ道（`mm-color-lut`）

OCIO なしで契約を通す。伝達関数（sRGB、BT.709）の WGSL、ASC CDL の WGSL、`.cube` の読み込みと 3D LUT を引く WGSL（`textureLoad` で三線形補間、サンプラー不要）、任意の変換の 3D LUT への焼き込み（`bake_lut3d`）を持つ。WGSL を GPU で実行して CPU と合わせる試験は `--features gpu-eval`（最大差 1e-4 未満）。

## 次の判断

- ACES 2.0 の表示変換を WGSL で出したい場合: 結合サンプラーの書き換えと、`TransformProgram` にテクスチャを持たせる口の変更（下）。焼いた LUT で足りるなら変更は要らない（誤差 1e-3 台）。
- 口への提案: `contract` の白の確認を、符号化値が (1,1,1) で白になる空間に限る（または空間ごとに期待値を持たせる）。
