//! 本編とショートの関係。
//!
//! - 参照のショート（`OriginRelation::Reference`）のカットは、元のカットの値を写して持つ。描画と書き出しは
//!   ショートのカットをそのまま読めばよく、元を引き直さない。写すのは [`Project::sync_references`] で、操作の層が
//!   操作を 1 つ適用するたびに通す。上書きした in/out（`Cut.overrides`）だけは写さない。
//! - 複製のショート（`OriginRelation::Fork`）のカットは、複製したときの元の値（`Cut.origin_base`）を持ち、
//!   [`Project::origin_changes`] が今の元と比べて変わったカットを返す。
//! - [`Project::short_uses`] は本編のカットごとに、どのショートのどのカットが使っているかを返す。

use std::collections::{BTreeMap, HashMap};

use mm_types::{MmError, RationalTime, Result};

use crate::sequence::{Cut, CutBase, CutId, CutOverrides, OriginRelation, SequenceId};
use crate::Project;

/// 参照の鎖（ショートのショート）をたどる上限。`validate` が無い元を拒否するので、循環は無い。
const MAX_ORIGIN_DEPTH: usize = 64;

/// 操作を適用する前の、参照のショートのカットの値。カットを直接直したか（拒否する）を見分けるのに使う。
#[derive(Debug, Clone, Default)]
pub struct ReferenceSnapshot(HashMap<CutId, Cut>);

/// 本編のカットを使うショートのカット 1 つ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortUse {
    pub short: SequenceId,
    pub relation: OriginRelation,
    pub cut: CutId,
    /// 参照のショートで上書きした所。複製は常に全部が偽。
    pub overrides: CutOverrides,
}

/// 複製のショートのカットの元の変化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OriginChange {
    pub cut: CutId,
    pub origin: CutId,
    /// 変わったフィールドの名前。元のカットが消えたら空。
    pub fields: Vec<&'static str>,
    /// 元のカットが消えた。
    pub removed: bool,
}

impl Project {
    /// 参照のショートのカットの今の値。
    pub fn reference_snapshot(&self) -> ReferenceSnapshot {
        let cuts = self
            .sequences
            .iter()
            .filter(|s| relation_of(self, &s.id) == Some(OriginRelation::Reference))
            .flat_map(|s| s.cuts.iter())
            .filter(|c| c.origin.is_some())
            .map(|c| (c.id.clone(), c.clone()))
            .collect();
        ReferenceSnapshot(cuts)
    }

    /// 参照のショートのカットに元のカットの値を写す。上書きした in/out は写さない。
    ///
    /// `before` は操作の前の値。ショートのカットの、元に従う所（上書きしていない in/out、素材、速さ、有効）を
    /// 直接変えた操作は `InvalidArgument` で拒否する（写すと黙って戻ってしまうため）。元のカットが無いカットは触らない。
    pub fn sync_references(&mut self, before: &ReferenceSnapshot) -> Result<()> {
        for id in self.reference_order() {
            let Some(origin) = self.sequence(&id)?.origin.clone() else {
                continue;
            };
            let main: HashMap<CutId, Cut> = self
                .sequence(&origin.sequence)?
                .cuts
                .iter()
                .map(|c| (c.id.clone(), c.clone()))
                .collect();
            for cut in &mut self.sequence_mut(&id)?.cuts {
                let Some(m) = cut.origin.as_ref().and_then(|o| main.get(o)) else {
                    continue;
                };
                sync_cut(cut, before.0.get(&cut.id), m)?;
            }
        }
        Ok(())
    }

    /// 本編 `main` のカットごとの、それを使うショートのカット（ショートの ID の順）。
    pub fn short_uses(&self, main: &SequenceId) -> BTreeMap<CutId, Vec<ShortUse>> {
        let mut out: BTreeMap<CutId, Vec<ShortUse>> = BTreeMap::new();
        let mut shorts: Vec<_> = self
            .sequences
            .iter()
            .filter_map(|s| s.origin.as_ref().map(|o| (s, o)))
            .filter(|(_, o)| &o.sequence == main)
            .collect();
        shorts.sort_by(|a, b| a.0.id.cmp(&b.0.id));
        for (short, origin) in shorts {
            for cut in &short.cuts {
                let Some(o) = &cut.origin else { continue };
                out.entry(o.clone()).or_default().push(ShortUse {
                    short: short.id.clone(),
                    relation: origin.relation,
                    cut: cut.id.clone(),
                    overrides: cut.overrides,
                });
            }
        }
        out
    }

