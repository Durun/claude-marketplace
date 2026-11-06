//! フレームのキャッシュのキー。レイヤーの画像は種類に渡す直前の入力から、入れ子の画像は中身の全部
//! （入れ子の先、シーケンス、効果）の指紋から作る。
//!
//! 画素を変えうる外の入力は全部キーに入れる: 評価済みのパラメータ、素材の `contentHash`（効果が参照する
//! LUT などの素材も）、色の変換の設定の指紋（`ColorTransform::config_fingerprint`）、素材ごとに選ばれた
//! デコーダーの実装（`Decoder::backend_id`）。`contentHash` が空の素材（まだ取り込んでいない）に頼る画像は覚えない。

use std::collections::BTreeSet;

use mm_doc::{Composition, Layer, Node, Project, SEQUENCE_KIND, SEQUENCE_PARAM};
use mm_eval::{node_params_at, source_time};
use mm_node_api::{FrameCx, NodeKind, NodeRegistry, Quality};
use mm_render_api::{digest_bytes, size_bucket, FrameCacheKey};
use mm_types::{AssetId, CompositionId, Id, ParamValue, Params, RationalTime, Size};

use crate::layers::{evaluate, MAX_DEPTH, NESTED_KIND, NESTED_PARAM};
use crate::renderer::WGPU_RENDERER_VERSION;

/// キーと、その値が頼った ID。
pub(crate) type Keyed = (FrameCacheKey, Vec<String>);

/// キーを作る材料のうち、呼ぶ場所で決まるもの。
pub(crate) struct KeyEnv<'a> {
    pub project: &'a Project,
    pub quality: Quality,
    /// 色の変換の実装と設定の指紋。
    pub color: &'a str,
    /// 素材を読むデコーダーの実装の名前。まだ開いていない素材はファクトリーの名前。
    pub decoder: &'a dyn Fn(&AssetId) -> String,
}

/// 素材の ID を持つパラメータから、その ID。
pub(crate) fn asset_of(params: &Params) -> Option<&AssetId> {
    params
        .0
        .values()
        .find_map(ParamValue::as_asset)
        .filter(|id| !id.as_str().is_empty())
}

/// 素材の `contentHash`。素材が無いか、ハッシュが空（取り込みの前）なら `None`（覚えない）。
fn content_hash(project: &Project, id: &AssetId) -> Option<String> {
    let hash = &project.asset(id).ok()?.content_hash;
    (!hash.is_empty()).then(|| hash.clone())
}

/// レイヤーの画像のキー。ノードの種類に渡す直前の `frame`（大きさと領域は倍率を掛けた後）から作る。
/// 決定的でない種類や、素材が引けないものは `None`（覚えない）。
pub(crate) fn kind_key(
    env: &KeyEnv<'_>,
    kind: &dyn NodeKind,
    layer: &Layer,
    frame: &FrameCx<'_>,
) -> Option<Keyed> {
    let traits = kind.traits();
    if !traits.deterministic {
        return None;
    }
    let mut deps = vec![layer.id.as_str().to_owned()];
    let mut hash = None;
    let mut color_config = env.color.to_owned();
    if let Some(id) = asset_of(frame.params) {
        hash = Some(content_hash(env.project, id)?);
        deps.push(id.as_str().to_owned());
        color_config = format!("{color_config}+{}", (env.decoder)(id));
    }
    let source_frame = frame
        .footage
        .as_deref()
        .map(|f| f.info().frame_index_at(frame.source_time));
    // 素材が無い時間で変わる種類は、時刻そのものがキーに要る。素材があれば素材のフレームの番号で足りる。
    let exact_time =
        (traits.time_varying && source_frame.is_none()).then(|| frame.time.to_string());
    let r = frame.region;
    let params_json = serde_json::json!({
        "p": frame.params,
        "canvas": [frame.size.width, frame.size.height],
        "region": [r.x.to_bits(), r.y.to_bits(), r.width.to_bits(), r.height.to_bits()],
        "t": exact_time,
    })
    .to_string();
    let key = FrameCacheKey {
        kind: kind.id().to_owned(),
        kind_version: kind.version(),
        renderer_version: WGPU_RENDERER_VERSION,
        params_json,
        input_keys: Vec::new(),
        content_hash: hash,
        frame_index: source_frame,
        quality: env.quality,
        color_config,
        size_bucket_milli: bucket_milli(frame.scale),
    };
    Some((key, deps))
}

fn bucket_milli(scale: f32) -> u32 {
    (size_bucket(scale) * 1000.0).round() as u32
}

/// 入れ子のコンポジションの画像のキーの材料。
pub(crate) struct NestedKeyInput<'a> {
    pub nodes: &'a dyn NodeRegistry,
    pub kind: &'a dyn NodeKind,
    pub layer: &'a Layer,
    pub params: &'a Params,
    pub nested: &'a Composition,
    /// 入れ子の中の時刻。
    pub t: RationalTime,
    pub size: Size,
    pub scale: f32,
}

