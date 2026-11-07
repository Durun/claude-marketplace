//! マグネティックシーケンス（種類 `sequence`）の展開。
//!
//! シーケンスのレイヤーは、その時刻にあるカットを 1 枚の画布に描いたもの。カットは `Cut.source` のノードを、カットの
//! 位置・イン点・速さを持つレイヤーとして描く（`layer_item`）。カットの境のトランジション（`Cut.transition_in`）は
//! 境を中心に前後へ半分ずつ掛け、境の前では前のカットを後ろへ、境の後では次のカットを前へ延ばして描く
//! （掛かる範囲は [`crate::crossfade`]）。

use std::sync::Arc;

use mm_doc::{Cut, Framing, Layer, Sequence, SEQUENCE_PARAM};
use mm_eval::{node_params_at, place_cuts, reframe_at, source_time, PlacedCut};
use mm_node_api::{GpuTexture, NodeKind};
use mm_types::{Id, MmError, ParamValue, Params, RationalTime, Rect, Result};

use crate::crossfade::{crossfade_at, handles_in, rewind_rate, Crossfade};
use crate::effects::NodeRun;
use crate::layers::{CompCx, LayerImage, Prepared, MAX_DEPTH};
use crate::renderer::WgpuRenderer;

/// トランジションが担う進み具合のパラメータの名前（`transition.*` の規約）。
const PROGRESS_PARAM: &str = "progress";
/// 掛かる長さ（秒）のパラメータの名前。種類のパラメータにあるときだけ書く。
const DURATION_PARAM: &str = "duration";

/// 境のトランジションを描く手順。種類は `Transition.node.kind` で引いたもの。
struct Plan {
    fade: Crossfade,
    kind: Arc<dyn NodeKind>,
    params: Params,
    /// 出ていくカットを逆向きに描く倍率。0 なら普通に描く。
    rewind: f64,
}

/// 逆向きの時刻を有理数に丸める刻み（1 秒あたり）。24、25、30、48、50、60 fps のコマの境を割り切る。
const REWIND_TICKS: u32 = 120_000;

/// 逆向きに描くときの出ていくカットの時刻。掛かり始めから `rate` 倍で戻り、カットの頭で止まる。
fn rewound_time(placed: &PlacedCut, fade: &Crossfade, t: RationalTime, rate: f64) -> RationalTime {
    let back = fade.start - (t.to_f64() - fade.start) * rate;
    let ticks = (back * f64::from(REWIND_TICKS)).round() as i128;
    RationalTime::new(ticks, REWIND_TICKS)
        .map_or(placed.range.start, |time| time.max(placed.range.start))
}

/// 描く sequence と、それを置いたレイヤーのコンポジションごとの見せ方（再フレーミング）。
type Shown<'a> = (&'a Sequence, &'a Framing);

/// カットを、シーケンスの時間軸の上にある 1 枚のレイヤーとして見たもの。
pub(crate) fn cut_layer(cut: &Cut, placed: &PlacedCut) -> Layer {
    Layer {
        speed: cut.speed,
        ..Layer::new(
            cut.id.as_str(),
            cut.source.clone(),
            placed.range.start,
            cut.in_point,
            cut.out_point,
        )
    }
}

impl WgpuRenderer {
    /// シーケンスのレイヤーの画像。レイヤーの時刻にあるカット（境ではトランジションで混ぜた 2 つ）を画布に描く。
    /// どのカットにも掛からない時刻は `None`。
    pub(crate) fn sequence_image(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        layer: &Layer,
        params: &Params,
    ) -> Result<Option<LayerImage>> {
        let name = params
            .get(SEQUENCE_PARAM)
            .and_then(ParamValue::as_text)
            .unwrap_or("");
        if name.is_empty() {
            return Ok(None);
        }
        if cx.depth >= MAX_DEPTH {
            return Err(MmError::InvalidArgument(format!(
                "入れ子が {MAX_DEPTH} 段を超えた（循環の疑い）"
            )));
        }
        let seq = cx.project.sequence(&Id::new(name))?;
        let t = source_time(layer, cx.t)?;
        let placed = place_cuts(seq)?;
        let Some(pos) = placed.iter().position(|p| p.range.contains(t)) else {
            return Ok(None);
        };
        let child = CompCx {
            t,
            depth: cx.depth + 1,
            ..*cx
        };
        let texture = match self.crossfade_plan(seq, &child, &placed, pos)? {
            Some(plan) => {
                self.encode_crossfade(encoder, &child, (seq, &layer.framing), &placed, &plan)?
            }
            None => self.cut_canvas(encoder, &child, (seq, &layer.framing), &placed[pos])?,
        };
        Ok(Some(LayerImage {
            texture,
            size: cx.size,
            place: Rect::new(0.0, 0.0, cx.comp.width as f32, cx.comp.height as f32),
            cached: false,
        }))
    }

