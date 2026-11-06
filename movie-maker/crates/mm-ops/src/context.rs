//! 操作のコンテキスト。どの操作経路から来たか（ロックの判定に使う）。

use serde::{Deserialize, Serialize};

/// 操作経路。4 つの操作経路は同じ操作を呼び、違いはロックの扱いだけ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Entry {
    /// wgpu のサーフェス（Rust から直接）。人の操作。
    Surface,
    /// WebView の画面（Tauri の IPC）。人の操作。
    WebView,
    /// Claude の CLI（`mm op`）。
    Cli,
    /// Claude の MCP。
    Mcp,
}

impl Entry {
    /// Claude の操作経路か。ロックの付いた所に触る操作を拒否する。
    pub fn is_claude(self) -> bool {
        matches!(self, Self::Cli | Self::Mcp)
    }
}

/// 操作を適用するときのコンテキスト。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpContext {
    pub entry: Entry,
}

impl OpContext {
    /// 操作経路だけのコンテキスト。
    pub fn new(entry: Entry) -> Self {
        Self { entry }
    }

    /// 人の操作（ロックを気にしない）。
    pub fn human() -> Self {
        Self::new(Entry::Surface)
    }

    /// Claude の操作（ロックの付いた所を拒否する）。
    pub fn claude() -> Self {
        Self::new(Entry::Mcp)
    }
}
