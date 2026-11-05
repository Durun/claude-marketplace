//! 常時の層。`INFO` のスパンの名前ごとの回数と分布、カウンタ、ゲージを集計し、
//! `metrics.jsonl` の 1 行にする。

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::json;

/// 分布を出す百分位。
const PERCENTILES: [(&str, f64); 3] = [("p50", 0.50), ("p95", 0.95), ("p99", 0.99)];

/// スパンに付ける開始時刻。
struct Started(Instant);

/// 1 つのスパンの名前の分布（マイクロ秒）。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SpanStats {
    pub count: u64,
    pub p50_us: u64,
    pub p95_us: u64,
    pub p99_us: u64,
    pub max_us: u64,
}

/// ある時点の集計。`to_jsonl_line` で 1 行にする。
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MetricsSnapshot {
    pub spans: BTreeMap<String, SpanStats>,
    pub counters: BTreeMap<String, f64>,
    pub gauges: BTreeMap<String, f64>,
}

impl MetricsSnapshot {
    /// `metrics.jsonl` の 1 行（改行なし）。
    pub fn to_jsonl_line(&self) -> String {
        let mut out = String::from("{");
        json::key(&mut out, "spans");
        out.push('{');
        for (i, (name, s)) in self.spans.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            json::key(&mut out, name);
            out.push_str(&format!(
                "{{\"count\":{},\"p50_us\":{},\"p95_us\":{},\"p99_us\":{},\"max_us\":{}}}",
                s.count, s.p50_us, s.p95_us, s.p99_us, s.max_us
            ));
        }
        out.push_str("},");
        write_map(&mut out, "counters", &self.counters);
        out.push(',');
        write_map(&mut out, "gauges", &self.gauges);
        out.push('}');
        out
    }
}

fn write_map(out: &mut String, name: &str, map: &BTreeMap<String, f64>) {
    json::key(out, name);
    out.push('{');
    for (i, (k, v)) in map.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        json::key(out, k);
        json::number(out, *v);
    }
    out.push('}');
}

#[derive(Default)]
struct Inner {
    durations_us: BTreeMap<String, Vec<u64>>,
    counters: BTreeMap<String, f64>,
    gauges: BTreeMap<String, f64>,
}

/// 集計を読む側の取っ手。層と共有する。
#[derive(Clone, Default)]
pub struct MetricsHandle {
    inner: Arc<Mutex<Inner>>,
}

impl MetricsHandle {
    /// 今の集計。
    pub fn snapshot(&self) -> MetricsSnapshot {
        let inner = self.inner.lock().expect("metrics lock");
        let spans = inner
            .durations_us
            .iter()
            .map(|(name, d)| (name.clone(), stats_of(d)))
            .collect();
        MetricsSnapshot {
            spans,
            counters: inner.counters.clone(),
            gauges: inner.gauges.clone(),
        }
    }

    /// 集計を取り出して空にする（1 分ごとの 1 行に使う）。
    pub fn drain(&self) -> MetricsSnapshot {
        let snap = self.snapshot();
        let mut inner = self.inner.lock().expect("metrics lock");
        inner.durations_us.clear();
        inner.counters.clear();
        snap
    }

    fn record_span(&self, name: &str, us: u64) {
        let mut inner = self.inner.lock().expect("metrics lock");
        inner
            .durations_us
            .entry(name.to_owned())
            .or_default()
            .push(us);
    }

    fn record_metric(&self, kind: &str, name: &str, value: f64) {
        let mut inner = self.inner.lock().expect("metrics lock");
        match kind {
            "counter" => *inner.counters.entry(name.to_owned()).or_insert(0.0) += value,
            "gauge" => {
                inner.gauges.insert(name.to_owned(), value);
            }
            _ => {}
        }
    }
}

