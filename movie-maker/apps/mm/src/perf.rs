//! `mm perf run <場面>`。場面を回して、予算と並べた結果を表と JSON に出す。
//!
//! 結果は `mm-trace` の `metrics.jsonl` の形（1 行 1 集計）と、予算つきの結果の JSON の 2 つ。

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use clap::ValueEnum;
use mm_trace::{MetricsHandle, MetricsSnapshot};
use mm_types::{MmError, Result};
use serde_json::{json, Value};

use crate::scenes;

/// 回せる場面。`playback` と `scrub` は段階 2 で足す。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Scene {
    Startup,
    Export,
    All,
}

/// 予算の向き。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Limit {
    /// この値以下なら合格。
    AtMost(f64),
    /// この値以上なら合格。
    AtLeast(f64),
}

/// 予算。目標の値と、予算の表（docs/design/17-instrumentation.md）での項目名。
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub limit: Limit,
    pub row: &'static str,
}

/// 1 つの測定項目。`value` は繰り返しの中央値。
#[derive(Debug, Clone)]
pub struct Item {
    pub name: &'static str,
    pub unit: &'static str,
    pub value: f64,
    pub samples: Vec<f64>,
    pub budget: Option<Budget>,
}

impl Item {
    /// サンプルの中央値を値にした項目。
    pub fn from_samples(
        name: &'static str,
        unit: &'static str,
        samples: Vec<f64>,
        budget: Option<Budget>,
    ) -> Self {
        Self {
            name,
            unit,
            value: median(&samples),
            samples,
            budget,
        }
    }

    /// 予算に収まっているか。予算が無ければ `None`。
    pub fn within_budget(&self) -> Option<bool> {
        self.budget.map(|b| match b.limit {
            Limit::AtMost(max) => self.value <= max,
            Limit::AtLeast(min) => self.value >= min,
        })
    }
}

/// 中央値。空なら 0。
pub fn median(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut v = samples.to_vec();
    v.sort_by(f64::total_cmp);
    let mid = v.len() / 2;
    if v.len() % 2 == 1 {
        v[mid]
    } else {
        (v[mid - 1] + v[mid]) / 2.0
    }
}

/// `mm perf run` の引数。
#[derive(Debug, Clone)]
pub struct PerfArgs {
    pub scene: Scene,
    pub repeat: usize,
    pub out_dir: PathBuf,
    pub project: PathBuf,
    /// `export` の描画の大きさ（予算は 1080p の値のため既定は 1920x1080）。
    pub size: (u32, u32),
}

/// 場面を回して結果を書く。
pub fn run(args: &PerfArgs, metrics: &MetricsHandle) -> Result<()> {
    if args.repeat == 0 {
        return Err(MmError::InvalidArgument("--repeat は 1 以上".into()));
    }
    let mut items = Vec::new();
    if matches!(args.scene, Scene::Startup | Scene::All) {
        items.extend(scenes::startup(args)?);
    }
    if matches!(args.scene, Scene::Export | Scene::All) {
        items.extend(scenes::export(args)?);
    }
    let snapshot = metrics.snapshot();
    print_table(&items);
    let path = write_results(args, &items, &snapshot)?;
    println!("結果: {}", path.display());
    Ok(())
}

fn print_table(items: &[Item]) {
    println!(
        "{:<28} {:>12} {:<6} {:<16} 判定",
        "項目", "値(中央値)", "単位", "予算"
    );
    for item in items {
        let (budget, verdict) = match (item.budget, item.within_budget()) {
            (Some(b), Some(ok)) => (
                match b.limit {
                    Limit::AtMost(v) => format!("<= {v}"),
                    Limit::AtLeast(v) => format!(">= {v}"),
                },
                if ok { "OK" } else { "超過" },
            ),
            _ => ("-".into(), "-"),
        };
        println!(
            "{:<28} {:>12.2} {:<6} {:<16} {}",
            item.name, item.value, item.unit, budget, verdict
        );
    }
}

fn item_json(item: &Item) -> Value {
    let budget = item.budget.map(|b| {
        let (kind, value) = match b.limit {
            Limit::AtMost(v) => ("at_most", v),
            Limit::AtLeast(v) => ("at_least", v),
        };
        json!({ "kind": kind, "value": value, "row": b.row })
    });
    json!({
        "name": item.name,
        "unit": item.unit,
        "value": item.value,
        "samples": item.samples,
        "budget": budget,
        "ok": item.within_budget(),
    })
}

/// 結果の JSON と `metrics.jsonl` を書き、JSON のパスを返す。
fn write_results(args: &PerfArgs, items: &[Item], snapshot: &MetricsSnapshot) -> Result<PathBuf> {
    std::fs::create_dir_all(&args.out_dir)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let adapter = mm_render::GpuContext::shared()
        .ok()
        .map(|c| c.adapter_name.clone());
    let scene = format!("{:?}", args.scene).to_lowercase();
    let metrics: Value = serde_json::from_str(&snapshot.to_jsonl_line())?;
    let doc = json!({
        "scene": scene,
        "timestamp": stamp,
        "repeat": args.repeat,
        "adapter": adapter,
        "items": items.iter().map(item_json).collect::<Vec<_>>(),
        "metrics": metrics,
    });
    let path = args.out_dir.join(format!("{scene}-{stamp}.json"));
    std::fs::write(&path, serde_json::to_string_pretty(&doc)?)?;
    append_line(
        &args.out_dir.join("metrics.jsonl"),
        &snapshot.to_jsonl_line(),
    )?;
    Ok(path)
}

fn append_line(path: &Path, line: &str) -> Result<()> {
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 中央値() {
        assert_eq!(median(&[3.0, 1.0, 2.0]), 2.0);
        assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), 2.5);
        assert_eq!(median(&[]), 0.0);
    }

    #[test]
    fn 予算の向きで合否が決まる() {
        let budget = |limit| Some(Budget { limit, row: "x" });
        let at_most = Item::from_samples("a", "ms", vec![10.0], budget(Limit::AtMost(20.0)));
        let at_least = Item::from_samples("b", "fps", vec![10.0], budget(Limit::AtLeast(20.0)));
        let none = Item::from_samples("c", "ms", vec![10.0], None);
        assert_eq!(at_most.within_budget(), Some(true));
        assert_eq!(at_least.within_budget(), Some(false));
        assert_eq!(none.within_budget(), None);
    }
}
