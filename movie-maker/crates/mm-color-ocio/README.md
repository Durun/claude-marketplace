# mm-color-ocio

OpenColorIO で `mm-color-api::ColorTransform` を実装する。結果と数字は
[docs/design/spikes/ocio.md](../../docs/design/spikes/ocio.md)。

## OCIO が無い環境（既定）

何も要らない。`cargo build` / `cargo test --workspace` は OCIO を探さない。
このクレートは GLSL → WGSL の変換（`glsl` モジュール、naga）だけを持ち、
`tests/it/` の OCIO の試験は feature `native` が無いので空になる。

## OCIO を使う（feature `native`）

入れるもの（Windows 11、MSVC）:

1. OCIO 2.6.0。vcpkg のマニフェスト（このフォルダの `vcpkg.json`）でバージョンを固定する。
   `builtin-baseline` で vcpkg のコミットを、`overrides` で OCIO のバージョンを固定しているので、誰が入れても同じ 2.6.0 になる。
   ```bash
   git clone https://github.com/microsoft/vcpkg.git /d/dev/vcpkg
   /d/dev/vcpkg/bootstrap-vcpkg.bat -disableMetrics
   cd crates/mm-color-ocio
   /d/dev/vcpkg/vcpkg.exe install --triplet x64-windows --x-manifest-root=. --x-install-root=/d/dev/mm-ocio-vcpkg
   ```
   `builtin-baseline` のコミットは完全なクローンにしか無い（`--depth 1` にしない）。
   バージョンは 2.6.0 とする。設計書の 2.5 は下限で、vcpkg の最新が 2.6.0 であり、試作と試験は 2.6.0 で通した。
   組み込み設定は `ocio://studio-config-latest`（2.5 以上にある）を使う。`ocio://default` の CG 設定には
   `bt709_oetf` に対応する `Camera Rec.709` が無い。
2. LLVM（bindgen が `libclang.dll` を使う）。`winget install LLVM.LLVM`

環境変数:

| 名前 | 値 |
|---|---|
| `OCIO_ROOT` | `/d/dev/mm-ocio-vcpkg/x64-windows`（上の `--x-install-root` の下。`include`、`lib`、`bin` を持つ所） |
| `LIBCLANG_PATH` | `libclang.dll` のある所（`C:\Program Files\LLVM\bin`） |

実行:

```bash
export OCIO_ROOT=/d/dev/mm-ocio-vcpkg/x64-windows
export LIBCLANG_PATH="/c/Program Files/LLVM/bin"
cargo test -p mm-color-ocio --features native               # 契約、精度（CPU とベイクした LUT）
cargo test -p mm-color-ocio --features native,gpu-eval -- --nocapture   # WGSL を GPU で実行して OCIO の CPU と比べる
cargo test --release -p mm-color-ocio --features native --test it timing -- --nocapture  # スパンで時間を測る
```

- `build.rs` が `OpenColorIO*.dll` などを `target/<profile>/` と `deps/` にコピーするので、PATH は触らなくてよい。
- `program(from, ColorSpace("display"))` は設定の既定の display と view で見る変換。ACES 2.0 のトーンマップを含むので `Lut3d`（既定 33³）で返り、`known_spaces` には出ない。
- `gpu-eval` は wgpu で高性能側のアダプターを探し、無ければその試験をスキップする。
- `native` を付けて `OCIO_ROOT` が無いとビルドが止まり、理由を表示する。
