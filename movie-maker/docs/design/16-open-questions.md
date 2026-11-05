# 決めてほしいこと

1. wgpu の面と WebView2 を並べられなかったとき
   - 案 1（60%）: wgpu の面を別の窓（ビューアーとタイムラインの窓）にして、WebView の窓と 2 枚で使う。Tauri の構成は保てる
   - 案 2（40%）: 対案 C（egui か Slint で全部ネイティブ）に移る。Slint は wgpu のテクスチャを取り込めるが GPLv3、無償の Royalty-free、商用の 3 つのライセンスがある〔推。公式で確かめる〕 [29]、egui は MIT で Rerun の実績がある
2. NVDEC の経路: `d3d12va`（55%。コピーなし。wgpu と同じ D3D12 で同期が素直）か `cuvid`（45%。OFX の CUDA と同じ経路で、後で OFX を足すときに合う）。段階 0 の結果で決める。どちらも `mm-decode-api` の実装として残せるので、決めるのは既定にする方だけ
3. 口の版の検査: 口のクレートの破壊的変更を `cargo-semver-checks` で CI に入れるか（70%。公開しないクレートにも使えるかを段階 1 で確かめる）、`*-api` の `CHANGELOG` と契約テストだけで見るか（30%）


---

出典の番号（[1] など）は [references.md](references.md) を見る。
