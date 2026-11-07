//! プロジェクトのデータの不変条件。[`Project::validate`] が読み込みと操作の後に通す。

use std::collections::{BTreeSet, HashMap, HashSet};

use mm_types::{Id, LayerId, MmError, ParamValue, Result};

use crate::node::{Node, NESTED_KIND, NESTED_PARAM, SEQUENCE_KIND, SEQUENCE_PARAM};
use crate::param::{Interpolation, Param};
use crate::{Composition, Cut, Layer, Mask, Project, Sequence};

fn invalid(msg: String) -> MmError {
    MmError::InvalidArgument(msg)
}

/// 同じ種類の ID の重複を見つける。
#[derive(Default)]
struct IdSet<'a> {
    kind: &'static str,
    seen: HashSet<&'a str>,
}

impl<'a> IdSet<'a> {
    fn new(kind: &'static str) -> Self {
        Self {
            kind,
            seen: HashSet::new(),
        }
    }

    fn add<T>(&mut self, id: &'a Id<T>) -> Result<()> {
        if !self.seen.insert(id.as_str()) {
            return Err(invalid(format!("{} の ID {id} が重複", self.kind)));
        }
        Ok(())
    }
}

/// コンテンツハッシュに使える文字か。ハッシュは `cache/` の下のファイル名になるので、区切りと `.` を通さない。
/// 試験の素材は `stage0-cfr` のような名前を持つので、16 進に限らず英数字と `-`、`_` を受ける。
pub(crate) fn is_hash_name(hash: &str) -> bool {
    hash.bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Project {
    /// プロジェクトのデータの不変条件を確かめる。
    ///
    /// - 素材のコンテンツハッシュが英数字と `-`、`_` だけ（`cache/` の外を指さない）。
    /// - 素材とコンポジションの長さが負でない。レイヤーとカットは `in < out`。
    /// - ID が種類ごとに重複しない。
    /// - 親・トラックマット・接続・レンダーターゲット・入れ子・sequence の参照先が在る。
    /// - 親の鎖と入れ子に循環が無い。
    /// - キーフレームの時刻が昇順。タイムリマップは `Hold` と `Linear` だけ。ベジェの制御点の x が `[0,1]`。
    /// - マーカーの時刻が厳密な昇順（同じ時刻を持たない）で、長さが負でない。
    pub fn validate(&self) -> Result<()> {
        self.validate_ids()?;
        for asset in &self.assets {
            if asset.duration.is_negative() {
                return Err(invalid(format!("素材 {} の長さが負", asset.id)));
            }
            if !is_hash_name(&asset.content_hash) {
                return Err(invalid(format!(
                    "素材 {} のコンテンツハッシュに使えない文字: {:?}",
                    asset.id, asset.content_hash
                )));
            }
        }
        for comp in &self.compositions {
            validate_composition(self, comp)?;
        }
        for seq in &self.sequences {
            validate_sequence(self, seq)?;
        }
        for target in &self.render_targets {
            self.composition(&target.composition).map_err(|_| {
                invalid(format!(
                    "レンダーターゲット {} のコンポジション {} が無い",
                    target.id, target.composition
                ))
            })?;
        }
        self.validate_bins()?;
        validate_nesting(self)
    }

    fn validate_ids(&self) -> Result<()> {
        let mut assets = IdSet::new("素材");
        let mut comps = IdSet::new("コンポジション");
        let mut layers = IdSet::new("レイヤー");
        let mut markers = IdSet::new("マーカー");
        let mut effects = IdSet::new("効果");
        let mut masks = IdSet::new("マスク");
        let mut seqs = IdSet::new("sequence");
        let mut cuts = IdSet::new("カット");
        let mut targets = IdSet::new("レンダーターゲット");
        for a in &self.assets {
            assets.add(&a.id)?;
        }
        for c in &self.compositions {
            comps.add(&c.id)?;
            for m in &c.markers {
                markers.add(&m.id)?;
            }
            for l in &c.layers {
                layers.add(&l.id)?;
                for e in &l.effects {
                    effects.add(&e.id)?;
                }
                for m in &l.masks {
                    masks.add(&m.id)?;
                }
            }
        }
        for s in &self.sequences {
            seqs.add(&s.id)?;
            for c in &s.cuts {
                cuts.add(&c.id)?;
            }
        }
        for t in &self.render_targets {
            targets.add(&t.id)?;
        }
        Ok(())
    }
}

fn validate_composition(project: &Project, comp: &Composition) -> Result<()> {
    comp.range()
        .map_err(|e| invalid(format!("コンポジション {} の長さ: {e}", comp.id)))?;
    let mut last_marker = None;
    for m in &comp.markers {
        if m.duration.is_negative() {
            return Err(invalid(format!("マーカー {} の長さが負", m.id)));
        }
        if last_marker.is_some_and(|t| m.time <= t) {
            return Err(invalid(format!(
                "マーカー {} の時刻が厳密な昇順でない",
                m.id
            )));
        }
        last_marker = Some(m.time);
    }
    if let Some(mixer) = &comp.mixer {
        mixer
            .check()
            .map_err(|e| invalid(format!("コンポジション {} のミキサー: {e}", comp.id)))?;
    }
    // 親とトラックマットの先を ID の集合で引く（レイヤー数の 2 乗にしない）。
    let ids: HashSet<&str> = comp.layers.iter().map(|l| l.id.as_str()).collect();
    for layer in &comp.layers {
        validate_layer(project, &ids, layer)?;
    }
    validate_parent_chain(comp)
}

fn validate_layer(project: &Project, ids: &HashSet<&str>, layer: &Layer) -> Result<()> {
    let id = &layer.id;
    if layer.in_point >= layer.out_point {
        return Err(invalid(format!("レイヤー {id} は in < out でない")));
    }
    layer
        .range()
        .map_err(|e| invalid(format!("レイヤー {id} の範囲: {e}")))?;
    if let Some(parent) = &layer.parent {
        if parent == id || !ids.contains(parent.as_str()) {
            return Err(invalid(format!("レイヤー {id} の親 {parent} が無い")));
        }
    }
    if let Some(matte) = &layer.track_matte {
        if matte.layer == *id || !ids.contains(matte.layer.as_str()) {
            return Err(invalid(format!(
                "レイヤー {id} のトラックマット {} が無い",
                matte.layer
            )));
        }
    }
    if let Some(cut) = &layer.attach_to {
        project
            .cut(cut)
            .map_err(|_| invalid(format!("レイヤー {id} の接続先のカット {cut} が無い")))?;
    }
    if let Some(remap) = &layer.time_remap {
        validate_time_remap(id, remap)?;
    }
    for param in layer_params(layer) {
        validate_keyframes(&format!("レイヤー {id}"), param)?;
    }
    validate_framing(layer)?;
    validate_fades(layer)?;
    for node in layer.nodes() {
        validate_node(project, &format!("レイヤー {id}"), node)?;
    }
    Ok(())
}

/// 再フレーミングのカットは 1 つに 1 つ。消えたカットの項目は残してよい（取り消しで戻ったカットに効き直す）。
fn validate_framing(layer: &Layer) -> Result<()> {
    let mut seen = HashSet::new();
    match layer.framing.cuts.iter().find(|r| !seen.insert(&r.cut)) {
        Some(dup) => Err(invalid(format!(
            "レイヤー {} の再フレーミングのカット {} が重なる",
            layer.id, dup.cut
        ))),
        None => Ok(()),
    }
}

/// 音のフェードの長さは正。長さ 0 のフェードは「無い」と区別がつかないので `None` で表す。
fn validate_fades(layer: &Layer) -> Result<()> {
    let Some(audio) = &layer.audio else {
        return Ok(());
    };
    let bad = [&audio.fade_in, &audio.fade_out]
        .into_iter()
        .flatten()
        .any(|f| f.duration <= mm_types::RationalTime::ZERO);
    if bad {
        return Err(invalid(format!(
            "レイヤー {} の音のフェードの長さが正でない",
            layer.id
        )));
    }
    Ok(())
}

/// 親の鎖をたどって循環を見つける。
fn validate_parent_chain(comp: &Composition) -> Result<()> {
    let parents: HashMap<&str, &LayerId> = comp
        .layers
        .iter()
        .filter_map(|l| l.parent.as_ref().map(|p| (l.id.as_str(), p)))
        .collect();
    for layer in &comp.layers {
        let mut seen = BTreeSet::new();
        let mut cur = layer.id.as_str();
        while let Some(parent) = parents.get(cur) {
            if !seen.insert(cur) {
                return Err(invalid(format!("レイヤー {} の親が循環", layer.id)));
            }
            cur = parent.as_str();
        }
    }
    Ok(())
}

fn validate_time_remap(id: &LayerId, remap: &crate::TimeRemap) -> Result<()> {
    if remap.keyframes.is_empty() {
        return Err(invalid(format!("レイヤー {id} のタイムリマップが空")));
    }
    let mut last = None;
    for k in &remap.keyframes {
        if k.interpolation == Interpolation::Bezier {
            return Err(invalid(format!(
                "レイヤー {id} のタイムリマップはベジェを使えない"
            )));
        }
        if last.is_some_and(|t| k.time <= t) {
            return Err(invalid(format!(
                "レイヤー {id} のタイムリマップの時刻が昇順でない"
            )));
        }
        last = Some(k.time);
    }
    Ok(())
}

/// レイヤーが直接持つパラメータ（ノードの種類のパラメータは [`validate_node`]）。
fn layer_params(layer: &Layer) -> impl Iterator<Item = &Param> {
    let masks = layer.masks.iter().flat_map(Mask::params);
    let audio = layer.audio.iter().flat_map(|a| [&a.gain_db, &a.pan]);
    std::iter::once(&layer.opacity)
        .chain(layer.transform.params())
        .chain(layer.crop.params())
        .chain(masks)
        .chain(audio)
        .chain(layer.framing.params())
}

fn validate_node(project: &Project, owner: &str, root: &Node) -> Result<()> {
    for node in root.descendants() {
        for (name, param) in &node.params {
            validate_keyframes(&format!("{owner} の {}.{name}", node.kind), param)?;
            if let ParamValue::AssetRef(asset) = &param.value {
                project.asset(asset).map_err(|_| {
                    invalid(format!(
                        "{owner} の {}.{name} の素材 {asset} が無い",
                        node.kind
                    ))
                })?;
            }
        }
        if node.kind == NESTED_KIND {
            let id = node.text_param(NESTED_PARAM).unwrap_or("");
            project
                .composition(&Id::new(id))
                .map_err(|_| invalid(format!("{owner} の入れ子のコンポジション {id} が無い")))?;
        }
        if node.kind == SEQUENCE_KIND {
            let id = node.text_param(SEQUENCE_PARAM).unwrap_or("");
            project
                .sequence(&Id::new(id))
                .map_err(|_| invalid(format!("{owner} の sequence {id} が無い")))?;
        }
    }
    Ok(())
}

fn validate_keyframes(owner: &str, param: &Param) -> Result<()> {
    let mut last = None;
    for k in &param.keyframes {
        if last.is_some_and(|t| k.time <= t) {
            return Err(invalid(format!("{owner} のキーフレームの時刻が昇順でない")));
        }
        if k.bezier.is_some_and(|b| !b.is_valid()) {
            return Err(invalid(format!(
                "{owner} のベジェの制御点の x が [0,1] に無い"
            )));
        }
        last = Some(k.time);
    }
    Ok(())
}

fn validate_sequence(project: &Project, seq: &Sequence) -> Result<()> {
    if let Some(origin) = &seq.origin {
        project.sequence(&origin.sequence).map_err(|_| {
            invalid(format!(
                "sequence {} の元の sequence {} が無い",
                seq.id, origin.sequence
            ))
        })?;
    }
    for cut in &seq.cuts {
        if cut.in_point >= cut.out_point {
            return Err(invalid(format!("カット {} は in < out でない", cut.id)));
        }
        cut.duration()
            .map_err(|e| invalid(format!("カット {} の長さ: {e}", cut.id)))?;
        if let Some(t) = &cut.transition_in {
            if t.duration.is_negative() {
                return Err(invalid(format!(
                    "カット {} のトランジションの長さが負",
                    cut.id
                )));
            }
        }
        for node in cut.nodes() {
            validate_node(project, &format!("カット {}", cut.id), node)?;
        }
    }
    Ok(())
}

/// 入れ子の循環を調べる巡りの 1 歩の行き先。コンポジションと sequence は ID の名前空間が別。
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Visit<'a> {
    Comp(&'a str),
    Seq(&'a str),
}

impl Visit<'_> {
    fn describe(self) -> String {
        match self {
            Visit::Comp(id) => format!("コンポジション {id}"),
            Visit::Seq(id) => format!("sequence {id}"),
        }
    }
}

/// 入れ子（コンポジション・sequence から、ノードが指すコンポジション・sequence）の循環。
/// 根はコンポジションと sequence の全部で、どこからも参照されない sequence の循環も見つける。
fn validate_nesting(project: &Project) -> Result<()> {
    let mut walk = Nesting {
        project,
        path: Vec::new(),
        done: HashSet::new(),
    };
    for comp in &project.compositions {
        walk.visit(Visit::Comp(comp.id.as_str()))?;
    }
    for seq in &project.sequences {
        walk.visit(Visit::Seq(seq.id.as_str()))?;
    }
    Ok(())
}

struct Nesting<'a> {
    project: &'a Project,
    path: Vec<Visit<'a>>,
    done: HashSet<Visit<'a>>,
}

