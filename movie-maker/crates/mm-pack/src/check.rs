//! パックの形式の検査。読めたパックについて、名前、色、文字のスタイル、ミキサーの設定、マクロの引数の参照を確かめ、
//! 各マクロを例の引数で展開して、テンプレートを適用したプロジェクトに適用してみる。

use std::collections::BTreeSet;

use mm_doc::{Cut, Node, Project, Sequence};
use mm_ops::{AddSequence, Editor, OpContext, Transaction};
use mm_types::{Id, RationalTime};

use crate::args::{referenced_names, Env};
use crate::expand::expand;
use crate::layout::EstimateShaper;
use crate::macros::{MacroDef, MacroKind, Step, WebStep};
use crate::pack::{Pack, FORMAT};
use crate::style::TextStyleSpec;

/// 検査の結果。`errors` が空なら通った。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Report {
    pub errors: Vec<String>,
    /// 例で展開して適用できたマクロとテンプレートの数。
    pub applied: usize,
}

impl Report {
    /// 通ったか。
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }

    fn push(&mut self, message: String) {
        self.errors.push(message);
    }
}

/// パックを検査する。
pub fn check(pack: &Pack) -> Report {
    let mut report = Report::default();
    check_manifest(pack, &mut report);
    check_theme(pack, &mut report);
    for (name, style) in &pack.styles {
        check_style(pack, name, style, &mut report);
    }
    check_mixer(pack, &mut report);
    check_names(pack, &mut report);
    for def in &pack.macros {
        check_references(pack, def, &mut report);
    }
    if report.ok() {
        apply_examples(pack, &mut report);
    }
    report
}

fn check_manifest(pack: &Pack, report: &mut Report) {
    let m = &pack.manifest;
    if m.format != FORMAT {
        report.push(format!(
            "pack.json: format は {FORMAT}（{} は読めない）",
            m.format
        ));
    }
    let valid = |c: char| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-';
    if m.name.is_empty() || !m.name.chars().all(valid) {
        report.push(format!(
            "pack.json: name は英小文字、数字、- だけ: {}",
            m.name
        ));
    }
}

fn check_theme(pack: &Pack, report: &mut Report) {
    let t = &pack.theme;
    for (name, hex) in &t.colors {
        if let Err(e) = crate::theme::parse_hex(hex) {
            report.push(format!("theme.json: 色 {name}: {e}"));
        }
    }
    if t.motion.in_max <= 0.0 || t.motion.out_max <= 0.0 {
        report.push("theme.json: motion の上限は正にする".into());
    }
    for c in std::iter::once(&t.canvas).chain(t.short.iter()) {
        if c.width == 0 || c.height == 0 || c.fps == 0 {
            report.push(format!("theme.json: 画面の大きさと fps は正にする: {c:?}"));
        }
    }
}

fn check_style(pack: &Pack, name: &str, s: &TextStyleSpec, report: &mut Report) {
    let at = |what: &str| format!("styles.json の {name}: {what}");
    let mut colors = vec![s.color.as_str()];
    if let Some(p) = &s.plate {
        colors.push(&p.color);
        colors.extend(p.border.as_ref().map(|b| b.color.as_str()));
        colors.extend(p.shadow.as_ref().map(|s| s.color.as_str()));
        if !(0.0..=1.0).contains(&p.opacity) {
            report.push(at("plate.opacity は 0..1"));
        }
    } else if s.tilt != 0.0 {
        report.push(at("tilt は plate のあるスタイルだけ"));
    }
    for c in colors {
        if let Err(e) = pack.theme.color(c) {
            report.push(at(&e.to_string()));
        }
    }
    if s.size <= 0.0 {
        report.push(at("size は正にする"));
    }
    if s.anchor.iter().any(|a| !(0.0..=1.0).contains(a)) {
        report.push(at("anchor は 0..1"));
    }
    let motion = pack.theme.motion;
    if s.fade_in < 0.0 || s.fade_in > motion.in_max {
        report.push(at(&format!(
            "fade_in は 0..{}（theme の motion.in_max）",
            motion.in_max
        )));
    }
    if s.fade_out < 0.0 || s.fade_out > motion.out_max {
        report.push(at(&format!(
            "fade_out は 0..{}（theme の motion.out_max）",
            motion.out_max
        )));
    }
}

fn check_mixer(pack: &Pack, report: &mut Report) {
    let Some(mixer) = &pack.mixer else { return };
    if let Some(d) = &mixer.ducking {
        if d.reduction_db >= 0.0 {
            report.push("mixer.json: ducking.reduction_db は負にする".into());
        }
        if d.targets.contains(&d.trigger) {
            report.push("mixer.json: ducking の trigger を targets に入れない".into());
        }
        if d.threshold_db > 0.0 {
            report.push("mixer.json: ducking.threshold_db は dBFS（0 以下）".into());
        }
        if d.attack_ms < 0.0 || d.release_ms < 0.0 {
            report.push("mixer.json: ducking の attack_ms と release_ms は 0 以上".into());
        }
    }
}

