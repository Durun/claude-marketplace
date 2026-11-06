//! マグネティックシーケンス（種類 `sequence`）の展開。
//!
//! シーケンスのレイヤーは、その時刻にあるカットを 1 枚の画布に描いたもの。カットは `Cut.source` のノードを、カットの
//! 位置・イン点・速さを持つレイヤーとして描く（`layer_item`）。カットの境のトランジション（`Cut.transition_in`）は
//! 境を中心に前後へ半分ずつ掛け、境の前では前のカットを後ろへ、境の後では次のカットを前へ延ばして描く。

use std::sync::Arc;

use mm_doc::{Cut, Layer, Sequence, Transition, SEQUENCE_PARAM};
use mm_eval::{node_params_at, place_cuts, source_time, PlacedCut};
use mm_node_api::{GpuTexture, NodeKind};
use mm_types::{Id, MmError, ParamValue, Params, RationalTime, Rect, Result};

use crate::effects::NodeRun;
use crate::layers::{CompCx, LayerImage, Prepared, MAX_DEPTH};
use crate::renderer::WgpuRenderer;

/// トランジションが担う進み具合のパラメータの名前（`transition.*` の約束）。
const PROGRESS_PARAM: &str = "progress";

/// 境のトランジションを描く手順。種類は `Transition.node.kind` で引いたもの。
struct Plan {
    fade: Crossfade,
    kind: Arc<dyn NodeKind>,
    params: Params,
}

/// 境のトランジションの掛かり方。番号は配置したカットの並びの中の位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Crossfade {
    pub from: usize,
    pub to: usize,
    /// 0 で出ていくカットだけ、1 で入ってくるカットだけ。
    pub progress: f32,
}

/// 時刻 `t` が `placed[pos]` の中にあるとき、境のトランジションに掛かっていればその掛かり方を返す。
/// 半分の長さはトランジションの長さの半分だが、両側のカットの長さを超えない。
pub(crate) fn crossfade_at(
    seq: &Sequence,
    placed: &[PlacedCut],
    pos: usize,
    t: RationalTime,
) -> Result<Option<Crossfade>> {
    let tf = t.to_f64();
    let own = &placed[pos];
    if let Some(prev) = pos.checked_sub(1).map(|p| &placed[p]) {
        if let Some(tr) = &seq.cuts[own.index].transition_in {
            let boundary = own.range.start.to_f64();
            let half = half_of(tr, prev, own)?;
            if half > 0.0 && tf - boundary < half {
                return Ok(Some(crossfade(pos - 1, pos, tf, boundary, half)));
            }
        }
    }
    if let Some(next) = placed.get(pos + 1) {
        if let Some(tr) = &seq.cuts[next.index].transition_in {
            let boundary = next.range.start.to_f64();
            let half = half_of(tr, own, next)?;
            if half > 0.0 && boundary - tf <= half {
                return Ok(Some(crossfade(pos, pos + 1, tf, boundary, half)));
            }
        }
    }
    Ok(None)
}

fn crossfade(from: usize, to: usize, t: f64, boundary: f64, half: f64) -> Crossfade {
    Crossfade {
        from,
        to,
        progress: ((t - (boundary - half)) / (2.0 * half)).clamp(0.0, 1.0) as f32,
    }
}

fn half_of(tr: &Transition, before: &PlacedCut, after: &PlacedCut) -> Result<f64> {
    let len = |p: &PlacedCut| p.range.duration().map(|d| d.to_f64());
    Ok((tr.duration.to_f64() / 2.0)
        .min(len(before)?)
        .min(len(after)?))
}

