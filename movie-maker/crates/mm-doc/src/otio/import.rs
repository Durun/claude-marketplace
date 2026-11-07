//! OTIO の `Timeline` から素材と sequence への変換。入口と、ID・素材の割り当て。トラックの中身は `import_track`。

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use mm_types::{AssetId, ColorSpace, MmError, ParamValue, RationalTime, Result};

use super::export::path_to_url;
use super::import_track::import_track;
use super::schema_name;
use super::time::{range_from_json, rational_from_f64};
use crate::{Asset, Composition, Node, Project, Sequence};

/// 取り込みの指定。
#[derive(Debug, Clone, Default)]
pub struct ImportOptions {
    /// sequence の ID を指定する。指定すると、ファイルの `movie-maker` のメタデータの ID（sequence、カット、素材、
    /// コンポジション、マーカー）と元との関係を使わず、ID を作り直す。同じファイルを別の名前で取り込み直すときに使う。
    pub sequence_id: Option<String>,
}

/// 取り込みの結果。プロジェクトへは `addAsset`、`addSequence`、`addComposition` の操作で足す。
#[derive(Debug, Clone, Default)]
pub struct Imported {
    /// プロジェクトに無かった素材。同じ場所の素材が既にあれば足さず、既にある素材を指す。
    pub assets: Vec<Asset>,
    pub sequences: Vec<Sequence>,
    /// マーカーを持つコンポジション。
    pub compositions: Vec<Composition>,
    /// 取り込めなかったもの、変えて取り込んだもの。
    pub warnings: Vec<String>,
}

/// 取り込みの間の状態。使った ID と、新しく足す素材。
pub(super) struct Ctx<'a> {
    pub project: &'a Project,
    /// メタデータの ID を使うか。
    pub keep_ids: bool,
    pub warnings: Vec<String>,
    pub assets: Vec<Asset>,
    /// OTIO のメタデータの素材の ID から、割り当てた素材の ID。ノードの素材の参照を書き換える。
    pub asset_remap: HashMap<String, AssetId>,
    /// メタデータが無く、取り込みで作った素材。長さをカットのアウト点の最大まで延ばす。
    generated: HashSet<AssetId>,
    extent: HashMap<AssetId, RationalTime>,
    used: HashMap<&'static str, HashSet<String>>,
    warned: HashSet<String>,
}

pub(super) const SEQUENCE: &str = "sequence";
pub(super) const CUT: &str = "cut";
pub(super) const MARKER: &str = "marker";
pub(super) const COMPOSITION: &str = "composition";
pub(super) const LAYER: &str = "layer";
const DEFAULT_ASSET_STEM: &str = "media";

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

/// ID に使う短い名前。英数字以外は `-` にまとめる。
pub(super) fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let trimmed = out.trim_matches('-');
    if trimmed.is_empty() {
        "x".to_owned()
    } else {
        trimmed.to_owned()
    }
}

/// プロジェクトで使っている ID を種類ごとに集める。取り込む ID はこれと重ならないように選ぶ。
fn existing_ids(project: &Project) -> HashMap<&'static str, HashSet<String>> {
    let comps = &project.compositions;
    let seqs = &project.sequences;
    HashMap::from([
        (SEQUENCE, seqs.iter().map(|s| s.id.to_string()).collect()),
        (
            CUT,
            seqs.iter()
                .flat_map(|s| s.cuts.iter().map(|c| c.id.to_string()))
                .collect(),
        ),
        (
            COMPOSITION,
            comps.iter().map(|c| c.id.to_string()).collect(),
        ),
        (
            MARKER,
            comps
                .iter()
                .flat_map(|c| c.markers.iter().map(|m| m.id.to_string()))
                .collect(),
        ),
        (
            LAYER,
            comps
                .iter()
                .flat_map(|c| c.layers.iter().map(|l| l.id.to_string()))
                .collect(),
        ),
    ])
}

impl<'a> Ctx<'a> {
    fn new(project: &'a Project, opts: &ImportOptions) -> Self {
        Self {
            project,
            keep_ids: opts.sequence_id.is_none(),
            warnings: Vec::new(),
            assets: Vec::new(),
            asset_remap: HashMap::new(),
            generated: HashSet::new(),
            extent: HashMap::new(),
            used: existing_ids(project),
            warned: HashSet::new(),
        }
    }

    pub fn warn(&mut self, msg: String) {
        self.warnings.push(msg);
    }

