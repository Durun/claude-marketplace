# mm-color-ocio

OpenColorIO で `mm-color-api::ColorTransform` を実装する。結果と数字は
[docs/design/spikes/ocio.md](../../docs/design/spikes/ocio.md)。

## OCIO が無い環境（既定）

何も要らない。`cargo build` / `cargo test --workspace` は OCIO を探さない。
このクレートは GLSL → WGSL の変換（`glsl` モジュール、naga）だけを持ち、
`tests/contract.rs` などの OCIO の試験は feature `native` が無いので空になる。

## OCIO を使う（feature `native`）

入れるもの（Windows 11、MSVC）:

1. OCIO。vcpkg で入れる（2.6.0 が入る。2.5 以降の組み込み設定 `ocio://default` を使う）。
   ```bash
   git clone --depth 1 https://github.com/microsoft/vcpkg.git /d/dev/vcpkg
   /d/dev/vcpkg/bootstrap-vcpkg.bat -disableMetrics
   /d/dev/vcpkg/vcpkg.exe install opencolorio:x64-windows
   ```
2. LLVM（bindgen が `libclang.dll` を使う）。`winget install LLVM.LLVM`

環境変数:

| 名前 | 値 |
|---|---|
| `OCIO_ROOT` | vcpkg の `installed/x64-windows`（`include`、`lib`、`bin` を持つ所） |
| `LIBCLANG_PATH` | `libclang.dll` のある所（`C:\Program Files\LLVM\bin`） |

実行:

```bash
export OCIO_ROOT=/d/dev/vcpkg/installed/x64-windows
export LIBCLANG_PATH="/c/Program Files/LLVM/bin"
cargo test -p mm-color-ocio --features native               # 契約、精度（CPU と焼いた LUT）
cargo test -p mm-color-ocio --features native,gpu-eval -- --nocapture   # WGSL を GPU で実行して OCIO の CPU と比べる
cargo test --release -p mm-color-ocio --features native --test timing -- --nocapture  # スパンで時間を測る
```

- `build.rs` が `OpenColorIO*.dll` などを `target/<profile>/` と `deps/` に写すので、PATH は触らなくてよい。
- `gpu-eval` は wgpu でアダプターを探し、無ければその試験を飛ばす。
- `native` を付けて `OCIO_ROOT` が無いとビルドが止まり、理由を表示する。
