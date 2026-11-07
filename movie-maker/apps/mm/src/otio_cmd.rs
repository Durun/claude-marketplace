//! `mm otio export` / `mm otio import`: sequence と OpenTimelineIO（`.otio`）の変換。
//! 取り込みは素材と sequence を足す操作として 1 つのトランザクションで適用し、`mm undo` で取り消せる。

use std::path::{Path, PathBuf};

use mm_doc::otio::{export_sequence, import_timeline, ImportOptions, Imported};
use mm_doc::{Project, SequenceId};
use mm_ops::{AddAsset, AddComposition, AddSequence, Op, Transaction};
use mm_types::{Id, MmError, Result};

use crate::commands::{actor, cli_context, finish, now_ms, op_error, open_target, Target};
use crate::project::load_project;
use crate::remote;

/// `mm otio`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// sequence と OpenTimelineIO（`.otio`）の変換。取り込みは操作として当てる（取り消せる）。
    Otio {
        #[command(subcommand)]
        command: OtioCommand,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Otio { command } => run(command),
        }
    }
}

/// トランザクションの名前。
const TXN_NAME: &str = "OTIO の取り込み";
/// `--sequence` を省いたときに選ぶ sequence の ID（本編）。
const MAIN_SEQUENCE: &str = "main";

/// `mm otio` のサブコマンド。
#[derive(clap::Subcommand)]
pub enum OtioCommand {
    /// sequence を `.otio` に書く。
    Export {
        /// プロジェクトの JSON、または `project.mmdb` のあるフォルダー。
        project: PathBuf,
        /// 書き出す sequence の ID。省くと `main`、それも無く sequence が 1 つなら、その 1 つ。
        #[arg(long)]
        sequence: Option<String>,
        /// 出力先。省くと標準出力。
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// `.otio` を読み、素材と sequence を足す（`mm undo` で取り消せる）。
    Import {
        dir: PathBuf,
        file: PathBuf,
        /// sequence の ID を指定し、ファイルのメタデータの ID を使わず ID を作り直す。
        /// 書き出したファイルを同じプロジェクトに別の sequence として取り込むときに使う。
        #[arg(long = "as")]
        sequence_id: Option<String>,
    },
}

/// サブコマンドを実行する。
pub fn run(command: OtioCommand) -> Result<()> {
    match command {
        OtioCommand::Export {
            project,
            sequence,
            output,
        } => export(&project, sequence.as_deref(), output.as_deref()),
        OtioCommand::Import {
            dir,
            file,
            sequence_id,
        } => import(&dir, &file, sequence_id),
    }
}

fn choose_sequence(project: &Project, wanted: Option<&str>) -> Result<SequenceId> {
    if let Some(id) = wanted {
        return Ok(Id::new(id));
    }
    let ids: Vec<&SequenceId> = project.sequences.iter().map(|s| &s.id).collect();
    match ids.as_slice() {
        [only] => Ok((*only).clone()),
        _ if ids.iter().any(|id| id.as_str() == MAIN_SEQUENCE) => Ok(Id::new(MAIN_SEQUENCE)),
        _ => Err(MmError::InvalidArgument(format!(
            "--sequence で sequence を選ぶ（{}）",
            ids.iter()
                .map(|id| id.as_str())
                .collect::<Vec<_>>()
                .join("、")
        ))),
    }
}

fn export(project: &Path, sequence: Option<&str>, output: Option<&Path>) -> Result<()> {
    let project = load_project(project)?;
    let id = choose_sequence(&project, sequence)?;
    let mut text = serde_json::to_string_pretty(&export_sequence(&project, &id)?)?;
    text.push('\n');
    match output {
        Some(path) => {
            std::fs::write(path, text)?;
            eprintln!("{}: sequence {id} を書いた", path.display());
        }
        None => print!("{text}"),
    }
    Ok(())
}

/// 取り込みの結果を、素材、sequence、コンポジションの順に足す操作にする。
fn ops_for(imported: &Imported) -> Vec<Op> {
    let assets = imported.assets.iter().map(|a| {
        AddAsset {
            asset: a.clone(),
            index: None,
        }
        .into()
    });
    let sequences = imported.sequences.iter().map(|s| {
        AddSequence {
            sequence: s.clone(),
        }
        .into()
    });
    let comps = imported.compositions.iter().map(|c| {
        AddComposition {
            composition: c.clone(),
        }
        .into()
    });
    assets.chain(sequences).chain(comps).collect()
}

fn import(dir: &Path, file: &Path, sequence_id: Option<String>) -> Result<()> {
    let text = std::fs::read_to_string(file)
        .map_err(|e| MmError::NotFound(format!("{}: {e}", file.display())))?;
    let otio = serde_json::from_str(&text)?;
    let opts = ImportOptions { sequence_id };
    match open_target(dir)? {
        Target::Local(mut store) => {
            let imported = import_timeline(&otio, store.project(), &opts)?;
            report(&imported);
            let txn = Transaction::new(TXN_NAME, ops_for(&imported));
            let stored = store
                .apply_txn(txn, &cli_context(), actor(), now_ms(), None)
                .map_err(op_error)?;
            println!("バージョン {}: {}", stored.doc_version, stored.name);
            finish(*store)
        }
        Target::Remote(client) => {
            let imported = import_timeline(&otio, &remote::project(&client)?, &opts)?;
            report(&imported);
            remote::op(&client, &Transaction::new(TXN_NAME, ops_for(&imported)))
        }
    }
}

fn report(imported: &Imported) {
    for w in &imported.warnings {
        eprintln!("警告: {w}");
    }
    let ids: Vec<&str> = imported.sequences.iter().map(|s| s.id.as_str()).collect();
    println!(
        "sequence {}、素材 {} 件、コンポジション {} 件を足す",
        ids.join("、"),
        imported.assets.len(),
        imported.compositions.len()
    );
}