    /// 同じ内容の警告は 1 回だけ出す。
    pub fn warn_once(&mut self, msg: String) {
        if self.warned.insert(msg.clone()) {
            self.warnings.push(msg);
        }
    }

    /// [`super::read_meta`] の読めない理由を警告にして `None` にする。
    pub fn meta<T: for<'de> serde::Deserialize<'de>>(&mut self, object: &Value) -> Option<T> {
        super::read_meta(object).unwrap_or_else(|e| {
            self.warn_once(e);
            None
        })
    }

    pub fn is_used(&self, kind: &'static str, id: &str) -> bool {
        self.used.get(kind).is_some_and(|s| s.contains(id))
    }

    /// `kind` の ID を 1 つ使う。メタデータの ID があって未使用ならそれ、無ければ `{prefix}-{n}` の最初の空き。
    pub fn take_id(&mut self, kind: &'static str, preferred: Option<&str>, prefix: &str) -> String {
        let set = self.used.entry(kind).or_default();
        let id = match preferred {
            Some(p) if !set.contains(p) => p.to_owned(),
            _ => (1..)
                .map(|n| format!("{prefix}{n}"))
                .find(|c| !set.contains(c))
                .unwrap_or_default(),
        };
        set.insert(id.clone());
        id
    }

    /// 素材の参照から素材を決める。同じ場所の素材が既にあればそれ、無ければ足す。
    pub fn resolve_asset(&mut self, reference: &Value) -> Result<AssetId> {
        let url = reference
            .get("target_url")
            .and_then(Value::as_str)
            .ok_or_else(|| invalid("素材の参照に target_url が無い".into()))?;
        let path = url_to_path(url);
        let meta = self
            .meta::<AssetMeta>(reference)
            .map(|m| m.asset)
            .filter(|a| path_to_url(&a.path) == url);
        let meta_id = meta.as_ref().map(|a| a.id.to_string());
        if let Some(existing) = self.find_asset(&path_to_url(&path)) {
            self.remember_remap(meta_id, &existing);
            return Ok(existing);
        }
        let id = self.new_asset_id(meta.as_ref(), &path)?;
        let from_meta = meta.is_some();
        let asset = match meta {
            Some(a) => Asset {
                id: id.clone(),
                ..a
            },
            None => asset_from_reference(id.clone(), path, reference)?,
        };
        if !from_meta {
            self.generated.insert(id.clone());
        }
        self.remember_remap(meta_id, &id);
        self.assets.push(asset);
        Ok(id)
    }

    /// URL どうしで比べる。取り込みは Windows の区切り（`D:\a.mp4`）で保存し、URL から戻したパスは `/` 区切りになる。
    fn find_asset(&self, url: &str) -> Option<AssetId> {
        let same = |a: &&Asset| path_to_url(&a.path) == url;
        self.project
            .assets
            .iter()
            .chain(self.assets.iter())
            .find(same)
            .map(|a| a.id.clone())
    }

    fn remember_remap(&mut self, meta_id: Option<String>, id: &AssetId) {
        if let Some(old) = meta_id.filter(|old| old != id.as_str()) {
            self.asset_remap.insert(old, id.clone());
        }
    }

    fn new_asset_id(&self, meta: Option<&Asset>, path: &str) -> Result<AssetId> {
        let taken = |id: &str| {
            self.project
                .assets
                .iter()
                .chain(self.assets.iter())
                .any(|a| a.id.as_str() == id)
        };
        if let Some(m) = meta.filter(|_| self.keep_ids) {
            if taken(m.id.as_str()) {
                return Err(invalid(format!(
                    "素材の ID {} が別の場所の素材で使われている（--as で ID を作り直せる）",
                    m.id
                )));
            }
            return Ok(m.id.clone());
        }
        let stem = path
            .rsplit('/')
            .next()
            .and_then(|f| f.rsplit_once('.').map(|(s, _)| s).or(Some(f)))
            .filter(|s| !s.is_empty())
            .unwrap_or(DEFAULT_ASSET_STEM);
        let base = format!("a-{}", slug(stem));
        let id = std::iter::once(base.clone())
            .chain((2..).map(|n| format!("{base}-{n}")))
            .find(|c| !taken(c))
            .unwrap_or(base);
        Ok(AssetId::new(id))
    }

    /// カットのアウト点を覚える。作った素材の長さをそこまで延ばす。
    pub fn note_extent(&mut self, id: &AssetId, out: RationalTime) {
        let e = self.extent.entry(id.clone()).or_insert(RationalTime::ZERO);
        *e = (*e).max(out);
    }

