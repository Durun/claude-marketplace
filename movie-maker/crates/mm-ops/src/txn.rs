//! トランザクション。複数の操作を 1 つのトランザクションとして適用し、途中で失敗したら全部戻す。

use serde::{Deserialize, Serialize};

use mm_doc::Project;

use crate::context::OpContext;
use crate::error::OpResult;
use crate::op::Op;
use crate::touches::Touches;

/// 適用する前のトランザクション。ドラッグの連続、マクロ、Claude の対応 1 回。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Transaction {
    /// 記録に残す名前（「ドラッグ」「Claude の対応」）。
    pub name: String,
    /// 元にしたプロジェクトのデータのバージョン。`None` は今のバージョンを元にする（判定しない）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_version: Option<u64>,
    /// 順に適用する操作。
    pub ops: Vec<Op>,
}

impl Transaction {
    /// 今のバージョンを元にするトランザクション。
    pub fn new(name: impl Into<String>, ops: Vec<Op>) -> Self {
        Self {
            name: name.into(),
            base_version: None,
            ops,
        }
    }

    /// 元にしたバージョンを付ける。
    pub fn based_on(mut self, version: u64) -> Self {
        self.base_version = Some(version);
        self
    }
}

/// 操作 1 つの記録。保存の層が `ops` の表に 1 行で書く形。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OpRecord {
    /// 適用した操作（`op` のフィールドに名前）。
    pub op: serde_json::Value,
    /// 打ち消す操作。
    pub inverse: serde_json::Value,
    /// 触った所の ID。
    pub touches: Vec<String>,
}

impl OpRecord {
    /// 操作と逆操作と touches から。
    pub fn new(op: &Op, inverse: &Op, touches: &Touches) -> serde_json::Result<Self> {
        Ok(Self {
            op: serde_json::to_value(op)?,
            inverse: serde_json::to_value(inverse)?,
            touches: touches.to_vec(),
        })
    }

    /// 操作を型に戻す。
    pub fn op(&self) -> serde_json::Result<Op> {
        serde_json::from_value(self.op.clone())
    }

    /// 逆操作を型に戻す。
    pub fn inverse(&self) -> serde_json::Result<Op> {
        serde_json::from_value(self.inverse.clone())
    }
}

/// トランザクションを適用した結果。
#[derive(Debug, Clone, PartialEq)]
pub struct TxnOutcome {
    /// 操作ごとの記録（適用した順）。
    pub records: Vec<OpRecord>,
    /// トランザクション全体で触った所。
    pub touches: Touches,
}

impl TxnOutcome {
    /// 取り消しのトランザクション。逆操作を逆の順に並べる。
    pub fn inverse_ops(&self) -> serde_json::Result<Vec<Op>> {
        self.records.iter().rev().map(OpRecord::inverse).collect()
    }
}

/// 操作の列を 1 つのトランザクションとして適用する。途中で失敗したらプロジェクトのデータを元に戻して `Err`。
///
/// バージョンの判定（`base_version`）はしない。それは [`crate::Editor`] の仕事で、ここはプロジェクトのデータだけを見る。
pub fn apply_transaction(
    project: &mut Project,
    ops: &[Op],
    ctx: &OpContext,
) -> OpResult<TxnOutcome> {
    let before = project.clone();
    match apply_all(project, ops, ctx) {
        Ok(outcome) => Ok(outcome),
        Err(e) => {
            *project = before;
            Err(e)
        }
    }
}

fn apply_all(project: &mut Project, ops: &[Op], ctx: &OpContext) -> OpResult<TxnOutcome> {
    let mut records = Vec::with_capacity(ops.len());
    let mut touches = Touches::new();
    for op in ops {
        let applied = op.apply_unchecked(project, ctx)?;
        records.push(
            OpRecord::new(op, &applied.inverse, &applied.touches)
                .map_err(|e| crate::error::OpError::Doc(mm_types::MmError::Json(e)))?,
        );
        touches.extend(&applied.touches);
    }
    Ok(TxnOutcome { records, touches })
}
