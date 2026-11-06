# mm-decode-d3d12va

FFmpeg の `d3d12va` でデコードした NV12 / P010 の `ID3D12Resource` を、コピーなしで wgpu のテクスチャにラップし、レンダラーと同じデバイスの上で線形の `Rgba16Float` に変えて返す `Decoder`。
結果と判断は [`docs/design/spikes/nvdec.md`](../../docs/design/spikes/nvdec.md)。`mm-runtime` の `decode = "d3d12va"`（と `auto`）で選ぶ。

## feature

FFmpeg と D3D12 に触る所は feature `d3d12va` の中にあり、既定で切ってある。
切ったままなら FFmpeg も libclang も要らず、ワークスペースはそのままビルドできる（中身は定数 `ENABLED` と `ffi` の構造体だけ）。

```sh
export FFMPEG_DIR=D:/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export PATH="$PATH:/d/dev/ffmpeg-dev/ffmpeg-n9.0-latest-win64-gpl-shared-9.0/bin"
cargo test -p mm-decode-d3d12va --features d3d12va
```

## 作り

- デバイスは `DecoderFactory::open` の `gpu`（`mm-gpu` の `GpuContext`）。D3D12 のアダプターで作る必要があり、頼み方は `gpu_request()`（`GpuRequest::backends` を D3D12 にする）。`required_features()` が NV12、P010、16 bit の形式を求める。
- フレームの画像は `GpuFrameTexture`（`Rgba16Float`、線形、premultiplied）。変換先は借用が終わればプールへ戻る。
- デコードしたサーフェスは、変換を読み終えたことを wgpu のキューのフェンスで確かめるまで FFmpeg のプールに返さない（`shared/release.rs`）。
- 開く前に、別プロセス `mm-d3d12va-probe` で先頭 4 フレームをデコードして確かめる（NVIDIA の d3d12va は失敗するとプロセスの D3D12 のデバイスが全部消えるため）。実行ファイルは `MM_D3D12VA_PROBE`、なければ実行ファイルと同じ場所（かその一つ上）の `mm-d3d12va-probe.exe`。Cargo は依存先の bin を作らないので、`cargo run -p mm` などでは先に `cargo build -p mm-decode-d3d12va --features d3d12va --bin mm-d3d12va-probe` を実行しておく。見つからない、起動できない、時間切れのときは試せなかった扱いで `Err(Unsupported)`（次の実装へ譲る。結果はキャッシュしない）。
- 時間軸（PTS の索引、VFR）、シーク、フレームの取り出し、YUV の行列は `mm-decode-ffmpeg-core`、フェンスの待ち、変換の係数、描画パスは `mm-decode-hw`（どちらも `mm-decode-cuvid` と `mm-decode-ffmpeg` の共通実装。[RFC 0007](../../docs/rfcs/0007-decode-shared-crates.md)）。
- PQ と HLG の素材は `Err(Unsupported)`（Rec.1886 の式で線形にすると明るさが狂うため。CPU の経路も同じ判定）。
- 音は無音を返す。`mm-runtime` が FFmpeg のデコーダーの音と素材の情報を付ける。

## 測る

```sh
cargo build --release -p mm-decode-d3d12va --features d3d12va --bins --examples
./target/release/examples/bench_d3d12va.exe <素材> <decode|render|seek> [--frames N] [--warmup N]
```

時間は `mm-trace` のスパン（`decode.next_frame`、`decode.wrap`、`decode.seek`）で出す。素材は ffmpeg の CLI で作る（`target/` の下など、リポジトリの外に置く）。

## テスト

`tests/contract.rs` が `mm-testkit` の契約（CFR と VFR）、焼き込んだ番号、シーク、10 bit、プール、フェンス（GPU を 600 ms 遅らせる負荷）を実機で確かめる。
素材は `fixtures/media/generate.sh` が作る（10 bit の HEVC と NVENC の 4K HEVC を含む。NVENC が無い PC では後者を作らず、そのテストは飛ぶ）。
