//! `mm shorts`: 本編のカットごとに使っているショートと、複製のショートの元が変わったカットを出す。

use std::path::{Path, PathBuf};

use serde_json::json;

use mm_doc::{CutOverrides, OriginRelation, Project, ShortUse};
use mm_types::Result;

use crate::project::load_project;

/// `mm shorts`。上の段の登録は `main.rs` の 1 行。
#[derive(clap::Subcommand)]
pub enum Cmd {
    /// 本編のカットごとに使っているショート（上書きのフラグつき）と、複製のショートの元が変わったカットを出す。
    Shorts {
        /// プロジェクトの JSON、または `project.mmdb` のあるフォルダー。
        project: PathBuf,
        /// 1 行に 1 件の JSON で出す。
        #[arg(long)]
        json: bool,
    },
}

impl Cmd {
    /// サブコマンドを実行する。
    pub fn run(self) -> Result<()> {
        match self {
            Self::Shorts { project, json } => shorts(&project, json),
        }
    }
}

/// 結果を出す。`json` なら 1 行に 1 件の JSON。
pub fn shorts(project: &Path, json: bool) -> Result<()> {
    let project = load_project(project)?;
    let lines = if json {
        json_lines(&project)?
    } else {
        text_lines(&project)?
    };
    for line in lines {
        println!("{line}");
    }
    Ok(())
}

fn relation_name(r: OriginRelation) -> &'static str {
    match r {
        OriginRelation::Reference => "参照",
        OriginRelation::Fork => "複製",
    }
}

fn override_mark(o: CutOverrides) -> &'static str {
    match (o.in_point, o.out_point) {
        (true, true) => "、in と out を上書き",
        (true, false) => "、in を上書き",
        (false, true) => "、out を上書き",
        (false, false) => "",
    }
}

fn use_text(u: &ShortUse) -> String {
    format!(
        "{}:{}（{}{}）",
        u.short,
        u.cut,
        relation_name(u.relation),
        override_mark(u.overrides)
    )
}

/// 人が読む形。本編ごとに使われたカットの行、続けて元が変わったカットの行。
pub fn text_lines(project: &Project) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for seq in &project.sequences {
        let uses = project.short_uses(&seq.id);
        if uses.is_empty() {
            continue;
        }
        lines.push(format!("本編 {}", seq.id));
        for cut in seq.cuts.iter().filter(|c| uses.contains_key(&c.id)) {
            let list: Vec<String> = uses[&cut.id].iter().map(use_text).collect();
            lines.push(format!("  {}  {}", cut.id, list.join("  ")));
        }
    }
    for seq in project.sequences.iter().filter(|s| s.origin.is_some()) {
        let changes = project.origin_changes(&seq.id)?;
        if changes.is_empty() {
            continue;
        }
        lines.push(format!("ショート {} の元が変わったカット", seq.id));
        for c in changes {
            let what = if c.removed {
                "消えた".to_owned()
            } else {
                c.fields.join(", ")
            };
            lines.push(format!("  {} ← {}: {what}", c.cut, c.origin));
        }
    }
    Ok(lines)
}

/// 1 行に 1 件の JSON。使われたカットは `uses`、元が変わったカットは `change` の種類。
pub fn json_lines(project: &Project) -> Result<Vec<String>> {
    let mut lines = Vec::new();
    for seq in &project.sequences {
        for (cut, uses) in project.short_uses(&seq.id) {
            let uses: Vec<_> = uses
                .iter()
                .map(|u| {
                    json!({
                        "short": u.short.as_str(),
                        "relation": u.relation,
                        "cut": u.cut.as_str(),
                        "overrides": u.overrides,
                    })
                })
                .collect();
            let row =
                json!({"kind": "uses", "main": seq.id.as_str(), "cut": cut.as_str(), "uses": uses});
            lines.push(row.to_string());
        }
    }
    for seq in project.sequences.iter().filter(|s| s.origin.is_some()) {
        for c in project.origin_changes(&seq.id)? {
            let row = json!({
                "kind": "change", "short": seq.id.as_str(), "cut": c.cut.as_str(),
                "origin": c.origin.as_str(), "fields": c.fields, "removed": c.removed,
            });
            lines.push(row.to_string());
        }
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::{Cut, CutBase, Node, Sequence, SequenceOrigin};
    use mm_types::{Id, RationalTime};

    fn cut(id: &str, origin: Option<&str>) -> Cut {
        let mut c = Cut::new(
            id,
            Node::new("solid", 1),
            RationalTime::ZERO,
            RationalTime::seconds(2),
        );
        c.origin = origin.map(Id::new);
        c
    }

    fn project() -> Project {
        let mut p = Project::new();
        p.sequences.push(Sequence {
            id: Id::new("main"),
            cuts: vec![cut("k1", None), cut("k2", None)],
            origin: None,
        });
        let mut r = cut("r1", Some("k2"));
        r.overrides.out_point = true;
        p.sequences.push(Sequence {
            id: Id::new("short"),
            cuts: vec![r],
            origin: Some(SequenceOrigin {
                sequence: Id::new("main"),
                relation: OriginRelation::Reference,
            }),
        });
        let mut f = cut("f1", Some("k1"));
        f.origin_base = Some(CutBase {
            out_point: RationalTime::seconds(3),
            ..CutBase::of(&f)
        });
        p.sequences.push(Sequence {
            id: Id::new("fork"),
            cuts: vec![f],
            origin: Some(SequenceOrigin {
                sequence: Id::new("main"),
                relation: OriginRelation::Fork,
            }),
        });
        p
    }

    #[test]
    fn 本編のカットに使うショートと上書きのフラグと_元が変わったカットを出す() {
        let lines = text_lines(&project()).unwrap();
        assert_eq!(
            lines,
            vec![
                "本編 main",
                "  k1  fork:f1（複製）",
                "  k2  short:r1（参照、out を上書き）",
                "ショート fork の元が変わったカット",
                "  f1 ← k1: out",
            ]
        );
    }

    #[test]
    fn json_は_1_行に_1_件() {
        let lines = json_lines(&project()).unwrap();
        assert_eq!(lines.len(), 3);
        let first: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(first["uses"][0]["overrides"]["out"], true);
        assert_eq!(first["uses"][0]["relation"], "reference");
        let change: serde_json::Value = serde_json::from_str(&lines[2]).unwrap();
        assert_eq!(change["fields"][0], "out");
    }
}