    /// 時刻にトランジションが掛かっていて、その種類が使えるなら、掛け方と種類とパラメータ。
    /// 種類が無ければエラーを記録して、トランジション無し（カットのまま）にする。
    fn crossfade_plan(
        &mut self,
        seq: &Sequence,
        cx: &CompCx<'_>,
        placed: &[PlacedCut],
        pos: usize,
    ) -> Result<Option<Plan>> {
        let handles = |cut: &Cut| handles_in(cx.project, cut);
        let Some(fade) = crossfade_at(seq, placed, pos, cx.t, &handles)? else {
            return Ok(None);
        };
        let Some(tr) = &seq.cuts[placed[fade.to].index].transition_in else {
            return Ok(None);
        };
        let Some(kind) = cx.nodes.get(&tr.node.kind) else {
            self.errors.push(format!(
                "sequence {}: トランジションの種類 {} が無い",
                seq.id, tr.node.kind
            ));
            return Ok(None);
        };
        let schema = kind.params();
        let mut params = node_params_at(&tr.node, cx.t).with_defaults(&schema);
        params.set(PROGRESS_PARAM, ParamValue::Number(f64::from(fade.progress)));
        if schema.get(DURATION_PARAM).is_some() {
            params.set(DURATION_PARAM, ParamValue::Number(fade.length));
        }
        Ok(Some(Plan {
            fade,
            kind,
            params,
            rewind: rewind_rate(tr),
        }))
    }

    /// カット 1 つを画布の大きさのテクスチャに描く。再フレーミングがあればカットの画像に掛ける。
    /// 描けなかったら透明の画布にして、エラーを記録する。
    fn cut_canvas(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        (seq, framing): Shown<'_>,
        placed: &PlacedCut,
    ) -> Result<u64> {
        let cut = &seq.cuts[placed.index];
        let mut layer = cut_layer(cut, placed);
        if let Some(transform) = reframe_at(framing, cut, placed, cx.t)? {
            layer.transform = transform;
        }
        let target = self.gpu.create(cx.size)?;
        let mut temps = Vec::new();
        let items: Vec<Prepared> = match self.layer_item(encoder, cx, &layer, &mut temps) {
            Ok(item) => item.into_iter().collect(),
            Err(e) => {
                self.errors
                    .push(format!("sequence {}: cut {}: {e}", seq.id, cut.id));
                Vec::new()
            }
        };
        self.composite(encoder, target, &items);
        for id in temps {
            self.gpu.release_id(id);
        }
        Ok(target)
    }

    /// 出ていくカットと入ってくるカットを別々の画布に描き、トランジションの種類で混ぜる。
    fn encode_crossfade(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        shown: Shown<'_>,
        placed: &[PlacedCut],
        plan: &Plan,
    ) -> Result<u64> {
        let before = &placed[plan.fade.from];
        let from = if plan.rewind > 0.0 {
            let back = CompCx {
                t: rewound_time(before, &plan.fade, cx.t, plan.rewind),
                ..*cx
            };
            self.cut_canvas(encoder, &back, shown, before)?
        } else {
            self.cut_canvas(encoder, cx, shown, before)?
        };
        let to = self.cut_canvas(encoder, cx, shown, &placed[plan.fade.to])?;
        let run = NodeRun {
            kind: plan.kind.as_ref(),
            params: &plan.params,
            inputs: &[from, to].map(|id| GpuTexture { id, size: cx.size }),
            size: cx.size,
        };
        let mixed = self.run_node(encoder, cx, &run);
        // 恒等の種類は入力をそのまま返すので、返ってきた画布は解放しない。
        let kept = mixed.as_ref().ok().copied().flatten();
        for id in [from, to].into_iter().filter(|id| Some(*id) != kept) {
            self.gpu.release_id(id);
        }
        match mixed? {
            Some(id) => Ok(id),
            None => {
                let blank = self.gpu.create(cx.size)?;
                self.composite(encoder, blank, &[]);
                Ok(blank)
            }
        }
    }
}
