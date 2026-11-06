//! 取り込みが進み具合のスパンを出すか。購読者はプロセスで 1 つ置く（スレッドごとの購読者だと、
//! 別のテストの呼び出し地点の関心のキャッシュに左右される）ので、このテストだけで 1 つの実行ファイルにする。

mod common;

use common::{options, sample, tools};
use mm_ingest::ingest;
use mm_trace::MetricsLayer;
use tracing_subscriber::prelude::*;

#[test]
fn 取り込みの各段階がスパンで出る() {
    let Some(tools) = tools() else { return };
    let (layer, metrics) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer)).unwrap();
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
}