impl<'a> Nesting<'a> {
    fn visit(&mut self, target: Visit<'a>) -> Result<()> {
        if self.path.contains(&target) {
            return Err(invalid(format!("{} の入れ子が循環", target.describe())));
        }
        if self.done.contains(&target) {
            return Ok(());
        }
        self.path.push(target);
        match target {
            Visit::Comp(id) => {
                let comp = self.project.composition(&Id::new(id))?;
                for node in comp.layers.iter().flat_map(Layer::nodes) {
                    self.visit_node(node)?;
                }
            }
            Visit::Seq(id) => {
                let seq = self.project.sequence(&Id::new(id))?;
                for node in seq.cuts.iter().flat_map(Cut::nodes) {
                    self.visit_node(node)?;
                }
            }
        }
        self.path.pop();
        self.done.insert(target);
        Ok(())
    }

    fn visit_node(&mut self, root: &'a Node) -> Result<()> {
        for node in root.descendants() {
            if node.kind == NESTED_KIND {
                self.visit(Visit::Comp(node.text_param(NESTED_PARAM).unwrap_or("")))?;
            }
            if node.kind == SEQUENCE_KIND {
                self.visit(Visit::Seq(node.text_param(SEQUENCE_PARAM).unwrap_or("")))?;
            }
        }
        Ok(())
    }
}