fn stats_of(durations: &[u64]) -> SpanStats {
    let mut sorted = durations.to_vec();
    sorted.sort_unstable();
    let pick = |q: f64| -> u64 {
        if sorted.is_empty() {
            return 0;
        }
        let idx = ((sorted.len() - 1) as f64 * q).round() as usize;
        sorted[idx]
    };
    let mut stats = SpanStats {
        count: sorted.len() as u64,
        max_us: sorted.last().copied().unwrap_or(0),
        ..SpanStats::default()
    };
    for (label, q) in PERCENTILES {
        let v = pick(q);
        match label {
            "p50" => stats.p50_us = v,
            "p95" => stats.p95_us = v,
            _ => stats.p99_us = v,
        }
    }
    stats
}

/// 常時の層。`INFO` 以上のスパンの長さと、`mm.metrics` のイベントを集計する。
pub struct MetricsLayer {
    handle: MetricsHandle,
}

impl MetricsLayer {
    /// 層と、集計を読む取っ手の組。
    pub fn new() -> (Self, MetricsHandle) {
        let handle = MetricsHandle::default();
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

#[derive(Default)]
pub(crate) struct MetricVisitor {
    pub(crate) kind: Option<String>,
    pub(crate) name: Option<String>,
    pub(crate) value: Option<f64>,
}

impl Visit for MetricVisitor {
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}

    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "metric" => self.kind = Some(value.to_owned()),
            "name" => self.name = Some(value.to_owned()),
            _ => {}
        }
    }

    fn record_f64(&mut self, field: &Field, value: f64) {
        if field.name() == "value" {
            self.value = Some(value);
        }
    }

    fn record_i64(&mut self, field: &Field, value: i64) {
        self.record_f64(field, value as f64);
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        self.record_f64(field, value as f64);
    }
}

impl<S> Layer<S> for MetricsLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, _attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            if *span.metadata().level() <= tracing::Level::INFO {
                span.extensions_mut().insert(Started(Instant::now()));
            }
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let started = span.extensions().get::<Started>().map(|s| s.0);
        if let Some(started) = started {
            let us = started.elapsed().as_micros() as u64;
            self.handle.record_span(span.name(), us);
        }
    }

    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != crate::METRICS_TARGET {
            return;
        }
        let mut v = MetricVisitor::default();
        event.record(&mut v);
        if let (Some(kind), Some(name), Some(value)) = (v.kind, v.name, v.value) {
            if kind == "span" {
                self.handle.record_span(&name, value as u64);
            } else {
                self.handle.record_metric(&kind, &name, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn スパンとカウンタが集計に出る() {
        let (layer, handle) = MetricsLayer::new();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..3 {
                let span = tracing::info_span!("render.frame");
                let _g = span.enter();
                crate::counter!("cache.hit", 1);
            }
            crate::gauge!("memory.rss_bytes", 42);
            let dbg = tracing::debug_span!("render.layer");
            drop(dbg);
        });
        let snap = handle.snapshot();
        assert_eq!(snap.spans["render.frame"].count, 3);
        assert!(!snap.spans.contains_key("render.layer"));
        assert_eq!(snap.counters["cache.hit"], 3.0);
        assert_eq!(snap.gauges["memory.rss_bytes"], 42.0);
        let line = snap.to_jsonl_line();
        assert!(line.starts_with('{') && line.ends_with('}') && !line.contains('\n'));
    }

    #[test]
    fn 外で測ったスパンが分布に載る() {
        let (layer, handle) = MetricsLayer::new();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            for _ in 0..2 {
                crate::record_span("render.gpu_frame", std::time::Duration::from_micros(1500));
            }
        });
        let s = &handle.snapshot().spans["render.gpu_frame"];
        assert_eq!((s.count, s.p50_us), (2, 1500));
    }

    #[test]
    fn 百分位は並べた位置で取る() {
        let s = stats_of(&[5, 1, 3, 2, 4]);
        assert_eq!((s.count, s.p50_us, s.max_us), (5, 3, 5));
    }
}
