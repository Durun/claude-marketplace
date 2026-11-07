//! 操作の種と、プロジェクトのデータの ID への当てはめ。性質テストが使う。
//!
//! 操作の組ごとに 1 ファイル（種の enum、生成器、当てはめ）を持つ。操作を足すときは、その組のファイルだけを触る。
//! 新しい組を作るときだけ、ここに `mod`、`Spec` の変種、`arb_spec` の重み、`materialize` の腕を 1 行ずつ足す。

mod bins;
mod build;
mod doc;
mod mask;
mod mixer;
mod params;
mod seq;
mod shorts;

use mm_doc::Project;
use mm_ops::Op;
use proptest::prelude::*;

/// 操作の種。プロジェクトのデータを見ずに作り、[`materialize`] でプロジェクトのデータの ID に当てはめる。
#[derive(Debug, Clone)]
pub enum Spec {
    /// レイヤー、パラメータ、コンポジション、レンダーターゲット、アセット、マーカー。
    Doc(doc::DocSpec),
    /// sequence とカット。
    Seq(seq::SeqSpec),
    /// 段階 3 の操作。
    Bin(bins::BinSpec),
    /// 本編とショートの操作。
    Short(shorts::ShortSpec),
    /// レイヤーの音の設定とコンポジションのミキサー。
    Mix(mixer::MixSpec),
    /// レイヤーのマスク。
    Mask(mask::MaskSpec),
}

pub fn arb_spec() -> impl Strategy<Value = Spec> {
    prop_oneof![
        3 => doc::arb().prop_map(Spec::Doc),
        2 => seq::arb().prop_map(Spec::Seq),
        3 => bins::arb_bin_spec().prop_map(Spec::Bin),
        1 => shorts::arb_short_spec().prop_map(Spec::Short),
        1 => mixer::arb().prop_map(Spec::Mix),
        1 => mask::arb().prop_map(Spec::Mask),
    ]
}

/// 種を今のプロジェクトのデータの ID に当てはめる。当てはめる相手が無ければ `None`。
pub fn materialize(spec: &Spec, p: &Project) -> Option<Op> {
    match spec {
        Spec::Doc(s) => doc::op(s, p),
        Spec::Seq(s) => seq::op(s, p),
        Spec::Bin(s) => bins::bin_op(s, p),
        Spec::Short(s) => shorts::short_op(s, p),
        Spec::Mix(s) => mixer::op(s, p),
        Spec::Mask(s) => mask::op(s, p),
    }
}
