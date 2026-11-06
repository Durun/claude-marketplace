//! `playback` と `scrub` シナリオ。ウィンドウを出さず、レンダラーだけでプレビューの描画を回す。
//! GPU が要るので CI では回さず、手元の GPU で測る（docs/ci.md）。

use std::time::{Duration, Instant};

use mm_doc::{Composition, Project};
use mm_node_api::Quality;
use mm_runtime::{assemble, Backend, Engine, RuntimeConfig};
use mm_trace::MetricsHandle;
use mm_types::{CompositionId, GpuAdapterId, MmError, RationalTime, Result, Size};

use crate::perf::{percentile, Budget, Environment, Item, Limit, PerfArgs};
use crate::project::load_project;

const FRAME_ROW: &str = "描画の 1 フレーム（プレビュー、1080p、レイヤー 10 枚、文字 3 枚）";
const DROP_ROW: &str = "再生で落とすフレーム";
const SCRUB_ROW: &str = "スクラブ（縮小したフレームがサーフェスに出るまで）";
const MS_PER_SECOND: f64 = 1000.0;
const US_PER_MS: f64 = 1000.0;
/// 描画の 1 フレームの CPU の予算（ミリ秒）。
const CPU_BUDGET_MS: f64 = 4.0;
/// 描画の 1 フレームの GPU の予算（ミリ秒）。
const GPU_BUDGET_MS: f64 = 6.0;
/// キャッシュが無い再生で落としてよいフレームの割合（%）。
const DROP_BUDGET_PCT: f64 = 1.0;
/// キャッシュがある再生で落としてよいフレームの割合（%）。
const DROP_CACHED_BUDGET_PCT: f64 = 0.0;
/// 縮小したフレームの予算（ミリ秒）。
const PREVIEW_BUDGET_MS: f64 = 50.0;
/// 正確なフレームの予算（ミリ秒）。
const FULL_BUDGET_MS: f64 = 150.0;
/// 測る前に描いておく枚数（パイプラインとバッファの用意を測りに入れない）。
const WARMUP_FRAMES: u32 = 3;
/// 縮小したフレームの縦横の割合の分母（半分の大きさ）。
const PREVIEW_DIVISOR: u32 = 2;
/// シーク位置の疑似乱数。結果が毎回同じ位置になるよう固定する。
const LCG_MUL: u64 = 6_364_136_223_846_793_005;
const LCG_ADD: u64 = 1_442_695_040_888_963_407;
const LCG_SEED: u64 = 0x5eed;
/// 残りがこれ未満なら寝ずに待つ（スリープの粒度で次のフレームが遅れないように）。
const SPIN_MARGIN: Duration = Duration::from_millis(2);

/// シナリオに使うエンジンとコンポジション。
struct Rig {
    engine: Engine,
    project: Project,
    comp: Composition,
}

/// フレームのキャッシュを使うか。予算の表は「キャッシュなし」と「キャッシュあり」を分けて持つ。
#[derive(Clone, Copy, PartialEq, Eq)]
enum FrameCache {
    Off,
    On,
}

fn rig(args: &PerfArgs, env: &mut Environment, frame_cache: FrameCache) -> Result<Rig> {
    let mut project = load_project(&args.project)?;
    // 遅い GPU（CI の WARP）では小さく描いて回数を稼ぐ。既定の大きさは見本と同じ 1080p。
    for comp in &mut project.compositions {
        (comp.width, comp.height) = args.size;
    }
    let comp = project
        .compositions
        .first()
        .cloned()
        .ok_or_else(|| MmError::NotFound("コンポジションが無い".into()))?;
    // 素材の無いプロジェクトはデコーダーを使わないので、FFmpeg の無い設定でも測れる。
    let mut config = RuntimeConfig::real();
    if project.assets.is_empty() {
        config.decode = Backend::Sample;
        config.encode = Backend::Sample;
    }
    if let Some(pattern) = &args.adapter {
        config.gpu = Some(pick_adapter(pattern)?);
    }
    if frame_cache == FrameCache::Off {
        config.frame_memory_bytes = Some(0);
    }
    let engine = assemble(&config)?;
    env.adapter = engine.gpu_adapter().map(|a| a.name.clone());
    Ok(Rig {
        engine,
        project,
        comp,
    })
}

