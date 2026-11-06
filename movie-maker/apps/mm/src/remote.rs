//! 動いているアプリへ HTTP で転送する経路（`mm-serve`）。別のプロセスが書き手で、HTTP のポートと合い言葉が
//! `.mm/` にあるときに使う。動いていなければ `commands.rs` が `project.mmdb` を直接開く。

use std::path::Path;

use mm_ops::Transaction;
use mm_runtime::{assemble, RuntimeConfig};
use mm_serve::{parse_time, render_png, Client, FrameRequest, Response};
use mm_types::{MmError, Result};
use serde_json::{json, Value};

use crate::project::load_project;

/// 操作経路の名前。アプリ側でロックの付いた所に触る操作を拒否する扱いになる。
const ENTRY: &str = "cli";

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
        Some(Ok(png)) => png,
        // アプリが落ちて残った `.mm/` の情報は、接続できないので自分で描く。
        Some(Err(MmError::Io(_))) | None => render_local(args)?,
        Some(Err(e)) => return Err(e),
    };
    std::fs::write(args.output, &png)?;
    println!("{}: {} バイト", args.output.display(), png.len());
    Ok(())
}

fn fetch_frame(client: &Client, args: &FrameArgs) -> Result<Vec<u8>> {
    let mut path = format!("/frame?t={}", encode_query(args.time));
    if let Some(c) = args.composition {
        path.push_str(&format!("&comp={}", encode_query(c)));
    }
    if let Some(w) = args.width {
        path.push_str(&format!("&width={w}"));
    }
    Ok(check(client.get(&path)?, "フレームの描画")?.body)
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
