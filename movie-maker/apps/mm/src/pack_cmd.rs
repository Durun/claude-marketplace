//! `mm pack`: パック（docs/design/13-vlog-pack.md）の検査、マクロの一覧、マクロの展開と適用。
//! 適用は `mm op --txn` と同じ経路を通るので、アプリが書き手なら HTTP で転送し、取り消しは `mm undo`。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Subcommand;
use mm_pack::{check, expand, Pack};
use mm_runtime::{Backend, Engine, RuntimeConfig};
use mm_types::{MmError, Result};

use crate::commands::apply_txn;

/// `mm pack`。上の段の登録は `main.rs` の 1 行。
#[derive(Subcommand)]
pub enum Cmd {
    /// パック（vlog など）の検査、マクロの一覧、マクロを展開して適用する。
    Pack {
        #[command(subcommand)]
        command: PackCommand,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Pack { command } => run(command),
        }
    }
}

/// `mm pack` のサブコマンド。
#[derive(Subcommand)]
pub enum PackCommand {
    /// パックの形式を検査する。マクロを例の引数で展開し、テンプレートを適用した空のプロジェクトに適用してみる。
    Check { pack: PathBuf },
    /// テンプレートとマクロの一覧（引数の型と既定）。
    List {
        pack: PathBuf,
        /// 1 行に 1 件の JSON で出す。
        #[arg(long)]
        json: bool,
    },
    /// マクロを展開して、適用せずに `Transaction` の JSON を出す（`mm proposal add --txn` に渡せる）。
    Expand {
        pack: PathBuf,
        name: String,
        /// 引数（`--var text=こんにちは`）。
        #[arg(long = "var", value_parser = parse_var)]
        vars: Vec<(String, String)>,
    },
    /// マクロを展開して 1 つのトランザクションとして適用する。
    /// `mm pack apply <dir> packs/vlog caption --var id=c1 --var text=やあ --var at=1.5`
    Apply {
        dir: PathBuf,
        pack: PathBuf,
        name: String,
        /// 引数（`--var text=こんにちは`）。
        #[arg(long = "var", value_parser = parse_var)]
        vars: Vec<(String, String)>,
        /// 元にしたプロジェクトのデータのバージョン（`mm op --base` と同じ）。
        #[arg(long)]
        base: Option<u64>,
    },
}

/// `mm pack` を行う。
pub fn run(command: PackCommand) -> Result<()> {
    match command {
        PackCommand::Check { pack } => run_check(&pack),
        PackCommand::List { pack, json } => list(&pack, json),
        PackCommand::Expand { pack, name, vars } => {
            let engine = text_engine()?;
            let args = vars.into_iter().collect();
            let txn = expand(
                &Pack::load(&pack)?,
                &name,
                &args,
                &*engine.services().text_shaper,
            )?;
            println!("{}", serde_json::to_string_pretty(&txn)?);
            Ok(())
        }
        PackCommand::Apply {
            dir,
            pack,
            name,
            vars,
            base,
        } => {
            let args: BTreeMap<String, String> = vars.into_iter().collect();
            let engine = text_engine()?;
            let shaper = &*engine.services().text_shaper;
            let mut txn = expand(&Pack::load(&pack)?, &name, &args, shaper)?;
            txn.base_version = base;
            apply_txn(&dir, txn)
        }
    }
}

/// プレートの大きさを測るための、文字だけ実際の実装（フォントを読む CPU の実装）を選んだエンジン。描くときと同じ字の幅になる。
fn text_engine() -> Result<Engine> {
    mm_runtime::assemble(&RuntimeConfig {
        text: Backend::Cpu,
        ..RuntimeConfig::sample()
    })
}

fn run_check(dir: &Path) -> Result<()> {
    let pack = Pack::load(dir)?;
    let report = check(&pack);
    if report.ok() {
        println!(
            "{} {}: 通った（テンプレートとマクロ {} 個を例で適用した）",
            pack.manifest.name, pack.manifest.version, report.applied
        );
        return Ok(());
    }
    for e in &report.errors {
        eprintln!("  {e}");
    }
    Err(MmError::InvalidArgument(format!(
        "パック {} の検査で {} 件の誤り",
        pack.manifest.name,
        report.errors.len()
    )))
}

fn list(dir: &Path, json: bool) -> Result<()> {
    let pack = Pack::load(dir)?;
    for def in &pack.macros {
        let summary = def.summary();
        if json {
            println!("{}", serde_json::to_string(&summary)?);
            continue;
        }
        let params: Vec<String> = summary
            .params
            .iter()
            .map(|(name, p)| match (&p.default, p.optional) {
                (Some(d), _) => format!("{name}={d}"),
                (None, true) => format!("[{name}]"),
                (None, false) => name.clone(),
            })
            .collect();
        println!("{}\t{}\t{}", def.name, def.title, params.join(" "));
    }
    Ok(())
}

/// `名前=値` を組にする。
fn parse_var(raw: &str) -> std::result::Result<(String, String), String> {
    raw.split_once('=')
        .map(|(k, v)| (k.trim().to_string(), v.to_string()))
        .filter(|(k, _)| !k.is_empty())
        .ok_or_else(|| format!("引数は 名前=値 の形: {raw}"))
}
