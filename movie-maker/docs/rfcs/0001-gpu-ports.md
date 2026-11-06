# RFC 0001: GPU のインターフェース（GpuHelper で囲う、フレームの寿命、GPU の選び方）

状態: 採用

## 変える理由

- 設計書は `NodeKind::render` に `wgpu::CommandEncoder` を渡す形だったが、インターフェースに wgpu を出すと契約テストと縦の 1 本が GPU なしで回らない。PR #5 で `GpuHelper` トレイトで囲う形にした。これを正式にする。
- `mm-decode-api` の `GpuFrameHandle { id, size }` では wgpu のテクスチャをレンダラーに渡せない（NVDEC の試作）。
- 実装ごとに既定の GPU の選び方が違うと、内蔵 GPU と外付け GPU の 2 つがある PC で共有テクスチャが `E_INVALIDARG` で失敗する（CEF、NVDEC の試作）。
- 別プロセスのホストが返す NT ハンドルは、ホストのプロセスの中だけで有効。アプリの pid と、完了をどちらが待つかの契約が要る（CEF の試作）。

## 決めたこと

### インターフェースに wgpu を出さない。wgpu の型は共通実装 `mm-gpu` で合わせる

- API クレート（`mm-*-api`）は wgpu に依存しない。GPU は `mm_node_api::GpuHelper`、`mm_decode_api::{GpuFrame, GpuContextRef}` の型を消したトレイトで囲い、`as_any` で実装の型に降りる。
- 新しいクレート `mm-gpu` を「共通実装」として置く。wgpu のコンテキスト（`GpuContext`）、アダプターの選び方、`GpuHelper` の wgpu 実装（`WgpuGpu`）、GPU のフレームの内容（`GpuFrameTexture`）を持つ。GPU を触る実装（`mm-render`、`mm-decode-d3d12va`、`mm-decode-cuvid`、`mm-host-cef`、wgpu を使う `mm-nodes-*`）はこれに依存してよい。依存の規則 3 の例外として `14-dev-structure.md` に書く。
- wgpu の `CommandEncoder` が要るノードの種類は `GpuHelper::as_any` で `mm_gpu::WgpuGpu` に降りる。これで `mm-nodes-*` が `mm-render` に依存せずに済む。

### GPU のフレームの受け渡しと寿命

```rust
// mm-decode-api
pub trait GpuFrame: Send + Sync { fn as_any(&self) -> &dyn Any; }
pub struct GpuFrameHandle { pub id: u64, pub size: Size, pub frame: Arc<dyn GpuFrame> }
pub trait GpuContextRef: Send + Sync { fn adapter(&self) -> &GpuAdapterId; fn as_any(&self) -> &dyn Any; }
pub trait DecoderFactory { fn open(&self, path: &Path, gpu: Option<&dyn GpuContextRef>) -> Result<Box<dyn Decoder>>; }

// mm-node-api
pub trait GpuHelper {
    fn release(&mut self, frame: &FrameRef);
    fn import(&mut self, frame: &GpuFrameHandle) -> Result<FrameRef>;  // 既定は Unsupported
    // upload / download / allocate / as_any は変わらない
}
```

- `FrameSource::frame_at` が返す GPU のフレームは、レンダラーの次の `submit` が終わるまで借用する。レンダラーは `import` で借用し、`submit` の前に内容の待ち（`GpuFrameTexture::wait`）を呼び、`submit` の後に返す（`WgpuGpu::before_submit` / `after_submit`）。キャッシュに置くなら中間形式にコピーする。
- デコーダーが GPU でデコードするにはレンダラーと同じデバイスが要るので、`DecoderFactory::open` に `GpuContextRef` を渡す。`None` なら CPU のフレームを返す。

### GPU の選び方を 1 つにする

- `mm_types::GpuAdapterId { vendor, device, name, luid }`。`RuntimeConfig::gpu` で選び、`None` なら高性能のアダプター。
- `mm-runtime` の組み立てで `mm_gpu::GpuContext::shared(&GpuRequest)` を 1 回作り、レンダラーとデコーダーに渡す。`Engine::gpu_adapter()` が別プロセスのホストの `HostSpec::gpu` に渡す値。
- D3D12 では LUID も取る（`mm-gpu` が hal 経由で読む）。LUID が両方にあればそれで、無ければベンダー・デバイス・名前で同じアダプターと判定する。

### 別プロセスのホスト

```rust
pub struct HostSpec { /* 既存 */ pub gpu: Option<GpuAdapterId>, pub client_pid: Option<u32> }
pub enum SharedFrame { Cpu {..}, SharedTexture { handle: u64, size: Size, fence: Option<SharedFence> } }
pub struct SharedFence { pub handle: u64, pub value: u64 }
```

- ホストは `gpu` のアダプターでテクスチャを作り、`client_pid` のプロセスへ複製したハンドルを返す（`DuplicateHandle`）。
- 完了の同期: `fence` が `None` ならホストが描画の完了を待ってから返しており、アプリはすぐ開ける。`Some` ならアプリが `submit` の前にその値を待つ。段階 0 の CEF の試作は前者。

## 影響する実装

- `mm-render`: `GpuContext` / `WgpuGpu` を `mm-gpu` から使う。`submit` の前後で借用したフレームの待ちと返却。`DecoderFactory::open` にコンテキストを渡す。
- `mm-testkit`: `CpuGpu::release`、`SampleDecoderFactory::open` の引数、`HostFixture`。
- `mm-decode-ffmpeg`: `open` の引数（CPU なのでコンテキストは使わない）。
- `mm-decode-d3d12va`: `open` の引数と `GpuFrameHandle::frame`。試作は自前の D3D12 のコンテキストのままで、インターフェースのコンテキストにそろえるのは段階 1。
- `mm-runtime`: `RuntimeConfig::gpu`、`EngineParts::gpu`、`Engine::gpu_adapter`。
- `mm-host-cef`: `HostSpec` の新しいフィールドは `serde(default)` なので今の試作はそのまま動く。pid への複製は段階 1。

## 移行の手順

1. `mm-types` に `GpuAdapterId`、`mm-decode-api` と `mm-node-api` と `mm-host-api` に上の型を足す。
2. `mm-gpu` を作り、`mm-render` の `context.rs` と `gpu.rs` を移す。
3. 実装と契約テストを直し、`cargo test --workspace` と `MM_REQUIRE_GPU=1` で全部通す。
4. 段階 1 で d3d12va を既定にするとき、`D3d12vaFactory::open` がインターフェースのコンテキストを使い、`GpuFrameTexture` を返す形にする（API クレートは変えない）。
