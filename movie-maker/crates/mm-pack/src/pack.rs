//! パックのフォルダーを読む。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mm_doc::CompMixer;
use mm_types::{MmError, Result};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::macros::{MacroDef, MacroKind};
use crate::style::TextStyleSpec;
use crate::theme::Theme;
use crate::transition::Transitions;

/// この実装が読めるパックの形式の番号。`pack.json` の `format` と比べる。
pub const FORMAT: u32 = 1;

/// `pack.json`。
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// 形式の番号（[`FORMAT`]）。
    pub format: u32,
    /// パックの名前（英小文字、数字、`-`）。
    pub name: String,
    pub title: String,
    /// パックの中身のバージョン。
    pub version: String,
    #[serde(default)]
    pub description: String,
}

/// 読み込んだパック。
#[derive(Debug, Clone)]
pub struct Pack {
    pub dir: PathBuf,
    pub manifest: Manifest,
    pub theme: Theme,
    /// 文字のスタイルの名前 → スタイル。
    pub styles: BTreeMap<String, TextStyleSpec>,
    /// ミキサーの設定。`mixer.json` が無ければ `None`。
    pub mixer: Option<CompMixer>,
    /// 字幕の表記の置き換え（誤認識の語 → 正しい表記）。
    pub corrections: BTreeMap<String, String>,
    /// テンプレートとマクロ。テンプレートが先で、それぞれ名前の順。
    pub macros: Vec<MacroDef>,
    /// `web/*.html` の名前（拡張子を除く）→ HTML。`web` 手順が `web.cef` のレイヤーにする。
    pub pages: BTreeMap<String, String>,
    /// `transitions/*.wgsl`。`transition` 手順が `transition.wgsl` のトランジションにする。
    pub transitions: Transitions,
}

impl Pack {
    /// フォルダーからパックを読む。`pack.json` と `theme.json` は要り、ほかは無くてよい。
    /// JSON の形が違うファイルは、ファイルの名前を添えた `InvalidArgument`。
    pub fn load(dir: &Path) -> Result<Self> {
        let manifest: Manifest = read_json(&dir.join("pack.json"))?;
        let theme: Theme = read_json(&dir.join("theme.json"))?;
        let styles = read_optional(&dir.join("styles.json"))?.unwrap_or_default();
        let mixer = read_optional(&dir.join("mixer.json"))?;
        let corrections = read_optional(&dir.join("corrections.json"))?.unwrap_or_default();
        let mut macros = read_macros(&dir.join("templates"), MacroKind::Template)?;
        macros.extend(read_macros(&dir.join("macros"), MacroKind::Macro)?);
        Ok(Self {
            dir: dir.to_path_buf(),
            manifest,
            theme,
            styles,
            mixer,
            corrections,
            macros,
            pages: read_pages(&dir.join("web"))?,
            transitions: Transitions::load(&dir.join("transitions"))?,
        })
    }

    /// マクロかテンプレートを名前で引く。
    pub fn find(&self, name: &str) -> Result<&MacroDef> {
        self.macros.iter().find(|m| m.name == name).ok_or_else(|| {
            MmError::NotFound(format!(
                "パック {} のマクロ {name}（あるのは {}）",
                self.manifest.name,
                self.macros
                    .iter()
                    .map(|m| m.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })
    }

    /// テンプレートだけ。
    pub fn templates(&self) -> impl Iterator<Item = &MacroDef> {
        self.macros.iter().filter(|m| m.kind == MacroKind::Template)
    }
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| MmError::NotFound(format!("{}: {e}", path.display())))?;
    serde_json::from_str(&text)
        .map_err(|e| MmError::InvalidArgument(format!("{}: {e}", path.display())))
}

fn read_optional<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    if path.is_file() {
        read_json(path).map(Some)
    } else {
        Ok(None)
    }
}

/// フォルダーの `*.html` を名前（拡張子を除く）で読む。フォルダーが無ければ空。
fn read_pages(dir: &Path) -> Result<BTreeMap<String, String>> {
    if !dir.is_dir() {
        return Ok(BTreeMap::new());
    }
    let entries =
        std::fs::read_dir(dir).map_err(|e| MmError::NotFound(format!("{}: {e}", dir.display())))?;
    entries
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "html"))
        .map(|path| {
            let name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let html = std::fs::read_to_string(&path)
                .map_err(|e| MmError::NotFound(format!("{}: {e}", path.display())))?;
            Ok((name.to_string(), html))
        })
        .collect()
}

/// フォルダーの `*.json` をマクロとして名前の順に読む。ファイル名とマクロの名前が違えば `InvalidArgument`。
fn read_macros(dir: &Path, kind: MacroKind) -> Result<Vec<MacroDef>> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| MmError::NotFound(format!("{}: {e}", dir.display())))?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let mut def: MacroDef = read_json(path)?;
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if def.name != stem {
                return Err(MmError::InvalidArgument(format!(
                    "{}: name は {stem}（ファイル名と同じ）にする",
                    path.display()
                )));
            }
            def.kind = kind;
            Ok(def)
        })
        .collect()
}
