//! フレームのキャッシュと PTS の索引のキャッシュの効きを測る。時間は `mm-trace` のスパン `render.frame`
//! （リードバックを含まない、1 フレームを描く CPU の時間）、ヒットはカウンターで読む。
//!
//! `cargo run -p mm-runtime --release --example bench_frame_cache --features ffmpeg -- <素材> <cache のフォルダー> [decode]`
//!
//! 測るもの: 素材を開く時間（1 回目と、索引が置いてある 2 回目）、同じ位置へのシーク（1 回目と 2 回目）、
//! 区間の 2 回目、操作で捨てられるキャッシュの量（触ったレイヤーだけか、素材か）。

use std::path::PathBuf;
use std::time::Instant;

use mm_doc::{Asset, Composition, Layer, Node, Project};
use mm_node_api::Quality;
use mm_runtime::{assemble, Backend, Engine, RuntimeConfig};
use mm_trace::{MetricsHandle, MetricsLayer, MetricsSnapshot};
use mm_types::{ColorSpace, Id, MmError, ParamValue, Rational, RationalTime, Result, Rgba, Size};
use tracing_subscriber::layer::SubscriberExt;

const SIZE: Size = Size::new(1920, 1080);
const FPS: u32 = 30;
/// シークする位置（秒）。素材の後ろのほう。
const SEEK_SECONDS: i64 = 45;
/// 区間のフレーム数。
const PASS_FRAMES: i64 = 60;
const MIB: f64 = 1024.0 * 1024.0;

fn project(path: &str) -> Project {
    let mut p = Project::new();
    p.assets.push(Asset {
        id: Id::new("clip"),
        path: path.into(),
        content_hash: "bench-clip".into(),
        color_space: ColorSpace::bt709(),
        duration: RationalTime::seconds(60),
        fps: Some(Rational::integer(FPS)),
        timebase_offset: RationalTime::ZERO,
    });
    let color = ParamValue::Color(Rgba::new(0.1, 0.1, 0.1, 1.0));
    let bg = Node::new("solid", 1).with_param("color", color);
    let clip = ParamValue::AssetRef(Id::new("clip"));
    let footage = Node::new("footage", 1).with_param("asset", clip);
    let layer = |id: &str, node: Node| {
        Layer::new(
            id,
            node,
            RationalTime::ZERO,
            RationalTime::ZERO,
            RationalTime::seconds(60),
        )
    };
    p.compositions.push(
        Composition::new(
            Id::new("main"),
            SIZE.width,
            SIZE.height,
            Rational::integer(FPS),
            RationalTime::seconds(60),
        )
        .with_layers(vec![layer("bg", bg), layer("clip-layer", footage)]),
    );
    p
}

/// 1 フレーム描き、`render.frame` の時間（ms）と、その間の集計を返す。
fn frame(
    engine: &mut Engine,
    p: &Project,
    index: i64,
    metrics: &MetricsHandle,
) -> Result<(f64, MetricsSnapshot)> {
    let t = RationalTime::from_frames(index, Rational::integer(FPS))?;
    engine.render_frame(p, &Id::new("main"), t, SIZE, Quality::Full)?;
    let snap = metrics.drain();
    let ms = snap
        .spans
        .get("render.frame")
        .map_or(0.0, |s| s.max_us as f64 / 1000.0);
    Ok((ms, snap))
}

/// 区間を描いて、`render.frame` の合計（ms）と集計を返す。
fn pass(
    engine: &mut Engine,
    p: &Project,
    metrics: &MetricsHandle,
) -> Result<(f64, MetricsSnapshot)> {
    let mut total = 0.0;
    let mut hits = 0.0;
    let mut lookups = 0.0;
    let mut last = MetricsSnapshot::default();
    for i in 0..PASS_FRAMES {
        let (ms, snap) = frame(engine, p, i, metrics)?;
        total += ms;
        hits += snap
            .counters
            .get("cache.frame_hits")
            .copied()
            .unwrap_or(0.0);
        lookups += snap
            .counters
            .get("cache.frame_lookups")
            .copied()
            .unwrap_or(0.0);
        last = snap;
    }
    last.counters.insert("cache.frame_hits".into(), hits);
    last.counters.insert("cache.frame_lookups".into(), lookups);
    Ok((total, last))
}

