//! `mm proposal`: Claude の編集を提案として保留し（`add`）、人が承認・却下する（`approve`、`reject`、`reopen`）。
//! アプリが書き手で HTTP サーバーがあれば `/proposals` へ転送し、無ければ `project.mmdb` を直接開く。

use std::path::{Path, PathBuf};

use clap::Subcommand;
use mm_serve::Client;
use mm_store::{ProposalState, StoredProposal};
use mm_types::{MmError, Result};
use serde_json::{json, Value};

use crate::commands::{
    actor, build_txn, cli_context, finish, now_ms, op_error, open_reader, open_target, Target,
};
use crate::describe::describe_op_text;

/// `mm proposal`。上の段の登録は `main.rs` の 1 行。
#[derive(Subcommand)]
pub enum Cmd {
    /// Claude の編集の提案（保留、一覧、承認、却下、保留に戻す）。
    Proposal {
        #[command(subcommand)]
        command: ProposalCommand,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Proposal { command } => run(command),
        }
    }
}

/// `mm proposal` のサブコマンド。
#[derive(Subcommand)]
pub enum ProposalCommand {
    /// 操作を適用せずに提案として保留する。引数は `mm op` と同じ（`--txn <file.json>` も）。
    Add {
        dir: PathBuf,
        /// 操作の名前（addLayer、ripple、setParam など）。
        op_name: Option<String>,
        /// 複数の操作を 1 つの提案にする（`Transaction` の JSON）。
        #[arg(long)]
        txn: Option<PathBuf>,
        /// 対応しているコメントの ID。
        #[arg(long)]
        responds_to: Option<String>,
        /// 操作の引数（`--フィールド 値` の並び）。
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// 提案の一覧（新しい順）。既定は保留中だけ。
    List {
        dir: PathBuf,
        /// 決定済みも出す。
        #[arg(long)]
        all: bool,
    },
    /// 保留中の提案を承認して適用する。取り消しは `mm undo`。
    Approve {
        dir: PathBuf,
        id: i64,
        /// 承認する操作の番号（`0,2`）。省くと全部。
        #[arg(long, value_delimiter = ',')]
        ops: Option<Vec<usize>>,
    },
    /// 保留中の提案を却下する。プロジェクトのデータは変えない。
    Reject { dir: PathBuf, id: i64 },
    /// 却下した提案を保留に戻す。
    Reopen { dir: PathBuf, id: i64 },
}

/// `mm proposal` を行う。
pub fn run(command: ProposalCommand) -> Result<()> {
    match command {
        ProposalCommand::Add {
            dir,
            op_name,
            txn,
            responds_to,
            args,
        } => add(&dir, op_name.as_deref(), &args, txn.as_deref(), responds_to),
        ProposalCommand::List { dir, all } => list(&dir, all),
        ProposalCommand::Approve { dir, id, ops } => approve(&dir, id, ops),
        ProposalCommand::Reject { dir, id } => decide(&dir, id, Decision::Reject),
        ProposalCommand::Reopen { dir, id } => decide(&dir, id, Decision::Reopen),
    }
}

fn add(
    dir: &Path,
    name: Option<&str>,
    tokens: &[String],
    txn_file: Option<&Path>,
    responds_to: Option<String>,
) -> Result<()> {
    let txn = build_txn(name, tokens, txn_file)?;
    let mut store = match open_target(dir)? {
        Target::Local(store) => *store,
        Target::Remote(client) => {
            let body = json!({ "txn": txn, "entry": "cli", "responds_to": responds_to });
            let p = check(client.post("/proposals", &body)?, "提案")?;
            println!(
                "提案 {}: {}（アプリへ転送）",
                p["id"],
                p["name"].as_str().unwrap_or("")
            );
            return Ok(());
        }
    };
    let p = store
        .propose(txn, &cli_context(), actor(), now_ms(), responds_to)
        .map_err(op_error)?;
    print_proposal(&p);
    finish(store)
}

fn list(dir: &Path, all: bool) -> Result<()> {
    let store = open_reader(dir)?;
    let shown: Vec<StoredProposal> = store
        .proposals()?
        .into_iter()
        .filter(|p| all || p.state == ProposalState::Pending)
        .collect();
    if shown.is_empty() {
        println!("提案は無い");
    }
    for p in &shown {
        print_proposal(p);
    }
    Ok(())
}

fn approve(dir: &Path, id: i64, ops: Option<Vec<usize>>) -> Result<()> {
    let mut store = match open_target(dir)? {
        Target::Local(store) => *store,
        Target::Remote(client) => {
            let body = json!({ "ops": ops, "entry": "cli" });
            let res = check(
                client.post(&format!("/proposals/{id}/approve"), &body)?,
                "承認",
            )?;
            println!(
                "提案 {id} を承認した: バージョン {}（アプリへ転送）",
                res["change"]["version"]
            );
            return Ok(());
        }
    };
    let (p, txn) = store
        .approve_proposal(id, ops.as_deref(), &actor(), now_ms())
        .map_err(op_error)?;
    println!(
        "提案 {id} を承認した: バージョン {}（取り消しは mm undo）",
        txn.doc_version
    );
    print_proposal(&p);
    finish(store)
}

#[derive(Clone, Copy)]
enum Decision {
    Reject,
    Reopen,
}

impl Decision {
    fn path(self) -> &'static str {
        match self {
            Self::Reject => "reject",
            Self::Reopen => "reopen",
        }
    }
}