/// カットを、シーケンスの時間軸の上にある 1 枚のレイヤーとして見たもの。
fn cut_layer(cut: &Cut, placed: &PlacedCut) -> Layer {
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
            Some(plan) => self.encode_crossfade(encoder, &child, seq, &placed, &plan)?,
            None => self.cut_canvas(encoder, &child, seq, &placed[pos])?,
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
        let Some(fade) = crossfade_at(seq, placed, pos, cx.t)? else {
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
        let mut params = node_params_at(&tr.node, cx.t).with_defaults(&kind.params());
        params.set(PROGRESS_PARAM, ParamValue::Number(f64::from(fade.progress)));
        Ok(Some(Plan { fade, kind, params }))
    }

    /// カット 1 つを画布の大きさのテクスチャに描く。描けなかったら透明の画布にして、エラーを記録する。
    fn cut_canvas(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        cx: &CompCx<'_>,
        seq: &Sequence,
        placed: &PlacedCut,
    ) -> Result<u64> {
        let cut = &seq.cuts[placed.index];
        let target = self.gpu.create(cx.size)?;
        let mut temps = Vec::new();
        let items: Vec<Prepared> =
            match self.layer_item(encoder, cx, &cut_layer(cut, placed), &mut temps) {
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
        seq: &Sequence,
        placed: &[PlacedCut],
        plan: &Plan,
    ) -> Result<u64> {
        let from = self.cut_canvas(encoder, cx, seq, &placed[plan.fade.from])?;
        let to = self.cut_canvas(encoder, cx, seq, &placed[plan.fade.to])?;
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

#[cfg(test)]
mod tests {
    use mm_doc::Node;

    use super::*;

    fn secs(n: i64) -> RationalTime {
        RationalTime::seconds(n)
    }

    /// 2 秒のカットが 3 つ。2 番目と 3 番目の頭に、長さ `d` 秒のトランジション。
    fn three_cuts(d: i64) -> Sequence {
        let cut = |id: &str| Cut::new(id, Node::new("solid", 1), secs(0), secs(2));
        let mut cuts = vec![cut("a"), cut("b"), cut("c")];
        for c in &mut cuts[1..] {
            c.transition_in = Some(Transition {
                node: Node::new("transition.dissolve", 1),
                duration: secs(d),
            });
        }
        Sequence {
            id: Id::new("s"),
            cuts,
            origin: None,
        }
    }

    fn fade(seq: &Sequence, t: RationalTime) -> Option<Crossfade> {
        let placed = place_cuts(seq).unwrap();
        let pos = placed.iter().position(|p| p.range.contains(t)).unwrap();
        crossfade_at(seq, &placed, pos, t).unwrap()
    }

    #[test]
    fn 境から離れた時刻は掛からない() {
        let seq = three_cuts(1);
        assert_eq!(fade(&seq, RationalTime::new(1, 1).unwrap()), None);
        assert_eq!(fade(&seq, RationalTime::new(11, 8).unwrap()), None);
    }

    #[test]
    fn 境の前は前のカットから次のカットへ進む() {
        let f = fade(&three_cuts(1), RationalTime::new(7, 4).unwrap()).unwrap();
        assert_eq!((f.from, f.to), (0, 1));
        assert!((f.progress - 0.25).abs() < 1e-6, "{f:?}");
    }

    #[test]
    fn 境ちょうどは半分で混ざる() {
        let f = fade(&three_cuts(1), secs(2)).unwrap();
        assert_eq!((f.from, f.to), (0, 1));
        assert!((f.progress - 0.5).abs() < 1e-6, "{f:?}");
    }

    #[test]
    fn 境の後は次のカットの頭で終わる() {
        let f = fade(&three_cuts(1), RationalTime::new(9, 4).unwrap()).unwrap();
        assert_eq!((f.from, f.to), (0, 1));
        assert!((f.progress - 0.75).abs() < 1e-6, "{f:?}");
        assert_eq!(fade(&three_cuts(1), RationalTime::new(5, 2).unwrap()), None);
    }

    #[test]
    fn 最初のカットの頭にはトランジションが掛からない() {
        let mut seq = three_cuts(1);
        seq.cuts[0].transition_in = seq.cuts[1].transition_in.clone();
        assert_eq!(fade(&seq, secs(0)), None);
    }

    #[test]
    fn トランジションの半分は両側のカットの長さを超えない() {
        // 長さ 10 秒のトランジションでも、2 秒のカットの全体（境の前の 2 秒）までしか延びない。
        let f = fade(&three_cuts(10), secs(1)).unwrap();
        assert_eq!((f.from, f.to), (0, 1));
        assert!((f.progress - 0.25).abs() < 1e-6, "{f:?}");
    }

    #[test]
    fn 無効にしたカットを飛ばした隣と混ざる() {
        let mut seq = three_cuts(1);
        seq.cuts[1].enabled = false;
        let f = fade(&seq, secs(2)).unwrap();
        assert_eq!((f.from, f.to), (0, 1), "配置した並びの中の番号");
    }
}