    /// ノードの素材の参照を、割り当てた素材の ID に直す。
    pub fn remap_node(&self, node: &mut Node) {
        for param in node.params.values_mut() {
            if let ParamValue::AssetRef(id) = &mut param.value {
                if let Some(new) = self.asset_remap.get(id.as_str()) {
                    *id = new.clone();
                }
            }
        }
        for input in &mut node.inputs {
            self.remap_node(input);
        }
    }

    fn finish_assets(&mut self) {
        let extent = std::mem::take(&mut self.extent);
        for asset in &mut self.assets {
            if self.generated.contains(&asset.id) {
                if let Some(e) = extent.get(&asset.id) {
                    asset.duration = asset.duration.max(*e);
                }
            }
        }
    }
}

/// 素材の参照のメタデータ。
#[derive(serde::Deserialize)]
struct AssetMeta {
    asset: Asset,
}

/// メタデータの無い素材の参照から素材を作る。ハッシュは空で、色空間は映像の既定。
fn asset_from_reference(id: AssetId, path: String, reference: &Value) -> Result<Asset> {
    let available = range_from_json(reference.get("available_range"))?;
    let fps = reference
        .pointer("/available_range/start_time/rate")
        .and_then(Value::as_f64)
        .and_then(rational_from_f64);
    Ok(Asset {
        id,
        path,
        content_hash: String::new(),
        color_space: ColorSpace::bt709(),
        duration: available.map_or(RationalTime::ZERO, |(_, d)| d),
        fps,
        timebase_offset: RationalTime::ZERO,
    })
}

/// `target_url` を素材の場所にする。`file://` は % を戻し、Windows のドライブ文字の前の `/` を除く。
pub(super) fn url_to_path(url: &str) -> String {
    let Some(rest) = url.strip_prefix("file://") else {
        return url.to_owned();
    };
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode(rest);
    let bytes = decoded.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'/' && bytes[1].is_ascii_alphabetic() && bytes[2] == b':' {
        decoded[1..].to_owned()
    } else {
        decoded
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes
            .get(i + 1..i + 3)
            .and_then(|h| std::str::from_utf8(h).ok())
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `.otio` の `Timeline` を素材と sequence にする。映像のトラック 1 本が sequence 1 つ。
///
/// 音声のトラックは取り込まない（警告に出す）。`project` は既にある素材と ID を見るためだけに読む。
pub fn import_timeline(otio: &Value, project: &Project, opts: &ImportOptions) -> Result<Imported> {
    if schema_name(otio) != Some("Timeline") {
        return Err(invalid(format!(
            "OTIO の Timeline でない: {}",
            otio.get("OTIO_SCHEMA").unwrap_or(&Value::Null)
        )));
    }
    let children = otio
        .pointer("/tracks/children")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("Timeline に tracks の children が無い".into()))?;
    let mut ctx = Ctx::new(project, opts);
    let video: Vec<&Value> = children
        .iter()
        .filter(|c| is_video_track(c, &mut ctx))
        .collect();
    if video.is_empty() {
        return Err(invalid("映像のトラックが無い".into()));
    }
    let name = otio.get("name").and_then(Value::as_str).unwrap_or("");
    let base = opts.sequence_id.clone().unwrap_or_else(|| slug(name));
    let mut out = Imported::default();
    for (index, track) in video.iter().enumerate() {
        let (seq, comp) = import_track(&mut ctx, track, &base, index, video.len(), opts)?;
        out.sequences.push(seq);
        out.compositions.extend(comp);
    }
    ctx.finish_assets();
    out.assets = ctx.assets;
    out.warnings = ctx.warnings;
    Ok(out)
}

/// 映像のトラックか。音声と、Track 以外の子（入れ子の Stack など）は警告を出して除く。
fn is_video_track(child: &Value, ctx: &mut Ctx) -> bool {
    let name = child.get("name").and_then(Value::as_str).unwrap_or("");
    let kind = child.get("kind").and_then(Value::as_str).unwrap_or("Video");
    match (schema_name(child), kind) {
        (Some("Track" | "Sequence"), "Video") => true,
        (Some("Track" | "Sequence"), other) => {
            ctx.warn(format!("{other} のトラック {name:?} は取り込まない"));
            false
        }
        (other, _) => {
            ctx.warn(format!("トラックでない子 {other:?} は取り込まない"));
            false
        }
    }
}
