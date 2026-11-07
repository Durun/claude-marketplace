//! 操作の enum [`Op`] と、操作ごとの実装のインターフェース [`Apply`]。

use serde::{Deserialize, Serialize};

use mm_doc::Project;
use mm_types::Result;

use crate::context::OpContext;
use crate::error::{OpError, OpResult};
use crate::ops::*;
use crate::shorts::add_short_links;
use crate::touches::Touches;

/// 操作ごとの実装。`ops/` の各ファイルが実装する。
pub(crate) trait Apply {
    /// 読む・書く所の ID。適用の前のプロジェクトのデータで決める。
    fn touches(&self, project: &Project) -> Result<Touches>;

    /// プロジェクトのデータに適用し、逆操作を返す。失敗したらプロジェクトのデータを途中のままにしてよい（トランザクションが戻す）。
    fn apply(&self, project: &mut Project) -> Result<Op>;
}

/// 操作を適用した結果。
#[derive(Debug, Clone, PartialEq)]
pub struct Applied {
    /// 適用した操作を打ち消す操作。
    pub inverse: Op,
    /// 触った所。
    pub touches: Touches,
}

macro_rules! define_ops {
    ($( $(#[$doc:meta])* $variant:ident => $name:literal, )*) => {
        /// 操作。JSON では `op` のフィールドに名前（camelCase）、残りのフィールドに引数。
        /// 引数の大きさがバリアントごとに違うので、全部 `Box` で持つ。
        #[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
        #[serde(tag = "op", rename_all = "camelCase", deny_unknown_fields)]
        pub enum Op {
            $( $(#[$doc])* $variant(Box<$variant>), )*
        }

        impl Op {
            /// 操作の名前（JSON の `op` の値）。
            pub fn name(&self) -> &'static str {
                match self { $( Self::$variant(_) => $name, )* }
            }

            /// 全部の操作の名前。
            pub const NAMES: &'static [&'static str] = &[ $( $name, )* ];

            /// 読む・書く所の ID。参照のショートのカットと元のカットは、片方に触れば両方に触る
            /// （元を直すと反映され、ショートの上書きは元を読む）。
            pub fn touches(&self, project: &Project) -> Result<Touches> {
                let mut t = match self { $( Self::$variant(x) => x.touches(project), )* }?;
                add_short_links(project, &mut t);
                Ok(t)
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
    /// 素材を足す。
    AddAsset => "addAsset",
    /// 素材を外す。
    RemoveAsset => "removeAsset",
    /// レイヤーを足す。
    AddLayer => "addLayer",
    /// レイヤーを外す。
    RemoveLayer => "removeLayer",
    /// レイヤーにマスクを足す。
    AddMask => "addMask",
    /// レイヤーからマスクを外す。
    RemoveMask => "removeMask",
    /// レイヤーの重なりの位置を変える。
    MoveLayer => "moveLayer",
    /// パラメータの固定の値を置く。
    SetParam => "setParam",
    /// パラメータを外す（無くてもよいフィールドだけ）。
    RemoveParam => "removeParam",
    /// キーフレームを置く。
    SetKeyframe => "setKeyframe",
    /// キーフレームを外す。
    RemoveKeyframe => "removeKeyframe",
    /// キーフレームの補間とイージングを変える。
    SetEase => "setEase",
    /// キーフレームの時刻と中身を置き直す（外すと置くを 1 つにする）。
    MoveKeyframe => "moveKeyframe",
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
    /// 参照のショートのカットの in/out を上書きする・上書きを外す。
    OverrideCut => "overrideCut",
    /// 参照のショートのカットの上書きを全部外す。
    ResetCut => "resetCut",
    /// sequence のレイヤーのカットの再フレーミングを置く・外す。
    SetReframe => "setReframe",
    /// sequence のレイヤーの字幕の置き場所を置く・外す。
    SetCaption => "setCaption",
    /// レイヤーの音の設定（送り先のバス、音量、パン、ミュート、ソロ）を置く・外す。
    SetLayerAudio => "setLayerAudio",
    /// コンポジションのミキサーの設定（バスの音量、マスター、ダッキング）を置く・外す。
    SetMixer => "setMixer",
    /// カットの頭のトランジションを置く・差し替える・外す。
    SetTransition => "setTransition",
    /// レンダーターゲットを足す。
    AddRenderTarget => "addRenderTarget",
    /// レンダーターゲットを外す。
    RemoveRenderTarget => "removeRenderTarget",
    /// マーカーを足す。
    AddMarker => "addMarker",
    /// マーカーを外す。
    RemoveMarker => "removeMarker",
    /// カットをシーケンスに差し込む（後ろがずれる）。
    Insert => "insert",
    /// カットを別のカットで置き換える（数は変わらない）。
    Overwrite => "overwrite",
    /// カットを抜く（後ろが詰まる）。
    Extract => "extract",
    /// カットを 2 つに割る。
    Split => "split",
    /// 続きの 2 つのカットを 1 つに戻す。
    Join => "join",
    /// カットのイン点かアウト点を動かし、後ろをずらす。
    Ripple => "ripple",
    /// 次のカットとの編集点を動かす（全体の長さは変わらない）。
    Roll => "roll",
    /// カットの内容をずらす（位置と長さは変わらない）。
    Slip => "slip",
    /// カットを前後のカットの中で動かす（全体の長さは変わらない）。
    Slide => "slide",
    /// カットを配置から外す・戻す。
    Disable => "disable",
    /// カットを抜いて同じ長さの隙間を残す。
    Lift => "lift",
    /// 続いたカットの列を差し込む。
    InsertCuts => "insertCuts",
    /// 続いたカットの列を抜く。
    ExtractCuts => "extractCuts",
    /// 話題の列からカットを作って差し込む。
    InsertTopics => "insertTopics",
    /// sequence のレイヤーとコンポジションの長さを sequence の長さに合わせる。
    FitLayer => "fitLayer",
    /// レイヤーのイン点かアウト点を動かす。
    TrimLayer => "trimLayer",
    /// 素材のイン点を付ける・外す。
    MarkIn => "markIn",
    /// 素材のアウト点を付ける・外す。
    MarkOut => "markOut",
    /// 選別を足す。
    AddSelection => "addSelection",
    /// 選別を外す。
    RemoveSelection => "removeSelection",
    /// 選別か話題の評価を変える。
    Rate => "rate",
    /// 選別か話題にタグを付ける・外す。
    Tag => "tag",
    /// 話題を足す。
    AddTopic => "addTopic",
    /// 話題を外す。
    RemoveTopic => "removeTopic",
    /// 文字起こしの単語を消す・戻す。
    DeleteWords => "deleteWords",
    /// 文字起こしの単語を消し、その音をシーケンスのカットから抜いて詰める。
    CutWords => "cutWords",
    /// 文字起こしの単語の文字を直す。
    FixTranscript => "fixTranscript",
}

impl Op {
    /// 操作を 1 つ適用する。失敗したらプロジェクトのデータは変わらない。
    ///
    /// Claude の操作経路からは、ロックの付いた所に触る操作を拒否する。適用した後に `Project::validate` を通す。
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

    /// 失敗しても戻さない実装。トランザクションが途中の失敗をまとめて戻す。
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
        let before = project.reference_snapshot();
        let inverse = self.apply_inner(project)?;
        project.sync_references(&before)?;
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
