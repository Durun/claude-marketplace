//! 取り込みが進み具合のスパンを出すか。購読者はプロセスで 1 つ置く（スレッドごとの購読者だと、
//! 別のテストの呼び出し地点の関心のキャッシュに左右される）ので、このテストだけで別の実行ファイルにする（docs/ci.md）。

#[path = "it/common/mod.rs"]
mod common;

use common::{options, sample, tools};
use mm_ingest::ingest;
use mm_trace::MetricsLayer;
use std::sync::{Arc, Mutex};
use tracing_subscriber::layer::Context;
use tracing_subscriber::prelude::*;

/// 開いたスパンの属性を文字列にして集める。
struct FieldRecorder(Arc<Mutex<Vec<String>>>);

impl<S: tracing::Subscriber> tracing_subscriber::Layer<S> for FieldRecorder {
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        _: &tracing::span::Id,
        _: Context<'_, S>,
    ) {
        struct Visitor<'a>(&'a mut Vec<String>);
        impl tracing::field::Visit for Visitor<'_> {
            fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
                self.0.push(format!("{}={v:?}", f.name()));
            }
        }
        let mut out = self.0.lock().unwrap();
        attrs.record(&mut Visitor(&mut out));
    }
}

#[test]
fn 取り込みの各段階がスパンで出る() {
    let Some(tools) = tools() else { return };
    let (layer, metrics) = MetricsLayer::new();
    let fields = Arc::new(Mutex::new(Vec::new()));
    tracing::subscriber::set_global_default(
        tracing_subscriber::registry()
            .with(layer)
            .with(FieldRecorder(fields.clone())),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let input = sample(dir.path(), &tools);
    let opts = options(tools, dir.path());
    ingest(&opts, &input).unwrap();
    ingest(&opts, &input).unwrap();

    let spans = metrics.snapshot().spans;
    for name in [
        "ingest.file",
        "ingest.hash",
        "ingest.probe",
        "ingest.preview",
        "ingest.thumbnail",
        "ingest.waveform",
    ] {
        assert!(spans.contains_key(name), "スパン {name} が出ていない");
    }
    assert_eq!(spans["ingest.file"].count, 2);
    // 素材の絶対パスはスパンに載せない（17-instrumentation.md）。
    let recorded = fields.lock().unwrap().join("\n");
    assert!(
        !recorded.contains("sample.mp4"),
        "パスがスパンに出ている: {recorded}"
    );
}