/// 名前に `pattern`（大小文字を区別しない）を含む最初の GPU。
fn pick_adapter(pattern: &str) -> Result<GpuAdapterId> {
    let want = pattern.to_lowercase();
    let all = mm_gpu::enumerate_adapters();
    all.iter()
        .find(|a| a.name.to_lowercase().contains(&want))
        .cloned()
        .ok_or_else(|| {
            let names: Vec<&str> = all.iter().map(|a| a.name.as_str()).collect();
            MmError::NotFound(format!("GPU {pattern} が無い（ある GPU: {names:?}）"))
        })
}

impl Rig {
    /// 時刻 `at` のフレームを描いてリードバックし、かかった時間（ミリ秒）を返す。
    fn render_ms(&mut self, at: RationalTime, size: Size, quality: Quality) -> Result<f64> {
        let id: CompositionId = self.comp.id.clone();
        let start = Instant::now();
        self.engine
            .render_frame(&self.project, &id, at, size, quality)?;
        Ok(start.elapsed().as_secs_f64() * MS_PER_SECOND)
    }

    fn fps_f64(&self) -> f64 {
        f64::from(self.comp.fps.num()) / f64::from(self.comp.fps.den())
    }

    fn full_size(&self) -> Size {
        Size::new(self.comp.width, self.comp.height)
    }

    fn frame_time(&self, index: u64) -> Result<RationalTime> {
        let frame = (index % self.loop_frames()?) as i64;
        RationalTime::from_frames(frame, self.comp.fps)
    }

    /// コンポジションの長さのフレーム数。再生はこの長さで繰り返す。
    fn loop_frames(&self) -> Result<u64> {
        let frames = (self.comp.duration.to_f64() * self.fps_f64()).floor() as u64;
        if frames == 0 {
            return Err(MmError::InvalidArgument(
                "コンポジションが 1 フレームより短い".into(),
            ));
        }
        Ok(frames)
    }

    fn warm_up(&mut self) -> Result<()> {
        let size = self.full_size();
        for _ in 0..WARMUP_FRAMES {
            self.render_ms(RationalTime::ZERO, size, Quality::Preview)?;
        }
        Ok(())
    }
}

/// 実時間の再生を `--repeat` 回した結果（回ごとの p50、p99、落とした割合）。
#[derive(Default)]
struct Played {
    p50: Vec<f64>,
    p99: Vec<f64>,
    dropped: Vec<f64>,
}

/// 実時間の速さ（コンポジションの fps）で `--seconds` 秒ぶんを `--repeat` 回再生する。
fn play(args: &PerfArgs, rig: &mut Rig) -> Result<Played> {
    rig.warm_up()?;
    let interval = Duration::from_secs_f64(1.0 / rig.fps_f64());
    let frames = (args.seconds * rig.fps_f64()).ceil() as u64;
    let mut out = Played::default();
    for _ in 0..args.repeat {
        let size = rig.full_size();
        let mut times = Vec::with_capacity(frames as usize);
        let started = Instant::now();
        for n in 0..frames {
            wait_until(started + interval.mul_f64(n as f64));
            let at = rig.frame_time(n)?;
            times.push(rig.render_ms(at, size, Quality::Preview)?);
        }
        let late = times
            .iter()
            .filter(|t| **t > interval.as_secs_f64() * MS_PER_SECOND)
            .count();
        out.dropped.push(late as f64 * 100.0 / frames as f64);
        out.p50.push(percentile(&times, 0.5));
        out.p99.push(percentile(&times, 0.99));
    }
    Ok(out)
}

