//! `mm perf compare` と `mm perf budget`。`mm perf run` の結果の JSON を読み、前後の比較と予算の超過を出す。
//!
//! 入力は結果の JSON か、結果の JSON を集めたフォルダー。同じ項目のサンプルは全ファイルで束ね、
//! 束ねた中央値で比べる（runner の揺れを、繰り返しの数で均す）。

use std::collections::BTreeMap;
use std::path::Path;

use mm_types::{MmError, Result};
use serde_json::{json, Value};

use crate::perf::median;

/// 束ねた 1 項目。
#[derive(Debug, Clone)]
pub struct Pooled {
    pub scene: String,
    pub name: String,
    pub unit: String,
    pub higher_is_better: bool,
    pub samples: Vec<f64>,
    /// `(at_most か at_least, 値, 予算の表の行)`。
    pub budget: Option<(String, f64, String)>,
}

impl Pooled {
    /// 束ねたサンプルの中央値。
    pub fn value(&self) -> f64 {
        median(&self.samples)
    }

    /// 予算を超えているか。予算が無ければ `false`。
    pub fn over_budget(&self) -> bool {
        match &self.budget {
            Some((kind, limit, _)) if kind == "at_most" => self.value() > *limit,
            Some((_, limit, _)) => self.value() < *limit,
            None => false,
        }
    }
}

/// 比較の設定。
#[derive(Debug, Clone, Copy)]
pub struct CompareOptions {
    /// 遅くなったとみなす割合（0.3 は 30%）。
    pub threshold: f64,
    /// この絶対値より小さい差は無視する（項目の単位）。
    pub min_delta: f64,
}

/// `30%` か `0.3` を割合にする。
pub fn parse_threshold(s: &str) -> std::result::Result<f64, String> {
    let (num, scale) = match s.strip_suffix('%') {
        Some(n) => (n, 100.0),
        None => (s, 1.0),
    };
    let v: f64 = num.parse().map_err(|e| format!("{s}: {e}"))?;
    if v.is_nan() || v < 0.0 {
        return Err(format!("{s} は 0 以上の割合"));
    }
    Ok(v / scale)
}

/// 結果の JSON か、そのフォルダーを読んで項目ごとに束ねる。
pub fn load_pooled(path: &Path) -> Result<BTreeMap<String, Pooled>> {
    let files = if path.is_dir() {
        let mut v: Vec<_> = std::fs::read_dir(path)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "json"))
            .collect();
        v.sort();
        v
    } else {
        vec![path.to_path_buf()]
    };
    let mut pooled = BTreeMap::new();
    for file in &files {
        let text = std::fs::read_to_string(file)
            .map_err(|e| MmError::NotFound(format!("{}: {e}", file.display())))?;
        let doc: Value = serde_json::from_str(&text)?;
        pool_doc(&doc, &mut pooled);
    }
    if pooled.is_empty() {
        return Err(MmError::NotFound(format!(
            "{} に結果の JSON が無い",
            path.display()
        )));
    }
    Ok(pooled)
}

fn pool_doc(doc: &Value, pooled: &mut BTreeMap<String, Pooled>) {
    let scene = doc["scene"].as_str().unwrap_or("").to_owned();
    let Some(items) = doc["items"].as_array() else {
        return;
    };
    for item in items {
        let Some(name) = item["name"].as_str() else {
            continue;
        };
        let entry = pooled.entry(name.to_owned()).or_insert_with(|| Pooled {
            scene: scene.clone(),
            name: name.to_owned(),
            unit: item["unit"].as_str().unwrap_or("").to_owned(),
            higher_is_better: item["better"].as_str() == Some("higher"),
            samples: Vec::new(),
            budget: budget_of(&item["budget"]),
        });
        let samples = item["samples"].as_array().into_iter().flatten();
        entry.samples.extend(samples.filter_map(Value::as_f64));
    }
}

fn budget_of(v: &Value) -> Option<(String, f64, String)> {
    Some((
        v["kind"].as_str()?.to_owned(),
        v["value"].as_f64()?,
        v["row"].as_str().unwrap_or("").to_owned(),
    ))
}

/// 比べた結果の 1 行。
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub name: String,
    pub unit: String,
    pub before: f64,
    pub after: f64,
    /// 遅くなった割合。負は速くなった。
    pub slower: f64,
    pub regressed: bool,
}

/// 前後で共通の項目を比べる。
pub fn compare(
    before: &BTreeMap<String, Pooled>,
    after: &BTreeMap<String, Pooled>,
    opts: CompareOptions,
) -> Vec<Row> {
    after
        .values()
        .filter_map(|a| {
            let b = before.get(&a.name)?;
            let (bv, av) = (b.value(), a.value());
            let slower = slower_ratio(bv, av, a.higher_is_better);
            let delta = (av - bv).abs();
            Some(Row {
                name: a.name.clone(),
                unit: a.unit.clone(),
                before: bv,
                after: av,
                slower,
                regressed: slower > opts.threshold && delta >= opts.min_delta,
            })
        })
        .collect()
}

/// 遅くなった割合。前が 0 なら比べられないので 0。
fn slower_ratio(before: f64, after: f64, higher_is_better: bool) -> f64 {
    if before <= 0.0 {
        return 0.0;
    }
    if higher_is_better {
        (before - after) / before
    } else {
        (after - before) / before
    }
}

