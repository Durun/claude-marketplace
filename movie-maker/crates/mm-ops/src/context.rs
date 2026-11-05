//! 命令の文脈。どの入口から来たか（ロックの判定に使う）。

use serde::{Deserialize, Serialize};

/// 命令の入口。4 つの入口は同じ命令を呼び、違いはロックの扱いだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// wgpu の面（Rust から直接）。人の操作。
    Surface,
    /// WebView の画面（Tauri の IPC）。人の操作。
    WebView,
    /// Claude の CLI（`mm op`）。
    Cli,
    /// Claude の MCP。
    Mcp,
}

impl Entry {
    /// Claude の入口か。ロックの付いた所に触る命令を断る。
    pub fn is_claude(self) -> bool {
        matches!(self, Self::Cli | Self::Mcp)
    }
}

/// 命令を当てるときの文脈。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpContext {
    pub entry: Entry,
}

impl OpContext {
    /// 入口だけの文脈。
    pub fn new(entry: Entry) -> Self {
        Self { entry }
    }

    /// 人の操作（ロックを気にしない）。
    pub fn human() -> Self {
        Self::new(Entry::Surface)
    }

    /// Claude の操作（ロックの付いた所を断る）。
    pub fn claude() -> Self {
        Self::new(Entry::Mcp)
    }
}
