//! 参照のショートのカットと元のカットの結び付き。`touches` に足す所と、上書きの操作が引く元のカット。

use mm_doc::{Cut, CutId, OriginRelation, Project, Sequence};
use mm_types::Result;

use crate::ops::{cut_referrers, invalid};
use crate::touches::Touches;

/// 参照のショート（元に従う sequence）か。
fn is_reference(seq: &Sequence) -> bool {
    seq.origin
        .as_ref()
        .is_some_and(|o| o.relation == OriginRelation::Reference && o.sequence != seq.id)
}

/// `t` に入ったカットと結ばれた参照のカット・元のカットを、増えなくなるまで足す（ショートのショートもたどる）。
/// 参照のカットは元に合わせて長さが変わりうるので、リップルと同じく後ろのカットに接続したレイヤーも足す。
pub(crate) fn add_short_links(project: &Project, t: &mut Touches) {
    let refs: Vec<&Sequence> = project
        .sequences
        .iter()
        .filter(|s| is_reference(s))
        .collect();
    let links: Vec<(&str, &str)> = refs
        .iter()
        .flat_map(|s| s.cuts.iter())
        .filter_map(|c| Some((c.id.as_str(), c.origin.as_ref()?.as_str())))
        .collect();
    loop {
        let mut grown = false;
        for &(cut, origin) in &links {
            let (has_cut, has_origin) = (t.contains(cut), t.contains(origin));
            if has_cut != has_origin {
                t.add_str(if has_cut { origin } else { cut });
                grown = true;
            }
        }
        if !grown {
            break;
        }
    }
    for seq in refs {
        let Some(first) = seq.cuts.iter().position(|c| t.contains(c.id.as_str())) else {
            continue;
        };
        for cut in &seq.cuts[first..] {
            cut_referrers(project, &cut.id, t);
        }
    }
}

/// 参照のショートのカット `id` と、その元のカット。参照のショートのカットでなければ `InvalidArgument`。
pub(crate) fn reference_pair<'a>(project: &'a Project, id: &CutId) -> Result<(&'a Cut, &'a Cut)> {
    let (seq, cut) = project.cut(id)?;
    let not_reference = || {
        invalid(format!(
            "カット {id} は参照のショートの、元のあるカットでない"
        ))
    };
    if !is_reference(seq) {
        return Err(not_reference());
    }
    let origin = seq.origin.as_ref().ok_or_else(not_reference)?;
    let main_id = cut.origin.as_ref().ok_or_else(not_reference)?;
    let main = project
        .sequence(&origin.sequence)?
        .cuts
        .iter()
        .find(|c| &c.id == main_id)
        .ok_or_else(not_reference)?;
    Ok((cut, main))
}