/// `mm perf compare`。遅くなった項目があれば `false`（呼び出し側が終了コードで失敗させる）。
pub fn run_compare(before: &Path, after: &Path, opts: CompareOptions) -> Result<bool> {
    let rows = compare(&load_pooled(before)?, &load_pooled(after)?, opts);
    if rows.is_empty() {
        return Err(MmError::NotFound("前後で共通の項目が無い".into()));
    }
    println!(
        "{:<28} {:>12} {:>12} {:>9} 判定",
        "項目", "前", "後", "遅くなった"
    );
    for r in &rows {
        println!(
            "{:<28} {:>12.3} {:>12.3} {:>+8.1}% {}",
            r.name,
            r.before,
            r.after,
            r.slower * 100.0,
            if r.regressed { "回帰" } else { "OK" }
        );
    }
    let bad: Vec<_> = rows.iter().filter(|r| r.regressed).collect();
    if bad.is_empty() {
        return Ok(true);
    }
    let names: Vec<_> = bad.iter().map(|r| r.name.as_str()).collect();
    eprintln!(
        "回帰: {:.0}% を超えて遅くなった項目: {}",
        opts.threshold * 100.0,
        names.join(", ")
    );
    Ok(false)
}

/// `mm perf budget`。予算を超えた項目を表か JSON で出す。超えていても `Ok`（起票は呼び出し側が決める）。
pub fn run_budget(path: &Path, json_out: bool) -> Result<()> {
    let pooled = load_pooled(path)?;
    let over: Vec<&Pooled> = pooled.values().filter(|p| p.over_budget()).collect();
    if json_out {
        let rows: Vec<Value> = over.iter().map(|p| over_json(p)).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    for p in pooled.values().filter(|p| p.budget.is_some()) {
        let (kind, limit, row) = p.budget.clone().unwrap_or_default();
        let sign = if kind == "at_most" { "<=" } else { ">=" };
        println!(
            "{:<28} {:>12.3} {:<4} 予算 {sign} {limit} {}  [{row}]",
            p.name,
            p.value(),
            p.unit,
            if p.over_budget() { "超過" } else { "OK" }
        );
    }
    Ok(())
}

fn over_json(p: &Pooled) -> Value {
    let (kind, limit, row) = p.budget.clone().unwrap_or_default();
    json!({
        "scene": p.scene,
        "name": p.name,
        "unit": p.unit,
        "value": p.value(),
        "kind": kind,
        "limit": limit,
        "row": row,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pooled(name: &str, higher: bool, samples: &[f64]) -> (String, Pooled) {
        (
            name.to_owned(),
            Pooled {
                scene: "cpu".into(),
                name: name.into(),
                unit: "ms".into(),
                higher_is_better: higher,
                samples: samples.to_vec(),
                budget: None,
            },
        )
    }

    fn opts(threshold: f64, min_delta: f64) -> CompareOptions {
        CompareOptions {
            threshold,
            min_delta,
        }
    }

    #[test]
    fn 閾値を超えて遅くなると回帰になる() {
        let before = BTreeMap::from([pooled("a", false, &[10.0, 10.0, 10.0])]);
        let after = BTreeMap::from([pooled("a", false, &[14.0, 14.0, 14.0])]);
        assert!(compare(&before, &after, opts(0.3, 0.0))[0].regressed);
        assert!(!compare(&before, &after, opts(0.5, 0.0))[0].regressed);
    }

    #[test]
    fn 大きいほど良い項目は下がると回帰になる() {
        let before = BTreeMap::from([pooled("fps", true, &[100.0])]);
        let after = BTreeMap::from([pooled("fps", true, &[60.0])]);
        let row = &compare(&before, &after, opts(0.3, 0.0))[0];
        assert!(row.regressed && (row.slower - 0.4).abs() < 1e-9);
    }

    #[test]
    fn 速くなっても絶対値が小さい差でも回帰にしない() {
        let before = BTreeMap::from([pooled("a", false, &[0.02]), pooled("b", false, &[10.0])]);
        let after = BTreeMap::from([pooled("a", false, &[0.04]), pooled("b", false, &[5.0])]);
        let rows = compare(&before, &after, opts(0.3, 0.05));
        assert!(rows.iter().all(|r| !r.regressed), "{rows:?}");
    }

    #[test]
    fn 束ねた中央値で比べて外れ値に引きずられない() {
        let before = BTreeMap::from([pooled("a", false, &[10.0, 10.0, 10.0, 10.0, 10.0])]);
        let after = BTreeMap::from([pooled("a", false, &[10.0, 10.0, 10.0, 10.0, 80.0])]);
        assert!(!compare(&before, &after, opts(0.3, 0.0))[0].regressed);
    }

    #[test]
    fn 予算の向きで超過が決まる() {
        let (_, mut p) = pooled("a", false, &[30.0]);
        p.budget = Some(("at_most".into(), 20.0, "行".into()));
        assert!(p.over_budget());
        p.budget = Some(("at_least".into(), 20.0, "行".into()));
        assert!(!p.over_budget());
    }

    #[test]
    fn 割合は_percent_でも小数でも読める() {
        assert_eq!(parse_threshold("30%").unwrap(), 0.3);
        assert_eq!(parse_threshold("0.3").unwrap(), 0.3);
        assert!(parse_threshold("-1").is_err());
    }
}