/// `playback`: フレームのキャッシュなしで再生し、各フレームの時間と落とした数を測る。続けてキャッシュありで
/// 落とした数を測る。スパンの集計はキャッシュなしの再生の直後に取り、キャッシュに当たったフレームを混ぜない。
pub fn playback(
    args: &PerfArgs,
    env: &mut Environment,
    metrics: &MetricsHandle,
) -> Result<Vec<Item>> {
    let Played { p50, p99, dropped } = play(args, &mut rig(args, env, FrameCache::Off)?)?;
    let snapshot = metrics.snapshot();
    let cached = play(args, &mut rig(args, env, FrameCache::On)?)?;
    let span_ms = |name: &str| {
        snapshot
            .spans
            .get(name)
            .map(|s| s.p50_us as f64 / US_PER_MS)
    };
    let at_most = |limit: f64, row| {
        Some(Budget {
            limit: Limit::AtMost(limit),
            row,
        })
    };
    let mut items = vec![
        Item::from_samples("playback.frame_ms", "ms", p50, None),
        Item::from_samples("playback.frame_p99_ms", "ms", p99, None),
        Item::from_samples(
            "playback.dropped_pct",
            "%",
            dropped,
            at_most(DROP_BUDGET_PCT, DROP_ROW),
        ),
        Item::from_samples("playback.cached_frame_ms", "ms", cached.p50, None),
        Item::from_samples(
            "playback.cached_dropped_pct",
            "%",
            cached.dropped,
            at_most(DROP_CACHED_BUDGET_PCT, DROP_ROW),
        ),
    ];
    if let Some(cpu) = span_ms("render.frame") {
        items.push(Item::from_samples(
            "playback.cpu_ms",
            "ms",
            vec![cpu],
            at_most(CPU_BUDGET_MS, FRAME_ROW),
        ));
    }
    if let Some(gpu) = span_ms("render.gpu_frame") {
        items.push(Item::from_samples(
            "playback.gpu_ms",
            "ms",
            vec![gpu],
            at_most(GPU_BUDGET_MS, FRAME_ROW),
        ));
    }
    Ok(items)
}

/// `deadline` までスリープし、最後の数ミリ秒は回って待つ。
fn wait_until(deadline: Instant) {
    loop {
        let now = Instant::now();
        let Some(left) = deadline.checked_duration_since(now) else {
            return;
        };
        if left > SPIN_MARGIN {
            std::thread::sleep(left - SPIN_MARGIN);
        } else {
            std::hint::spin_loop();
        }
    }
}

/// 疑似乱数でシーク位置（フレームの番号）を `count` 個作る。
fn seek_frames(count: usize, loop_frames: u64) -> Vec<u64> {
    let mut state = LCG_SEED;
    (0..count)
        .map(|_| {
            state = state.wrapping_mul(LCG_MUL).wrapping_add(LCG_ADD);
            (state >> 33) % loop_frames
        })
        .collect()
}

/// `scrub`: ランダムな位置へ `--seeks` 回シークし、縮小したフレームと正確なフレームが出るまでの時間を測る。
pub fn scrub(args: &PerfArgs, env: &mut Environment) -> Result<Vec<Item>> {
    // 繰り返しで同じ位置に戻るので、キャッシュに当たらない初めてのシークの時間を測るためにキャッシュを切る。
    let mut rig = rig(args, env, FrameCache::Off)?;
    rig.warm_up()?;
    let full = rig.full_size();
    let preview = Size::new(full.width / PREVIEW_DIVISOR, full.height / PREVIEW_DIVISOR);
    let positions = seek_frames(args.seeks, rig.loop_frames()?);
    let (mut pv50, mut pv99, mut fl50, mut fl99) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..args.repeat {
        let (mut pv, mut fl) = (Vec::new(), Vec::new());
        for &n in &positions {
            let at = rig.frame_time(n)?;
            pv.push(rig.render_ms(at, preview, Quality::Preview)?);
            fl.push(rig.render_ms(at, full, Quality::Full)?);
        }
        pv50.push(percentile(&pv, 0.5));
        pv99.push(percentile(&pv, 0.99));
        fl50.push(percentile(&fl, 0.5));
        fl99.push(percentile(&fl, 0.99));
    }
    let at_most = |limit: f64| {
        Some(Budget {
            limit: Limit::AtMost(limit),
            row: SCRUB_ROW,
        })
    };
    Ok(vec![
        Item::from_samples("scrub.preview_p50_ms", "ms", pv50, None),
        Item::from_samples(
            "scrub.preview_p99_ms",
            "ms",
            pv99,
            at_most(PREVIEW_BUDGET_MS),
        ),
        Item::from_samples("scrub.full_p50_ms", "ms", fl50, None),
        Item::from_samples("scrub.full_p99_ms", "ms", fl99, at_most(FULL_BUDGET_MS)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn シーク位置は毎回同じで範囲に収まる() {
        let a = seek_frames(100, 600);
        assert_eq!(a, seek_frames(100, 600));
        assert!(a.iter().all(|n| *n < 600));
        assert!(a.iter().any(|n| *n != a[0]), "位置が散らばる");
    }
}