fn counter(snap: &MetricsSnapshot, name: &str) -> f64 {
    snap.counters.get(name).copied().unwrap_or(0.0)
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let media = args
        .next()
        .ok_or_else(|| MmError::InvalidArgument("素材のパスが要る".into()))?;
    let dir = PathBuf::from(
        args.next()
            .ok_or_else(|| MmError::InvalidArgument("cache のフォルダーが要る".into()))?,
    );
    let decode: Backend = args.next().unwrap_or_else(|| "ffmpeg".into()).parse()?;
    let (layer, metrics) = MetricsLayer::new();
    tracing::subscriber::set_global_default(tracing_subscriber::registry().with(layer))
        .map_err(|e| MmError::Internal(e.to_string()))?;
    let _ = std::fs::remove_dir_all(&dir);
    let config = RuntimeConfig {
        decode,
        cache: Backend::Fs,
        cache_dir: Some(dir),
        ..RuntimeConfig::real()
    };
    let p = project(&media);

    let mut first = assemble(&config)?;
    let started = Instant::now();
    frame(&mut first, &p, 0, &metrics)?;
    let open1 = started.elapsed().as_secs_f64() * 1000.0;
    drop(first);
    // 索引が置いてあるので、2 回目の開くはパケットを読まない。
    let mut second = assemble(&config)?;
    let started = Instant::now();
    let (_, snap) = frame(&mut second, &p, 0, &metrics)?;
    let open2 = started.elapsed().as_secs_f64() * 1000.0;
    println!(
        "開く（1 フレーム目まで、リードバックを含む）: 1 回目 {open1:.0} ms、2 回目 {open2:.0} ms（索引のヒット {}）",
        counter(&snap, "decode.index_cache_hits")
    );

    let seek = SEEK_SECONDS * i64::from(FPS);
    let (seek1, s1) = frame(&mut second, &p, seek, &metrics)?;
    let (seek2, s2) = frame(&mut second, &p, seek, &metrics)?;
    println!(
        "同じ位置（{SEEK_SECONDS} 秒）へのシーク render.frame: 1 回目 {seek1:.1} ms（ヒット {}/{}）、2 回目 {seek2:.1} ms（ヒット {}/{}）",
        counter(&s1, "cache.frame_hits"),
        counter(&s1, "cache.frame_lookups"),
        counter(&s2, "cache.frame_hits"),
        counter(&s2, "cache.frame_lookups"),
    );

    let (cold, _) = pass(&mut second, &p, &metrics)?;
    let (warm, snap) = pass(&mut second, &p, &metrics)?;
    println!(
        "{PASS_FRAMES} フレームの区間 render.frame の合計: 1 回目 {cold:.0} ms、2 回目 {warm:.0} ms（2 回目のヒット {}/{}）、キャッシュ {} 枚 / {:.0} MiB",
        counter(&snap, "cache.frame_hits"),
        counter(&snap, "cache.frame_lookups"),
        snap.gauges.get("cache.frame_entries").copied().unwrap_or(0.0),
        snap.gauges.get("cache.frame_memory_bytes").copied().unwrap_or(0.0) / MIB,
    );

    // 操作の後に捨てられる量。背景のレイヤー、素材の順に触る。
    for touched in ["bg", "clip"] {
        second.invalidate_touched(&[touched.to_string()]);
        let dropped = metrics.drain();
        println!(
            "{touched} を触った操作: 捨てた {} 枚 / {:.0} MiB（残り {} 枚）",
            counter(&dropped, "cache.frame_invalidated"),
            counter(&dropped, "cache.frame_invalidated_bytes") / MIB,
            dropped
                .gauges
                .get("cache.frame_entries")
                .copied()
                .unwrap_or(0.0)
        );
        let (redo, _) = pass(&mut second, &p, &metrics)?;
        println!("  その後の区間 {redo:.0} ms");
    }
    Ok(())
}
