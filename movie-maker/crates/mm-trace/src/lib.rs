//! 計装の基礎。`tracing` のスパンを全クレートの決まりにし、出力先を `Layer` で差し替える。
//!
//! - スパンの名前は `<クレート>.<動詞>`（[`names::is_valid_span_name`]）。
//! - カウンタとゲージは [`counter!`] と [`gauge!`] で `tracing` のイベントとして出し、
//!   [`MetricsLayer`] が集計する。
//! - 外で測った時間（GPU のタイムスタンプなど）は [`record_span`] でスパンの分布に載せる。
//! - 常時の出力は [`MetricsLayer::snapshot`] の 1 行（`metrics.jsonl`）、
//!   詳しくは [`ChromeTraceLayer`] の Chrome trace event（Perfetto UI で開ける）。
//!
//! wgpu のタイムスタンプを読む所は `mm-render` に置く。このクレートは GPU に依存しない。

pub mod chrome;
mod json;
pub mod metrics;
pub mod names;

pub use chrome::ChromeTraceLayer;
pub use metrics::{MetricsHandle, MetricsLayer, MetricsSnapshot, SpanStats};

/// `counter!` と `gauge!` のイベントの target。[`MetricsLayer`] はこの target だけを見る。
pub const METRICS_TARGET: &str = "mm.metrics";

/// カウンタを加算する。`counter!("cache.hit", 1)`。名前は `<種類>.<何>`。
#[macro_export]
macro_rules! counter {
    ($name:expr, $value:expr) => {
        $crate::__private::tracing::event!(
            target: $crate::METRICS_TARGET,
            $crate::__private::tracing::Level::INFO,
            metric = "counter",
            name = $name,
            value = ($value) as f64
        )
    };
}

/// ゲージを置き換える。`gauge!("memory.rss_bytes", bytes)`。
#[macro_export]
macro_rules! gauge {
    ($name:expr, $value:expr) => {
        $crate::__private::tracing::event!(
            target: $crate::METRICS_TARGET,
            $crate::__private::tracing::Level::INFO,
            metric = "gauge",
            name = $name,
            value = ($value) as f64
        )
    };
}

/// 外で測った長さを、名前 `name` のスパンが 1 回閉じたものとして記録する。
///
/// スパンの開閉では測れない時間（GPU のタイムスタンプなど）を、CPU のスパンと同じ分布に載せる。
pub fn record_span(name: &str, duration: std::time::Duration) {
    debug_assert!(
        names::is_valid_span_name(name),
        "スパン名が決まりに合わない: {name}"
    );
    __private::tracing::event!(
        target: METRICS_TARGET,
        __private::tracing::Level::INFO,
        metric = "span",
        name = name,
        value = duration.as_micros() as f64
    );
}

/// マクロが使う再輸出。利用側のクレートが `tracing` を直接依存に持たなくてよい。
pub mod __private {
    pub use tracing;
}