fn check_names(pack: &Pack, report: &mut Report) {
    let mut seen = BTreeSet::new();
    for def in &pack.macros {
        if !seen.insert(def.name.as_str()) {
            report.push(format!(
                "マクロの名前 {} が templates/ と macros/ で重なる",
                def.name
            ));
        }
    }
}

/// 手順が引く名前が全部あり、引数が全部使われ、例の引数が引数にあり、文字のスタイルがあるか。
fn check_references(pack: &Pack, def: &MacroDef, report: &mut Report) {
    let at = |what: String| format!("{}: {what}", def.name);
    let known: BTreeSet<String> = Env::names(def, &pack.theme).into_iter().collect();
    let mut used = BTreeSet::new();
    for step in &def.steps {
        match step {
            Step::Text(t) if !pack.styles.contains_key(&t.style) => {
                report.push(at(format!(
                    "styles.json に文字のスタイル {} が無い",
                    t.style
                )));
            }
            Step::Mixer(_) if pack.mixer.is_none() => {
                report.push(at("mixer 手順があるのに mixer.json が無い".into()));
            }
            Step::Web(w) => check_web(pack, w)
                .into_iter()
                .for_each(|e| report.push(at(e))),
            Step::Transition(t)
                if !t.name.contains("{{") && !pack.transitions.code.contains_key(&t.name) =>
            {
                report.push(at(format!("transitions/{}.wgsl が無い", t.name)));
            }
            _ => {}
        }
        for s in step.strings() {
            match referenced_names(s) {
                Ok(names) => used.extend(names),
                Err(e) => report.push(at(e.to_string())),
            }
        }
    }
    for name in used.difference(&known) {
        report.push(at(format!("{{{{{name}}}}} は引数にも theme にも無い")));
    }
    for name in def.params.keys().filter(|n| !used.contains(*n)) {
        report.push(at(format!("引数 {name} をどの手順も使わない")));
    }
    for name in def.example.keys().filter(|n| !def.params.contains_key(*n)) {
        report.push(at(format!("example の {name} は引数に無い")));
    }
}

/// `web` 手順のページがあり、動きの長さが テーマの上限の中か。
fn check_web(pack: &Pack, w: &WebStep) -> Vec<String> {
    let mut errors = Vec::new();
    if !w.page.contains("{{") && !pack.pages.contains_key(&w.page) {
        errors.push(format!("web/{}.html が無い", w.page));
    }
    let motion = pack.theme.motion;
    if !(0.0..=motion.in_max).contains(&w.fade_in) {
        errors.push(format!(
            "web の fade_in は 0..{}（テーマの motion.in_max）",
            motion.in_max
        ));
    }
    if !(0.0..=motion.out_max).contains(&w.fade_out) {
        errors.push(format!(
            "web の fade_out は 0..{}（テーマの motion.out_max）",
            motion.out_max
        ));
    }
    errors
}

/// カットに掛けるマクロ（`transition`）の例が引く、2 つのカット `c1`、`c2` の sequence `check`。
fn sample_sequence() -> Transaction {
    let cut = |id: &str| {
        Cut::new(
            id,
            Node::new("solid", 1),
            RationalTime::ZERO,
            RationalTime::seconds(2),
        )
    };
    let op = AddSequence {
        sequence: Sequence {
            id: Id::new("check"),
            cuts: vec![cut("c1"), cut("c2")],
            origin: None,
        },
    };
    Transaction::new("検査の sequence", vec![op.into()])
}

/// テンプレートを例の引数で空のプロジェクトに適用し、2 つのカットの sequence を足して、その上に各マクロを例の引数で
/// 適用してみる。
fn apply_examples(pack: &Pack, report: &mut Report) {
    let ctx = OpContext::human();
    let mut base = Editor::new(Project::new());
    for def in pack.templates() {
        match expand(pack, &def.name, &def.example, &EstimateShaper) {
            Ok(txn) => match base.apply(txn, &ctx) {
                Ok(_) => report.applied += 1,
                Err(e) => report.push(format!("{}: 例を適用できない: {e}", def.name)),
            },
            Err(e) => report.push(format!("{}: 例を展開できない: {e}", def.name)),
        }
    }
    if let Err(e) = base.apply(sample_sequence(), &ctx) {
        report.push(format!("検査の sequence を適用できない: {e}"));
    }
    for def in pack.macros.iter().filter(|m| m.kind == MacroKind::Macro) {
        let mut editor = Editor::new(base.project().clone());
        let result = expand(pack, &def.name, &def.example, &EstimateShaper)
            .map_err(|e| format!("例を展開できない: {e}"))
            .and_then(|txn| {
                editor
                    .apply(txn, &ctx)
                    .map(|_| ())
                    .map_err(|e| format!("例を適用できない: {e}"))
            });
        match result {
            Ok(()) => report.applied += 1,
            Err(e) => report.push(format!("{}: {e}", def.name)),
        }
    }
}