    /// ショート `short` のカットのうち、元のカットが変わったか消えたもの（カットの順）。
    ///
    /// 複製は `Cut.origin_base` と今の元を比べる。参照は元に従うので、元が消えたカットだけを返す。
    pub fn origin_changes(&self, short: &SequenceId) -> Result<Vec<OriginChange>> {
        let seq = self.sequence(short)?;
        let Some(origin) = &seq.origin else {
            return Ok(Vec::new());
        };
        let main = self.sequence(&origin.sequence)?;
        let changes = seq.cuts.iter().filter_map(|cut| {
            let o = cut.origin.as_ref()?;
            let change = |fields, removed| OriginChange {
                cut: cut.id.clone(),
                origin: o.clone(),
                fields,
                removed,
            };
            let Some(m) = main.cuts.iter().find(|c| &c.id == o) else {
                return Some(change(Vec::new(), true));
            };
            let base = cut.origin_base.as_ref()?;
            let fields = base.changed_fields(&CutBase::of(m));
            (!fields.is_empty()).then(|| change(fields, false))
        });
        Ok(changes.collect())
    }

    /// 参照のショートの ID を、元が先に来る順に並べる（ショートのショートは元のショートを写した後に写す）。
    fn reference_order(&self) -> Vec<SequenceId> {
        let mut refs: Vec<(usize, SequenceId)> = self
            .sequences
            .iter()
            .filter(|s| relation_of(self, &s.id) == Some(OriginRelation::Reference))
            .map(|s| (origin_depth(self, &s.id), s.id.clone()))
            .collect();
        refs.sort();
        refs.into_iter().map(|(_, id)| id).collect()
    }
}

fn relation_of(project: &Project, id: &SequenceId) -> Option<OriginRelation> {
    let seq = project.sequence(id).ok()?;
    let origin = seq.origin.as_ref()?;
    (origin.sequence != *id).then_some(origin.relation)
}

fn origin_depth(project: &Project, id: &SequenceId) -> usize {
    let mut depth = 0;
    let mut cur = id.clone();
    while let Some(o) = project.sequence(&cur).ok().and_then(|s| s.origin.clone()) {
        depth += 1;
        if depth >= MAX_ORIGIN_DEPTH {
            break;
        }
        cur = o.sequence;
    }
    depth
}

/// カット 1 つに元の値を写す。`prev` が無いカットはこの操作で足したもので、元と同じ値で足していること。
fn sync_cut(cut: &mut Cut, prev: Option<&Cut>, main: &Cut) -> Result<()> {
    let base = prev.unwrap_or(main);
    if cut.source != base.source || cut.speed != base.speed || cut.enabled != base.enabled {
        return Err(follow_error(cut, "素材と速さと有効"));
    }
    cut.source = main.source.clone();
    cut.speed = main.speed;
    cut.enabled = main.enabled;
    let prev_point = |f: fn(&Cut) -> (bool, RationalTime)| prev.map(f);
    let in_point = sync_point(
        (cut.overrides.in_point, cut.in_point),
        prev_point(|p| (p.overrides.in_point, p.in_point)),
        main.in_point,
    )
    .ok_or_else(|| follow_error(cut, "上書きしていない in"))?;
    let out_point = sync_point(
        (cut.overrides.out_point, cut.out_point),
        prev_point(|p| (p.overrides.out_point, p.out_point)),
        main.out_point,
    )
    .ok_or_else(|| follow_error(cut, "上書きしていない out"))?;
    cut.in_point = in_point;
    cut.out_point = out_point;
    Ok(())
}

/// 時刻 1 つの写した後の値。上書きしていれば今の値、していなければ元の値。
/// 上書きしていない値をこの操作が元と違う値に変えたら `None`。
fn sync_point(
    (overridden, value): (bool, RationalTime),
    prev: Option<(bool, RationalTime)>,
    main: RationalTime,
) -> Option<RationalTime> {
    if overridden {
        return Some(value);
    }
    let edited = match prev {
        Some((was_overridden, was)) => !was_overridden && value != was,
        None => true,
    };
    (!edited || value == main).then_some(main)
}

fn follow_error(cut: &Cut, what: &str) -> MmError {
    MmError::InvalidArgument(format!(
        "カット {} は参照のショートのカットで、{what}は元のカットに従う（in/out は overrideCut で上書きする）",
        cut.id
    ))
}

#[cfg(test)]
#[path = "shorts_tests.rs"]
mod tests;
