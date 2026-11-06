//! 素材の整理と文字起こしの直しの操作が共有する、プロジェクトのデータの書き換え。操作ではない。

use mm_doc::{Project, Rating, SourceMarks, WordEdit, WordId};
use mm_types::{AssetId, MmError, RationalTime, Result};

use crate::ops::Edge;

/// 選別か話題の評価とタグ（`rate` と `tag` が ID だけで指す）。
pub(crate) fn item_mut<'a>(
    project: &'a mut Project,
    id: &str,
) -> Result<(&'a mut Rating, &'a mut Vec<String>)> {
    let bins = &mut project.bins;
    if let Some(s) = bins.selections.iter_mut().find(|s| s.id.as_str() == id) {
        return Ok((&mut s.rating, &mut s.tags));
    }
    bins.topics
        .iter_mut()
        .find(|t| t.id.as_str() == id)
        .map(|t| (&mut t.rating, &mut t.tags))
        .ok_or_else(|| MmError::NotFound(format!("選別か話題 {id}")))
}

/// 素材の印の `edge` を `at` にし、前の値を返す。両方とも無くなった印は外す。
pub(crate) fn set_mark(
    project: &mut Project,
    asset: &AssetId,
    edge: Edge,
    at: Option<RationalTime>,
) -> Result<Option<RationalTime>> {
    project.asset(asset)?;
    let marks = &mut project.bins.marks;
    let index = match marks.iter().position(|m| &m.asset == asset) {
        Some(i) => i,
        None => {
            let at_pos = marks.partition_point(|m| m.asset < *asset);
            marks.insert(
                at_pos,
                SourceMarks {
                    asset: asset.clone(),
                    in_point: None,
                    out_point: None,
                },
            );
            at_pos
        }
    };
    let mark = &mut marks[index];
    let slot = match edge {
        Edge::In => &mut mark.in_point,
        Edge::Out => &mut mark.out_point,
    };
    let old = std::mem::replace(slot, at);
    if mark.in_point.is_none() && mark.out_point.is_none() {
        marks.remove(index);
    }
    Ok(old)
}

/// 単語の直しを `change` で変え、前の直しを返す（無ければ何も変えない直し）。何も変えない直しは外す。
pub(crate) fn update_edit(
    project: &mut Project,
    asset: &AssetId,
    word: &WordId,
    change: impl FnOnce(&mut WordEdit),
) -> Result<WordEdit> {
    project.asset(asset)?;
    let edits = &mut project.transcript_edits;
    let key = (asset, word);
    let found = edits.binary_search_by(|e| (&e.asset, &e.word).cmp(&key));
    let mut edit = match found {
        Ok(i) => edits[i].clone(),
        Err(_) => WordEdit {
            asset: asset.clone(),
            word: word.clone(),
            text: None,
            deleted: false,
        },
    };
    let old = edit.clone();
    change(&mut edit);
    match (found, edit.is_noop()) {
        (Ok(i), true) => {
            edits.remove(i);
        }
        (Ok(i), false) => edits[i] = edit,
        (Err(i), false) => edits.insert(i, edit),
        (Err(_), true) => {}
    }
    Ok(old)
}