/// 入れ子のコンポジションの画像のキー。中身の全部（入れ子の先も）の指紋を入力のキーに入れる。
pub(crate) fn nested_key(env: &KeyEnv<'_>, input: &NestedKeyInput<'_>) -> Option<Keyed> {
    let mut walk = Walk {
        env,
        nodes: input.nodes,
        deps: Vec::new(),
        fingerprint: String::new(),
        seen: BTreeSet::new(),
    };
    walk.visit(input.nested, input.t, 1)?;
    let mut deps = walk.deps;
    deps.push(input.layer.id.as_str().to_owned());
    let params_json = serde_json::json!({
        "p": input.params,
        "t": input.t.to_string(),
        "size": [input.size.width, input.size.height],
    })
    .to_string();
    let key = FrameCacheKey {
        kind: input.kind.id().to_owned(),
        kind_version: input.kind.version(),
        renderer_version: WGPU_RENDERER_VERSION,
        params_json,
        input_keys: vec![digest_bytes(walk.fingerprint.as_bytes())],
        content_hash: None,
        frame_index: None,
        quality: env.quality,
        color_config: env.color.to_owned(),
        size_bucket_milli: bucket_milli(input.scale),
    };
    Some((key, deps))
}

/// 入れ子の中身を辿って、頼った ID と指紋を集める。
struct Walk<'a> {
    env: &'a KeyEnv<'a>,
    nodes: &'a dyn NodeRegistry,
    deps: Vec<String>,
    fingerprint: String,
    seen: BTreeSet<String>,
}

impl Walk<'_> {
    /// 決定的でない種類や、引けない種類と素材があれば `None`（覚えない）。
    fn visit(&mut self, comp: &Composition, t: RationalTime, depth: u32) -> Option<()> {
        if depth > MAX_DEPTH {
            return None;
        }
        self.deps.push(comp.id.as_str().to_owned());
        self.fingerprint
            .push_str(&serde_json::to_string(comp).ok()?);
        for layer in &comp.layers {
            self.deps.push(layer.id.as_str().to_owned());
            let (kind, params) = evaluate(self.nodes, layer, t).ok()?;
            if !kind.traits().deterministic {
                return None;
            }
            self.asset(&params)?;
            for effect in layer.effects.iter().filter(|e| e.enabled) {
                self.deps.push(effect.id.as_str().to_owned());
                self.node(&effect.node, t)?;
            }
            match kind.id() {
                NESTED_KIND => self.visit_nested(layer, &params, t, depth)?,
                SEQUENCE_KIND => self.visit_sequence(&params, t, depth)?,
                _ => {}
            }
        }
        Some(())
    }

    /// パラメータの素材を依存に、ハッシュと読むデコーダーを指紋に入れる。
    fn asset(&mut self, params: &Params) -> Option<()> {
        if let Some(id) = asset_of(params) {
            let hash = content_hash(self.env.project, id)?;
            let decoder = (self.env.decoder)(id);
            self.deps.push(id.as_str().to_owned());
            self.fingerprint
                .push_str(&format!("|{id}={hash}@{decoder}"));
        }
        Some(())
    }

    /// レイヤーの外にあるノード（効果、カット、トランジション）。種類が決定的で、素材が引けること。
    fn node(&mut self, node: &Node, t: RationalTime) -> Option<Params> {
        let kind = self.nodes.get(&node.kind)?;
        if !kind.traits().deterministic {
            return None;
        }
        let params = node_params_at(node, t).with_defaults(&kind.params());
        self.asset(&params)?;
        Some(params)
    }

    /// シーケンスはコンポジションの JSON の外にあるので、中身（カット、トランジション）と素材も指紋と依存に入れる。
    fn visit_sequence(&mut self, params: &Params, t: RationalTime, depth: u32) -> Option<()> {
        let name = params.get(SEQUENCE_PARAM).and_then(|v| v.as_text())?;
        if name.is_empty() || !self.seen.insert(format!("sequence:{name}")) {
            return Some(());
        }
        let seq = self.env.project.sequence(&Id::new(name)).ok()?;
        self.deps.push(seq.id.as_str().to_owned());
        self.fingerprint.push_str(&serde_json::to_string(seq).ok()?);
        for cut in &seq.cuts {
            self.deps.push(cut.id.as_str().to_owned());
            if let Some(tr) = &cut.transition_in {
                self.node(&tr.node, t)?;
            }
            let params = self.node(&cut.source, t)?;
            if cut.source.kind == NESTED_KIND {
                self.visit_nested_params(&params, t, depth)?;
            }
        }
        Some(())
    }

    fn visit_nested(
        &mut self,
        layer: &Layer,
        params: &Params,
        t: RationalTime,
        depth: u32,
    ) -> Option<()> {
        let inner = source_time(layer, t).ok()?;
        self.visit_nested_params(params, inner, depth)
    }

    /// 入れ子の種類のパラメータが指すコンポジションを、入れ子の中の時刻 `t` で辿る。
    fn visit_nested_params(&mut self, params: &Params, t: RationalTime, depth: u32) -> Option<()> {
        let name = params.get(NESTED_PARAM).and_then(|v| v.as_text())?;
        if name.is_empty() || !self.seen.insert(name.to_owned()) {
            return Some(());
        }
        let id: CompositionId = Id::new(name);
        let nested = self.env.project.composition(&id).ok()?;
        self.visit(nested, t, depth + 1)
    }
}
