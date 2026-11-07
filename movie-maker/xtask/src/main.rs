//! 開発用のコマンド。`cargo xtask <コマンド>`（`.cargo/config.toml` の別名）。
//!
//! - `golden-update [--backend 名前] [--test 名前]`: 期待画像のテストを更新モードで回し、`fixtures/golden/<バックエンド>/` の
//!   PNG を書き換える。`--backend` を付けると、この環境のバックエンドがその名前のときだけ書く（違う GPU の画像で
//!   上書きしない）。書き換えた PNG は目で確かめてからコミットする。

use std::process::{Command, ExitCode};

/// 期待画像のテストの置き場所（クレート、テストの binary の名前、テストのモジュールの絞り込み）。
const GOLDEN_TESTS: &[(&str, &str, &str)] = &[
    ("mm-pack", "it", "golden::"),
    ("mm-render", "it", "golden::"),
];
/// 更新モードにする環境変数。`mm_testkit::golden::UPDATE_ENV` と同じ。
const UPDATE_ENV: &str = "MM_GOLDEN_UPDATE";
/// バックエンドの確認の環境変数。`mm_testkit::golden::BACKEND_ENV` と同じ。
const BACKEND_ENV: &str = "MM_GOLDEN_BACKEND";

const USAGE: &str =
    "使い方: cargo xtask golden-update [--backend <warp|rtx|lavapipe|...>] [--test <テスト名の一部>]";

/// `golden-update` の引数。
#[derive(Debug, Default, PartialEq)]
struct GoldenArgs {
    backend: Option<String>,
    /// `cargo test` に渡すテスト名の絞り込み。
    test: Option<String>,
}

fn parse_golden_args(args: &[String]) -> Result<GoldenArgs, String> {
    let mut parsed = GoldenArgs::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--backend" => {
                parsed.backend = Some(it.next().ok_or("--backend に名前が要る")?.clone());
            }
            "--test" => {
                parsed.test = Some(it.next().ok_or("--test に名前が要る")?.clone());
            }
            other => return Err(format!("知らない引数 {other}\n{USAGE}")),
        }
    }
    Ok(parsed)
}

fn golden_update(args: &GoldenArgs) -> Result<(), String> {
    for (package, test, module) in GOLDEN_TESTS {
        let mut cmd = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
        cmd.args(["test", "-p", package, "--test", test, "--"]);
        // 名前の一部を渡されたらそれだけで絞る（libtest の絞り込みは和なので、モジュールと重ねられない）。
        cmd.arg(args.test.as_deref().unwrap_or(module));
        cmd.arg("--nocapture")
            .env(UPDATE_ENV, "1")
            // GPU が無い環境で何も書かずに成功するのを防ぐ。
            .env("MM_REQUIRE_GPU", "1");
        if let Some(backend) = &args.backend {
            cmd.env(BACKEND_ENV, backend);
        }
        let status = cmd
            .status()
            .map_err(|e| format!("cargo を起動できない: {e}"))?;
        if !status.success() {
            return Err(format!("{package} の {test} が失敗した（{status}）"));
        }
    }
    println!("期待画像を更新した。fixtures/golden/ の PNG を目で確かめてからコミットする。");
    Ok(())
}

fn run(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("golden-update") => golden_update(&parse_golden_args(&args[1..])?),
        _ => Err(USAGE.into()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(args: &[&str]) -> Vec<String> {
        args.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn バックエンドを読む() {
        let parsed = parse_golden_args(&strings(&["--backend", "warp"])).unwrap();
        assert_eq!(parsed.backend.as_deref(), Some("warp"));
        assert_eq!(parse_golden_args(&[]).unwrap(), GoldenArgs::default());
    }

    #[test]
    fn テスト名を読む() {
        let parsed =
            parse_golden_args(&strings(&["--test", "color_cdl", "--backend", "rtx"])).unwrap();
        assert_eq!(parsed.test.as_deref(), Some("color_cdl"));
        assert_eq!(parsed.backend.as_deref(), Some("rtx"));
    }

    #[test]
    fn 知らない引数は拒否する() {
        assert!(parse_golden_args(&strings(&["--test"])).is_err());
        assert!(parse_golden_args(&strings(&["--nope"])).is_err());
        assert!(parse_golden_args(&strings(&["--backend"])).is_err());
    }

    #[test]
    fn コマンドが無ければ使い方を返す() {
        assert!(run(&[]).is_err());
    }
}
