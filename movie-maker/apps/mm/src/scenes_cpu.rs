//! `cpu` シナリオ。GPU も素材も要らないシナリオ（操作の適用、`validate`、保存のコミット）を回す。
//! CI の runner でも測れるので、性能の回帰のゲートはこのシナリオを前後で比べる。

use std::time::{Duration, Instant};

use mm_doc::{Composition, Layer, Node, Project};
use mm_ops::{Op, OpContext, ParamRef, SetParam, Transaction};
use mm_store::{Actor, Store};
use mm_types::{Id, ParamValue, Rational, RationalTime, Result};

use crate::perf::{Budget, Item, Limit, PerfArgs};

const OPS_ROW: &str = "操作の適用（トランザクション、ドラッグ 1 回分）";
const SAVE_ROW: &str = "保存（操作のトランザクションのコミット 1 回）";
const MS_PER_SECOND: f64 = 1000.0;
/// 大きなプロジェクトのデータのレイヤーの数（`mm-ops/tests/perf` と同じ）。
const LAYERS: usize = 2000;
/// 1 回の測定で繰り返す回数。1 回ごとの揺れを均す。
const INNER_ITERATIONS: u32 = 20;
/// 保存のコミットを 1 回の測定で繰り返す回数。
const COMMITS: usize = 30;
/// 操作の適用の予算（ミリ秒）。
const OPS_BUDGET_MS: f64 = 20.0;
/// 保存のコミットの予算（ミリ秒）。
const SAVE_BUDGET_MS: f64 = 20.0;
/// 操作の対象にするレイヤーの番号。
const TARGET_LAYER: usize = 1500;

/// 5 個ごとに前のレイヤーを親にした `LAYERS` 個のレイヤーを持つプロジェクトのデータ。
fn big_project() -> Project {
    let layers = (0..LAYERS)
        .map(|i| {
            let node = Node::new("solid", 1);
            let mut layer = Layer::new(
                format!("l{i}"),
                node,
                RationalTime::ZERO,
                RationalTime::ZERO,
                RationalTime::seconds(2),
            );
            if i % 5 != 0 {
                layer.parent = Some(Id::new(format!("l{}", i - 1)));
            }
            layer
        })
        .collect();
    let mut project = Project::new();
    project.compositions.push(
        Composition::new(
            Id::new("c"),
            320,
            180,
            Rational::integer(30),
            RationalTime::seconds(10),
        )
        .with_layers(layers),
    );
    project.normalize();
    project
}

fn set_opacity(value: f64) -> Op {
    SetParam {
        target: ParamRef::layer(&Id::new(format!("l{TARGET_LAYER}")), "opacity"),
        value: ParamValue::Number(value),
        keyframes: None,
    }
    .into()
}

fn mean_ms(iterations: u32, mut f: impl FnMut() -> Result<()>) -> Result<f64> {
    let start = Instant::now();
    for _ in 0..iterations {
        f()?;
    }
    Ok(duration_ms(start.elapsed()) / f64::from(iterations))
}

fn duration_ms(d: Duration) -> f64 {
    d.as_secs_f64() * MS_PER_SECOND
}

/// 操作の適用 1 回あたりの平均（ミリ秒）。適用は書き換えるので繰り返しごとに複製が要るが、
/// 2000 レイヤーの複製が時間に入ると回帰が埋もれるため、先に作っておいて適用だけを測る。
fn apply_ms(project: &Project, op: &Op, ctx: &OpContext) -> Result<f64> {
    let mut copies: Vec<Project> = (0..INNER_ITERATIONS).map(|_| project.clone()).collect();
    let start = Instant::now();
    for p in &mut copies {
        op.apply(p, ctx)
            .map_err(|e| mm_types::MmError::Internal(format!("適用に失敗: {e}")))?;
    }
    Ok(duration_ms(start.elapsed()) / f64::from(INNER_ITERATIONS))
}

/// 保存のコミット 1 回あたりの平均（ミリ秒）。毎回違う値を書き、差分が空にならないようにする。
fn commit_ms(project: &Project) -> Result<f64> {
    let dir = tempfile::tempdir()?;
    let mut store = Store::create(dir.path(), project, 0)?;
    let ctx = OpContext::human();
    let actor = Actor {
        kind: "perf".into(),
        name: "mm".into(),
    };
    let start = Instant::now();
    for i in 0..COMMITS {
        let opacity = 0.1 + (i % 9) as f64 / 10.0;
        let txn = Transaction::new("perf", vec![set_opacity(opacity)]);
        store
            .apply_txn(txn, &ctx, actor.clone(), i as i64, None)
            .map_err(|e| mm_types::MmError::Internal(format!("コミットに失敗: {e}")))?;
    }
    Ok(duration_ms(start.elapsed()) / COMMITS as f64)
}

/// `cpu`: 操作の適用、`validate`、保存のコミットの 1 回あたりの時間。
pub fn cpu(args: &PerfArgs) -> Result<Vec<Item>> {
    let project = big_project();
    project.validate()?;
    let ctx = OpContext::human();
    let op = set_opacity(0.5);
    let (mut apply, mut validate, mut commit) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..args.repeat {
        apply.push(apply_ms(&project, &op, &ctx)?);
        validate.push(mean_ms(INNER_ITERATIONS, || project.validate())?);
        commit.push(commit_ms(&project)?);
    }
    let budget = |ms, row| {
        Some(Budget {
            limit: Limit::AtMost(ms),
            row,
        })
    };
    Ok(vec![
        Item::from_samples("ops.apply_ms", "ms", apply, budget(OPS_BUDGET_MS, OPS_ROW)),
        Item::from_samples("ops.validate_ms", "ms", validate, None),
        Item::from_samples(
            "store.commit_ms",
            "ms",
            commit,
            budget(SAVE_BUDGET_MS, SAVE_ROW),
        ),
    ])
}
