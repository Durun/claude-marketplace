//! Transition パネルのアプリ側の処理。開いているコンポジションの sequence のカットを、トランジションの種類と長さつきで返す。
//! Tauri のコマンドは `transition_ipc.rs`、名前と引数の形は `ui/sdk/README.md` の「Transition のコマンド」。

use mm_doc::{Project, Transition};
use mm_pack::Pack;
use mm_types::{CompositionId, RationalTime};
use serde::Serialize;

use crate::win::timebase;

/// カット 1 つの行。時間は秒。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CutRow {
    pub id: String,
    /// sequence の中でのカットの頭。無効なカットは詰めて数える（`sequence_duration` と同じ）。
    pub start: f64,
    pub duration: f64,
    /// 前のカットとの境のトランジション。
    pub transition: Option<TransitionRow>,
}

/// 置いてあるトランジション。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TransitionRow {
    /// パックのトランジションの名前。パックのコードと一致しなければ `None`（パックに無い、書き換えた）。
    pub kind: Option<String>,
    pub duration: f64,
}

/// 開いているコンポジションの sequence とそのカット。sequence が無ければ `sequence` は `None`。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CutsState {
    pub sequence: Option<String>,
    pub cuts: Vec<CutRow>,
}

/// トランジションのノードのコード（`transition.wgsl` の `code` パラメータ）に当たるパックの名前。
fn kind_of(tr: &Transition, pack: Option<&Pack>) -> Option<String> {
    let code = tr.node.text_param("code")?;
    pack?
        .transitions
        .code
        .iter()
        .find_map(|(name, c)| (c == code).then(|| name.clone()))
}

/// コンポジションの先頭の sequence のレイヤーが置く sequence のカットを、頭の位置つきで並べる。
pub fn cuts_of(
    project: &Project,
    comp: &CompositionId,
    pack: Option<&Pack>,
) -> Result<CutsState, String> {
    let sequence = project.composition(comp).ok().and_then(|c| {
        c.layers
            .iter()
            .find_map(|l| timebase::layer_sequence(project, l))
    });
    let Some(sequence) = sequence else {
        return Ok(CutsState {
            sequence: None,
            cuts: Vec::new(),
        });
    };
    let mut at = RationalTime::ZERO;
    let mut cuts = Vec::new();
    for cut in &sequence.cuts {
        let duration = cut.duration().map_err(|e| e.to_string())?;
        cuts.push(CutRow {
            id: cut.id.to_string(),
            start: at.to_f64(),
            duration: duration.to_f64(),
            transition: cut.transition_in.as_ref().map(|tr| TransitionRow {
                kind: kind_of(tr, pack),
                duration: tr.duration.to_f64(),
            }),
        });
        if cut.enabled {
            at = at.checked_add(duration).map_err(|e| e.to_string())?;
        }
    }
    Ok(CutsState {
        sequence: Some(sequence.id.to_string()),
        cuts,
    })
}

#[cfg(test)]
mod tests;
