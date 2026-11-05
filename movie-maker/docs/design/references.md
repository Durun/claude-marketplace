# 出典


Web で確かめたもの。〔推〕は確かめられなかった推測。

1. tauri-apps/cef-rs（Rust の CEF バインディング。`cef`、`cef-dll-sys`。Windows x86_64 と ARM64、Apache-2.0）: https://github.com/tauri-apps/cef-rs 。OpenFX の Rust は `openfx-sys` 0.1.0（C API と HostSupport のバインディング）: https://docs.rs/crate/openfx-sys/latest 。Rust で書いた OpenFX のホストの先行事例は見つからなかった
2. `cef` クレートの `osr_texture_import/d3d11.rs`（D3D12 と Vulkan で CEF の共有テクスチャを wgpu に取り込む）: https://docs.rs/crate/cef/latest/source/src/osr_texture_import/d3d11.rs
3. `wgpu_hal::dx12::Device::texture_from_raw` と `create_texture_from_hal` で D3D12 の共有リソースを包む例（`grafting`、`mediaway`）: https://docs.rs/crate/grafting/latest/source/src/dx12_shared_texture.rs 、https://docs.rs/crate/mediaway/latest/source/src/wgpu/dx12_decode.rs
4. naga の入力（SPIR-V、WGSL、GLSL 440+ の Vulkan の流儀）と出力（SPIR-V、WGSL、MSL、HLSL SM 5.0+、GLSL 330+）: https://github.com/gfx-rs/wgpu/blob/trunk/naga/README.md
5. wgpu の `TextureFormat`（`Rgba16Float`、`Rgba32Float`）: https://wgpu.rs/doc/wgpu/enum.TextureFormat.html 。`Rgba32Float` のブレンドは `FLOAT32_FILTERABLE` 等の機能に依る〔推〕
6. CEF の `OnAcceleratedPaint`（`shared_texture_enabled`、`SendExternalBeginFrame`）の見本 cef-mixer: https://github.com/twobrainsgmbh/cef-mixer 。設計の経緯: https://bitbucket.org/chromiumembedded/cef/pull-requests/158
7. 共有ハンドルはコールバックの外で使えず、毎回開き直す（CefSharp の文書）: https://cefsharp.github.io/api/130.1.x/html/M_CefSharp_Wpf_ChromiumWebBrowser_OnAcceleratedPaint.htm 。OBS のブラウザソースがこの経路を使うことは obs-browser の実装では確かめていない〔推〕
8. Dioxus subsecond（ジャンプテーブルで関数を差し替え。構造体の形の変更は落ちる。`dx serve --hotpatch`。対応の表は Android、iOS、Linux）: https://docs.rs/subsecond/
9. Gyroflow（核は純 Rust、UI は Qt/QML、GPU は wgpu と OpenCL）: https://docs.gyroflow.xyz/app/technical-details/used-technologies
10. Rerun（egui + wgpu、`re_renderer`）: https://github.com/rerun-io/rerun/blob/main/ARCHITECTURE.md
11. Slint の wgpu の取り込み（`unstable-wgpu-NN`、`slint::Image::try_from(wgpu::Texture)`）: https://docs.slint.dev/latest/docs/rust/slint/wgpu_29/
12. glyphon（cosmic-text + etagere + wgpu）: https://lib.rs/crates/glyphon
13. MasterSelects（WebGPU の編集ソフト + Rust の Native Helper）: https://github.com/Sportinger/MasterSelects
14. OpenCut（Rust + wgpu の核を WASM で。desktop は gpui でまだ空）: https://github.com/opencut-app/opencut
15. OCIO の Rust は置き場所だけの空のクレート（`opencolorio` 0.1.0、2020）: https://lib.rs/crates/opencolorio
16. OCIO の GPU の言語（GLSL、HLSL SM 5.0、MSL、OSL）: https://deepwiki.com/AcademySoftwareFoundation/OpenColorIO/2.5-gpu-processing-system （二次情報。公式の `GpuShaderDesc` で確かめ直す）
17. `ffmpeg-the-third`（FFmpeg 5.1〜8.1）: https://docs.rs/crate/ffmpeg-the-third/latest 。`ffmpeg-next` は保守モード。`hwaccel` の扱いは各クレートの文書で確かめ直す〔推〕
18. FFmpeg 7.0 の D3D12VA の復号（H.264、HEVC、VP9、AV1、MPEG-2、VC1）: https://ffmpeg.org/doxygen/7.0/d3d12va__decode_8h_source.html
19. `cudarc` の `import_external_memory`（Win32 の不透明なハンドル。D3D12 の種類は生の `sys`）: https://docs.rs/crate/cudarc/0.19.3/source/src/driver/safe/external_memory.rs
20. `ebur128` クレート（libebur128 の移植。EBU TECH 3341・3342 のテストを通る）: https://docs.rs/crate/ebur128/latest
21. `opentimelineio-bind`（vfx-rs。Linux だけ）: https://github.com/vfx-rs/opentimelineio-bind
22. OpenFX 1.5 の GPU Rendering Suite（Metal、CUDA、OpenCL）: https://openfx.readthedocs.io/en/main/ReleaseNotes/relnotes-1.5.html
23. `clack-host`（安全な Rust の CLAP のホスト）: https://docs.rs/clack-host
24. `vst3` クレート（coupler-rs。SDK は別に要る）: https://docs.rs/crate/vst3/0.2.0/source/README.md
25. Tauri 2 の複数 WebView（`unstable`。Windows で新しい WebView が前に出ない報告 #9798）: https://github.com/tauri-apps/tauri/issues/9798
26. wry の WebView2 の透過が Windows 11 で黒くなる報告: https://github.com/tauri-apps/wry/issues/692
27. Tauri のモバイルの複数の窓（各窓が全画面の WebView。2026-03）: https://github.com/tauri-apps/tauri/pull/14484
28. wry の wgpu の例（`build_as_child` と透過の WebView）: https://docs.rs/crate/wry/latest/source/examples/wgpu.rs 。Tauri v2 への移植 FabianLars/tauri-v2-wgpu（2024-12 にアーカイブ）: https://github.com/FabianLars/tauri-v2-wgpu
29. Slint のライセンス（GPLv3、Royalty-free、商用）: https://users.rust-lang.org/t/which-gui-framwork-should-i-choose/137581 （二次情報）
30. Hexagonal architecture（ports and adapters。核を UI やデータベースなしで動かし、テストも人も同じポートから駆動する）: https://alistair.cockburn.us/hexagonal-architecture/
31. `wgpu-hal`（「A cross-platform unsafe graphics abstraction」。検証は `wgpu-core` の責任）: https://docs.rs/wgpu-hal/latest/wgpu_hal/ 。`wgpu-core`（公開 API で検証、資源の ID と追跡）: https://docs.rs/wgpu-core/latest/wgpu_core/
32. OpenFX の Core API（`OfxGetPlugin`、`OfxSetHost`、`OfxHost.fetchSuite`、`mainEntry` の action。プラグインはホストに記号で依存しない）: https://openfx.readthedocs.io/en/main/Reference/ofxCoreAPI.html
33. Bevy の `Plugin`（`fn build(&self, app: &mut App)`、`App::add_plugins`）と `PluginGroup`（`PluginGroupBuilder`、`set` で中の部品を差し替え）: https://docs.rs/bevy/latest/bevy/app/trait.Plugin.html 、https://docs.rs/bevy/latest/bevy/app/trait.PluginGroup.html
34. Zed のワークスペース（`crates/*` に約 250 のクレート、`workspace.dependencies`、`workspace.package`、既定のメンバーは `zed`）: https://github.com/zed-industries/zed/blob/main/Cargo.toml
35. `cargo-semver-checks`（rustdoc の JSON から semver 違反を検査）: https://github.com/obi1kenobi/cargo-semver-checks
36. Cargo のワークスペース（`members`、共有の `Cargo.lock` と `target/`、`workspace.dependencies` と `workspace.package` の継承、`default-members`）: https://doc.rust-lang.org/cargo/reference/workspaces.html
37. rustc のクエリ（名前・キー・結果で宣言し、結果をメモ化、提供者の表 `Providers`・`ExternProviders` を差し替えられる）: https://rustc-dev-guide.rust-lang.org/query.html
38. durun-dashboard の XP 風のスキン（`client/skin.ts` が `html` に `data-skin="xp"` を付け、`public/style.css` が `:root[data-skin="xp"]` で色の変数（`#ece9d8`、`#316ac5`、Tahoma）を上書き。作業ウィンドウ `.xp-task-title`・`.xp-task-body`、アイコンは CSS で描く `.xp-info-icon`）: `E:/git-repos/claude-marketplace/durun-dashboard/client/skin.ts`、`public/style.css`
