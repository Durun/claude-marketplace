//! 入口の呼び手。`mm` が動いているアプリに操作を送るのと、結合テストが使う。
//!
//! 依存を増やさず、標準の `TcpStream` で HTTP/1.0 を話す（応答は接続を閉じるまでが本体で、
//! SSE も分割転送にならない）。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use mm_store::Store;
use mm_types::{MmError, Result};
use serde_json::Value;

/// 接続と読みの待ちの上限。描画は長いので余裕を持たせる。
const IO_TIMEOUT: Duration = Duration::from_secs(60);

/// 応答。
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub body: Vec<u8>,
}

impl Response {
    /// 本体を JSON として読む。
    pub fn json(&self) -> Result<Value> {
        Ok(serde_json::from_slice(&self.body)?)
    }

    /// 失敗の応答のメッセージ（`error.message`）。JSON でなければ本体の文字列。
    pub fn error_message(&self) -> String {
        self.json()
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
            .unwrap_or_else(|| String::from_utf8_lossy(&self.body).into_owned())
    }
}

/// 動いている入口への接続情報。
#[derive(Debug, Clone)]
pub struct Client {
    port: u16,
    token: String,
}

impl Client {
    /// ポートと合い言葉から。
    pub fn new(port: u16, token: impl Into<String>) -> Self {
        Self {
            port,
            token: token.into(),
        }
    }

    /// プロジェクトのフォルダーの `.mm/` から、動いている入口を探す。ポートか合い言葉が無ければ `None`。
    pub fn for_project(dir: &Path) -> Option<Self> {
        let port = Store::read_writer_info(dir)?.http_port?;
        let token = Store::read_writer_token(dir)?;
        Some(Self::new(port, token))
    }

    /// 待っているポート。
    pub fn port(&self) -> u16 {
        self.port
    }

    /// `GET`。
    pub fn get(&self, path: &str) -> Result<Response> {
        self.send("GET", path, &self.default_headers(), None)
    }

    /// `POST`（本体は JSON）。
    pub fn post(&self, path: &str, body: &Value) -> Result<Response> {
        self.send(
            "POST",
            path,
            &self.default_headers(),
            Some(body.to_string().as_bytes()),
        )
    }

    /// 合い言葉と `Host` を付けた既定のヘッダー。
    pub fn default_headers(&self) -> Vec<(String, String)> {
        vec![
            ("Host".into(), format!("127.0.0.1:{}", self.port)),
            ("Authorization".into(), format!("Bearer {}", self.token)),
        ]
    }

    /// ヘッダーを自分で決める要求。合い言葉なし・別の `Origin` などの拒否を確かめるのに使う。
    pub fn send(
        &self,
        method: &str,
        path: &str,
        headers: &[(String, String)],
        body: Option<&[u8]>,
    ) -> Result<Response> {
        let mut stream = self.connect()?;
        write_request(&mut stream, method, path, headers, body)?;
        let mut reader = BufReader::new(stream);
        let status = read_status(&mut reader)?;
        skip_headers(&mut reader)?;
        let mut body = Vec::new();
        reader.read_to_end(&mut body)?;
        Ok(Response { status, body })
    }

    /// `GET /events` を開く。
    pub fn events(&self) -> Result<EventStream> {
        let mut stream = self.connect()?;
        write_request(&mut stream, "GET", "/events", &self.default_headers(), None)?;
        let mut reader = BufReader::new(stream);
        let status = read_status(&mut reader)?;
        if status != 200 {
            return Err(MmError::InvalidArgument(format!(
                "/events が {status} を返した"
            )));
        }
        skip_headers(&mut reader)?;
        Ok(EventStream { reader })
    }

    fn connect(&self) -> Result<TcpStream> {
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let stream = TcpStream::connect_timeout(&addr, IO_TIMEOUT)?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        Ok(stream)
    }
}

fn write_request(
    stream: &mut TcpStream,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: Option<&[u8]>,
) -> Result<()> {
    let mut head = format!("{method} {path} HTTP/1.0\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    if let Some(b) = body {
        head.push_str(&format!(
            "Content-Type: application/json\r\nContent-Length: {}\r\n",
            b.len()
        ));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    if let Some(b) = body {
        stream.write_all(b)?;
    }
    Ok(())
}

fn read_status(reader: &mut impl BufRead) -> Result<u16> {
    let mut line = String::new();
    reader.read_line(&mut line)?;
    line.split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| MmError::Internal(format!("応答の先頭が読めない: {line:?}")))
}

fn skip_headers(reader: &mut impl BufRead) -> Result<()> {
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 || line == "\r\n" || line == "\n" {
            return Ok(());
        }
    }
}

/// SSE の 1 件。
#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: String,
    pub data: Value,
}

/// 開いた SSE。`next_event` は次の通知まで待つ（待ちの上限は `IO_TIMEOUT`）。
pub struct EventStream {
    reader: BufReader<TcpStream>,
}

impl EventStream {
    /// 次の通知。閉じられたら `None`。キープアライブのコメントは読み飛ばす。
    pub fn next_event(&mut self) -> Result<Option<SseEvent>> {
        let (mut event, mut data) = (String::new(), String::new());
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Ok(None);
            }
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                if event.is_empty() && data.is_empty() {
                    continue;
                }
                return Ok(Some(SseEvent {
                    event,
                    data: serde_json::from_str(&data)?,
                }));
            }
            if let Some(v) = line.strip_prefix("event:") {
                event = v.trim().to_owned();
            } else if let Some(v) = line.strip_prefix("data:") {
                data.push_str(v.trim_start());
            }
        }
    }

    /// 待ちの上限を変える（テストで通知が来ないことを短い時間で確かめる）。
    pub fn set_timeout(&self, limit: Duration) -> Result<()> {
        self.reader.get_ref().set_read_timeout(Some(limit))?;
        Ok(())
    }
}
