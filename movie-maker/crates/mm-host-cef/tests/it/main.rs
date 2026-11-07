//! `mm-host-cef` の結合テスト。リンクを 1 回にするため、1 つの実行ファイルにまとめる（docs/ci.md）。
//! `serve` と `harden` は CEF の配布物が要る（`--features cef`、`CEF_PATH`）。

mod harden;
mod locate;
mod policy;
mod serve;
