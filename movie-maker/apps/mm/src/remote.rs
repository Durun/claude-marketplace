//! 動いているアプリへ HTTP で転送する経路（`mm-serve`）。別のプロセスが書き手で、HTTP のポートとトークンが
//! `.mm/` にあるときに使う。動いていなければ `commands.rs` が `project.mmdb` を直接開く。

use std::path::{Path, PathBuf};

use mm_ops::Transaction;
use mm_runtime::{assemble, RuntimeConfig};
use mm_serve::{parse_time, render_png, Client, FrameRequest, Response};
use mm_store::TranscriptInput;
use mm_types::{AssetId, MmError, Result};
use serde_json::{json, Value};

use crate::project::load_project;

/// `mm frame`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// 1 フレームを PNG に書く（Claude が目で見る）。アプリが動いていればアプリが描く。
    Frame {
        /// プロジェクトの JSON、または `project.mmdb` のあるフォルダー。
        project: PathBuf,
        /// コンポジションの時間軸での時刻（秒。`1.5`、`3/2`）。
        t: String,
        #[arg(short, long)]
        output: PathBuf,
        /// コンポジションの ID。省くと `main`、それも無ければ先頭。
        #[arg(long)]
        comp: Option<String>,
        /// 出力の幅。省くとコンポジションの大きさ。
        #[arg(long)]
        width: Option<u32>,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Frame {
                project,
                t,
                output,
                comp,
                width,
            } => frame(&FrameArgs {
                project: &project,
                time: &t,
                output: &output,
                composition: comp.as_deref(),
                width,
            }),
        }
    }
}

/// 操作経路の名前。アプリ側でロックの付いた所に触る操作を拒否する扱いになる。
const ENTRY: &str = "cli";
const UNAUTHORIZED: u16 = 401;

/// 失敗の応答を `mm` のエラーにする。
fn check(res: Response, what: &str) -> Result<Response> {
    if res.status == 200 {
        return Ok(res);
    }
    Err(MmError::InvalidArgument(format!(
        "{what}を拒否した（HTTP {}）: {}",
        res.status,
        res.error_message()
    )))
}

fn print_change(change: &Value) {
    println!(
        "バージョン {}: {}（アプリへ転送）",
        change["version"],
        change["name"].as_str().unwrap_or("")
    );
    if let Some(of) = change.get("of") {
        println!(
            "  元の操作: {} / {}（{}）",
            of["actor"].as_str().unwrap_or(""),
            of["actor_name"].as_str().unwrap_or(""),
            of["name"].as_str().unwrap_or("")
        );
    }
    let touched: Vec<&str> = change["touched"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if !touched.is_empty() {
        println!("  触った: {}", touched.join(", "));
    }
}

/// `mm op` をアプリに送る。
pub fn op(client: &Client, txn: &Transaction) -> Result<()> {
    let res = client.post("/ops", &json!({ "txn": txn, "entry": ENTRY }))?;
    print_change(&check(res, "操作")?.json()?);
    Ok(())
}

/// アプリが今持っているプロジェクトのデータ。
pub fn project(client: &Client) -> Result<mm_doc::Project> {
    let body = check(client.get("/project")?, "プロジェクトの取得")?.json()?;
    Ok(serde_json::from_value(body["project"].clone())?)
}

/// アプリのプロジェクトにある素材の（ID、コンテンツハッシュ）。
pub fn assets(client: &Client) -> Result<Vec<(AssetId, String)>> {
    let body = check(client.get("/project")?, "プロジェクトの取得")?.json()?;
    let list = body["project"]["assets"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    Ok(list
        .iter()
        .map(|a| {
            let id = a["id"].as_str().unwrap_or_default();
            let hash = a["content_hash"].as_str().unwrap_or_default();
            (AssetId::new(id), hash.to_owned())
        })
        .collect())
}

/// 素材の文字起こしをアプリの DB に入れる。入れた単語の数を返す。
pub fn put_transcript(
    client: &Client,
    asset: &AssetId,
    content_hash: &str,
    input: &TranscriptInput,
) -> Result<usize> {
    let words: Vec<Value> = input
        .words
        .iter()
        .map(|w| json!({ "segment": w.segment, "start": w.start, "end": w.end, "text": w.text }))
        .collect();
    let body = json!({
        "asset": asset, "content_hash": content_hash,
        "producer": input.producer, "language": input.language, "words": words,
    });
    let res = check(client.post("/transcripts", &body)?, "文字起こしの取り込み")?;
    Ok(res.json()?["words"].as_u64().unwrap_or(0) as usize)
}

/// `mm undo` / `mm redo` をアプリに送る。`path` は `/undo` か `/redo`。
pub fn history(client: &Client, path: &str, empty: &str) -> Result<()> {
    let res = check(client.post(path, &json!({ "entry": ENTRY }))?, "履歴の操作")?;
    let body = res.json()?;
    if body["change"].is_null() {
        println!("{empty}");
    } else {
        print_change(&body["change"]);
    }
    Ok(())
}

/// `mm frame` の指定。
pub struct FrameArgs<'a> {
    pub project: &'a Path,
    pub time: &'a str,
    pub output: &'a Path,
    pub composition: Option<&'a str>,
    pub width: Option<u32>,
}

/// `mm frame`: 動いているアプリがあればそこで描いてもらい、無ければ自分で描く。
pub fn frame(args: &FrameArgs) -> Result<()> {
    let dir = mm_store::store_dir(args.project);
    let remote = dir.as_deref().and_then(Client::for_project);
    let png = match remote.map(|c| fetch_frame(&c, args)) {
        Some(Ok(Some(png))) => png,
        // 接続できない、またはトークンが合わない HTTP サーバーは、古い情報として自分で描く。
        Some(Ok(None) | Err(MmError::Io(_))) | None => render_local(args)?,
        Some(Err(e)) => return Err(e),
    };
    std::fs::write(args.output, &png)?;
    println!("{}: {} バイト", args.output.display(), png.len());
    Ok(())
}

/// トークンが拒否されたら `None`（別のプロセスのポートに古いトークンを送った場合）。
fn fetch_frame(client: &Client, args: &FrameArgs) -> Result<Option<Vec<u8>>> {
    let mut path = format!("/frame?t={}", encode_query(args.time));
    if let Some(c) = args.composition {
        path.push_str(&format!("&comp={}", encode_query(c)));
    }
    if let Some(w) = args.width {
        path.push_str(&format!("&width={w}"));
    }
    let res = client.get(&path)?;
    if res.status == UNAUTHORIZED {
        return Ok(None);
    }
    Ok(Some(check(res, "フレームの描画")?.body))
}

fn render_local(args: &FrameArgs) -> Result<Vec<u8>> {
    let request = FrameRequest {
        composition: args.composition.map(str::to_owned),
        time: parse_time(args.time)?,
        width: args.width,
    };
    let project = load_project(args.project)?;
    let mut engine = assemble(&RuntimeConfig::real())?;
    render_png(&mut engine, &project, &request)
}

/// クエリの値の記号を `%XX` にする。`/`（分数の時刻）と `-` は読みやすさのためそのまま通す。
fn encode_query(raw: &str) -> String {
    raw.bytes()
        .map(|b| match b {
            b'0'..=b'9' | b'a'..=b'z' | b'A'..=b'Z' | b'-' | b'_' | b'.' | b'/' => {
                (b as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn クエリの記号は_16_進にする() {
        assert_eq!(encode_query("3/2"), "3/2");
        assert_eq!(encode_query("a b&c"), "a%20b%26c");
    }
}
