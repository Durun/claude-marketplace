//! 命令の束 [`Op`] と、命令ごとの実装の口 [`Apply`]。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::Result;

use crate::context::OpContext;
use crate::error::{OpError, OpResult};
use crate::ops::*;
use crate::touches::Touches;

/// 命令ごとの実装。`ops/` の各ファイルが実装する。
pub(crate) trait Apply {
    /// 読む・書く所の ID。適用の前の文書で決める。
    fn touches(&self, project: &Project) -> Result<Touches>;

    /// 文書に当て、逆命令を返す。失敗したら文書を途中のままにしてよい（トランザクションが戻す）。
    fn apply(&self, project: &mut Project) -> Result<Op>;
}

/// 命令を当てた結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// 当てた命令を打ち消す命令。
    pub inverse: Op,
    /// 触った所。
    pub touches: Touches,
}

macro_rules! define_ops {
    ($( $(#[$doc:meta])* $variant:ident => $name:literal, )*) => {
        /// 命令。JSON では `op` の欄に名前（camelCase）、残りの欄に引数。
        /// 引数の大きさが枝ごとに違うので、全部 `Box` で持つ。
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
        pub enum Op {
            $( $(#[$doc])* $variant(Box<$variant>), )*
        }

        impl Op {
            /// 命令の名前（JSON の `op` の値）。
            pub fn name(&self) -> &'static str {
                match self { $( Self::$variant(_) => $name, )* }
            }

            /// 全部の命令の名前。
            pub const NAMES: &'static [&'static str] = &[ $( $name, )* ];

            /// 読む・書く所の ID。
            pub fn touches(&self, project: &Project) -> Result<Touches> {
                match self { $( Self::$variant(x) => x.touches(project), )* }
            }

            fn apply_inner(&self, project: &mut Project) -> Result<Op> {
                match self { $( Self::$variant(x) => x.apply(project), )* }
            }
        }

        $( impl From<$variant> for Op {
            fn from(x: $variant) -> Self { Self::$variant(Box::new(x)) }
        } )*
    };
}

define_ops! {
    /// レイヤーを足す。
    AddLayer => "addLayer",
    /// レイヤーを外す。
    RemoveLayer => "removeLayer",
    /// レイヤーの重なりの位置を変える。
    MoveLayer => "moveLayer",
    /// パラメータの固定の値を置く。
    SetParam => "setParam",
    /// パラメータを外す（無くてもよい欄だけ）。
    RemoveParam => "removeParam",
    /// キーフレームを置く。
    SetKeyframe => "setKeyframe",
    /// キーフレームを外す。
    RemoveKeyframe => "removeKeyframe",
    /// キーフレームの補間とイージングを変える。
    SetEase => "setEase",
    /// 親のレイヤーを付ける・外す。
    ParentLayer => "parentLayer",
    /// カットへの接続を付ける・外す。
    AttachLayer => "attachLayer",
    /// ロックを付ける・外す。
    LockLayer => "lockLayer",
    /// レイヤーの組を新しいコンポジションに入れ、入れ子のレイヤーで置き換える。
    Precompose => "precompose",
    /// 入れ子のレイヤーを中のレイヤーに戻す（precompose の逆）。
    Unprecompose => "unprecompose",
    /// コンポジションを足す。
    AddComposition => "addComposition",
    /// コンポジションを外す。
    RemoveComposition => "removeComposition",
    /// sequence を足す。
    AddSequence => "addSequence",
    /// sequence を参照か複製で分岐する。
    ForkSequence => "forkSequence",
    /// sequence を外す。
    RemoveSequence => "removeSequence",
    /// レンダーターゲットを足す。
    AddRenderTarget => "addRenderTarget",
    /// レンダーターゲットを外す。
    RemoveRenderTarget => "removeRenderTarget",
    /// マーカーを足す。
    AddMarker => "addMarker",
    /// マーカーを外す。
    RemoveMarker => "removeMarker",
    /// カットを並びに差し込む（後ろがずれる）。
    Insert => "insert",
    /// カットを別のカットで置き換える（数は変わらない）。
    Overwrite => "overwrite",
    /// カットを抜く（後ろが詰まる）。
    Extract => "extract",
    /// カットを 2 つに割る。
    Split => "split",
    /// 続きの 2 つのカットを 1 つに戻す。
    Join => "join",
    /// カットの頭か尻を動かし、後ろをずらす。
    Ripple => "ripple",
    /// 次のカットとの境目を動かす（全体の長さは変わらない）。
    Roll => "roll",
    /// カットの中身をずらす（位置と長さは変わらない）。
    Slip => "slip",
    /// カットを前後のカットの中で動かす（全体の長さは変わらない）。
    Slide => "slide",
    /// カットを配置から外す・戻す。
    Disable => "disable",
}

impl Op {
    /// 命令を 1 つ当てる。失敗したら文書は変わらない。
    ///
    /// Claude の入口からは、ロックの付いた所に触る命令を断る。当てた後に `Project::validate` を通す。
    pub fn apply(&self, project: &mut Project, ctx: &OpContext) -> OpResult<Applied> {
        let before = project.clone();
        match self.apply_unchecked(project, ctx) {
            Ok(applied) => Ok(applied),
            Err(e) => {
                *project = before;
                Err(e)
            }
        }
    }

    /// 戻さない版。トランザクションが途中の失敗をまとめて戻す。
    pub(crate) fn apply_unchecked(
        &self,
        project: &mut Project,
        ctx: &OpContext,
    ) -> OpResult<Applied> {
        let touches = self.touches(project)?;
        if ctx.entry.is_claude() {
            let locked = touches.intersection(&locked_ids(project));
            if !locked.is_empty() {
                return Err(OpError::Locked(locked));
            }
        }
        let inverse = self.apply_inner(project)?;
        project.validate()?;
        Ok(Applied { inverse, touches })
    }
}

/// ロックの付いたレイヤーと、その効果・マスクの ID。
pub(crate) fn locked_ids(project: &Project) -> Touches {
    let mut t = Touches::new();
    for layer in project
        .compositions
        .iter()
        .flat_map(|c| c.layers.iter())
        .filter(|l| l.lock)
    {
        t.add(&layer.id);
        for e in &layer.effects {
            t.add(&e.id);
        }
        for m in &layer.masks {
            t.add(&m.id);
        }
    }
    t
}