fn decide(dir: &Path, id: i64, decision: Decision) -> Result<()> {
    let mut store = match open_target(dir)? {
        Target::Local(store) => *store,
        Target::Remote(client) => return decide_remote(&client, id, decision),
    };
    let p = match decision {
        Decision::Reject => store.reject_proposal(id, &actor(), now_ms())?,
        Decision::Reopen => store.reopen_proposal(id)?,
    };
    print_proposal(&p);
    finish(store)
}

fn decide_remote(client: &Client, id: i64, decision: Decision) -> Result<()> {
    let path = format!("/proposals/{id}/{}", decision.path());
    let p = check(
        client.post(&path, &json!({ "entry": "cli" }))?,
        "提案の決定",
    )?;
    println!(
        "提案 {id}: {}（アプリへ転送）",
        p["state"].as_str().unwrap_or("")
    );
    Ok(())
}

fn check(res: mm_serve::Response, what: &str) -> Result<Value> {
    if res.status != 200 {
        return Err(MmError::InvalidArgument(format!(
            "{what}を拒否した（HTTP {}）: {}",
            res.status,
            res.error_message()
        )));
    }
    res.json()
}

fn state_label(p: &StoredProposal) -> &'static str {
    match (p.state, p.applied) {
        (ProposalState::Pending, _) => "保留",
        (ProposalState::Approved, true) => "承認（適用中）",
        (ProposalState::Approved, false) => "承認（取り消し済み）",
        (ProposalState::Rejected, _) => "却下",
    }
}

fn print_proposal(p: &StoredProposal) {
    let responds = p
        .responds_to
        .as_deref()
        .map_or(String::new(), |c| format!("  対応: {c}"));
    println!(
        "提案 {}  {}  {}（{} / {}）  バージョン {} から{responds}",
        p.id,
        state_label(p),
        p.name,
        p.actor.kind,
        p.actor.name,
        p.base_version
    );
    let approved = p.approved_ops.as_deref();
    for (i, op) in p.ops.iter().enumerate() {
        let text =
            serde_json::to_value(op).map_or_else(|e| e.to_string(), |v| describe_op_text(&v));
        let mark = if approved.is_some_and(|a| !a.contains(&i)) {
            "（承認しなかった）"
        } else {
            ""
        };
        println!("  {i}: {text}{mark}");
    }
}
