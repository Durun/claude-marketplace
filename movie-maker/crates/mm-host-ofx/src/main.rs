//! `mm-host-ofx` の実行ファイル。`serve` はアプリ（`OfxLauncher`）からの要求に答え、
//! `describe <バンドル>` はバンドルのプラグインとパラメータの一覧を JSON の 1 行で出す。

mod host;

fn main() -> std::process::ExitCode {
    host::run()
}
