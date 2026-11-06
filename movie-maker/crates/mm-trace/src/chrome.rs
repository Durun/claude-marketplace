//! 詳細の `Layer`。全部のスパンを Chrome trace event（`ph: "X"`、`ts` と `dur` はマイクロ秒）に残す。
//! Perfetto UI で開け、Trace Processor の SQL で読める。

use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tracing::span::{Attributes, Id};
use tracing::{Event, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::registry::LookupSpan;

use crate::json;
use crate::metrics::MetricVisitor;

/// 1 つの complete event。
#[derive(Debug, Clone, PartialEq)]
pub struct ChromeEvent {
    pub name: String,
    pub target: String,
    pub ts_us: u64,
    pub dur_us: u64,
    pub tid: u64,
}

struct Started(Instant);

#[derive(Default)]
struct Inner {
    events: Vec<ChromeEvent>,
}

/// 集めたイベントを読むハンドル。
#[derive(Clone)]
pub struct ChromeTraceHandle {
    origin: Instant,
    inner: Arc<Mutex<Inner>>,
}

impl ChromeTraceHandle {
    /// 今までのイベント。
    pub fn events(&self) -> Vec<ChromeEvent> {
        self.inner.lock().expect("chrome lock").events.clone()
    }

    /// Chrome trace の JSON（`{"traceEvents":[...]}`）を書く。
    pub fn write_to(&self, w: &mut dyn Write) -> std::io::Result<()> {
        let mut out = String::from("{\"traceEvents\":[");
        for (i, e) in self.events().iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            json::key(&mut out, "name");
            json::string(&mut out, &e.name);
            out.push(',');
            json::key(&mut out, "cat");
            json::string(&mut out, &e.target);
            out.push_str(&format!(
                ",\"ph\":\"X\",\"ts\":{},\"dur\":{},\"pid\":1,\"tid\":{}}}",
                e.ts_us, e.dur_us, e.tid
            ));
        }
        out.push_str("]}");
        w.write_all(out.as_bytes())
    }

    fn push(&self, e: ChromeEvent) {
        self.inner.lock().expect("chrome lock").events.push(e);
    }
}

/// 全部のスパンを complete event として残す `Layer`。
pub struct ChromeTraceLayer {
    handle: ChromeTraceHandle,
}

impl ChromeTraceLayer {
    /// `Layer` とハンドルの組。`ts` は作られた時刻を 0 とする。
    pub fn new() -> (Self, ChromeTraceHandle) {
        let handle = ChromeTraceHandle {
            origin: Instant::now(),
            inner: Arc::default(),
        };
        (
            Self {
                handle: handle.clone(),
            },
            handle,
        )
    }
}

fn thread_id() -> u64 {
    // ThreadId の内部は非公開なので、Debug の表現から数字を取り出す。
    let s = format!("{:?}", std::thread::current().id());
    s.chars()
        .filter(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0)
}

impl<S> Layer<S> for ChromeTraceLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, _attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(Started(Instant::now()));
        }
    }

    // `record_span` のイベントは終わった時点で届くので、今から長さを引いた所を開始とする。
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if event.metadata().target() != crate::METRICS_TARGET {
            return;
        }
        let mut v = MetricVisitor::default();
        event.record(&mut v);
        if let (Some(kind), Some(name), Some(value)) = (v.kind, v.name, v.value) {
            if kind != "span" {
                return;
            }
            let dur_us = value as u64;
            let now_us = self.handle.origin.elapsed().as_micros() as u64;
            self.handle.push(ChromeEvent {
                name,
                target: crate::METRICS_TARGET.to_owned(),
                ts_us: now_us.saturating_sub(dur_us),
                dur_us,
                tid: thread_id(),
            });
        }
    }

    fn on_close(&self, id: Id, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(&id) else {
            return;
        };
        let started = span.extensions().get::<Started>().map(|s| s.0);
        let Some(started) = started else {
            return;
        };
        let ts_us = started
            .saturating_duration_since(self.handle.origin)
            .as_micros() as u64;
        self.handle.push(ChromeEvent {
            name: span.name().to_owned(),
            target: span.metadata().target().to_owned(),
            ts_us,
            dur_us: started.elapsed().as_micros() as u64,
            tid: thread_id(),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn 外で測ったスパンが_x_イベントになる() {
        let (layer, handle) = ChromeTraceLayer::new();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            crate::record_span("render.gpu_frame", std::time::Duration::from_micros(1500));
            crate::gauge!("memory.rss_bytes", 1);
        });
        let events = handle.events();
        assert_eq!(events.len(), 1);
        assert_eq!(
            (events[0].name.as_str(), events[0].dur_us),
            ("render.gpu_frame", 1500)
        );
    }

    #[test]
    fn 入れ子のスパンが_x_イベントになる() {
        let (layer, handle) = ChromeTraceLayer::new();
        let subscriber = tracing_subscriber::registry().with(layer);
        tracing::subscriber::with_default(subscriber, || {
            let outer = tracing::info_span!("render.frame");
            let _o = outer.enter();
            let inner = tracing::debug_span!("render.layer");
            drop(inner);
        });
        let events = handle.events();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].name, "render.layer");
        assert_eq!(events[1].name, "render.frame");
        let mut buf = Vec::new();
        handle.write_to(&mut buf).unwrap();
        let s = String::from_utf8(buf).unwrap();
        assert!(s.starts_with("{\"traceEvents\":[") && s.contains("\"ph\":\"X\""));
    }
}
