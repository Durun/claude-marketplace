use super::*;
use mm_audio_api::DEFAULT_SAMPLE_RATE;
use mm_decode_api::EncodeReport;
use mm_decode_api::{EncodeSettings, Encoder};
use mm_runtime::{assemble, RuntimeConfig};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::{vertical_project, VERTICAL_DURATION_SECONDS};
use mm_trace::MetricsLayer;
use mm_types::{RationalTime, RgbaImage};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use tracing_subscriber::prelude::*;

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mm_export_{name}_{}", std::process::id()))
}

/// 全部のテストをスパンを集める購読者の下で実行する。購読者なしのテストが並行に実行されると、
/// tracing が呼び出し点を「誰も見ない」と記憶してしまい、集める側のテストが数え強制終了する。
fn with_metrics<T>(f: impl FnOnce() -> T) -> (T, mm_trace::MetricsHandle) {
    let (layer, metrics) = MetricsLayer::new();
    let subscriber = tracing_subscriber::registry().with(layer);
    (tracing::subscriber::with_default(subscriber, f), metrics)
}

fn out_options(out: &Path) -> ExportOptions {
    ExportOptions {
        output: Some(out.to_path_buf()),
        ..ExportOptions::default()
    }
}

#[test]
fn 出力先を上書きして書き出し工程ごとのスパンが出る() {
    let media = SampleMedia::default();
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("dir").join("nested").join("out.json");
    let (summary, metrics) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        export_project(&mut engine, &project, &out_options(&out)).unwrap()
    });
    let frames = VERTICAL_DURATION_SECONDS as u64 * media.fps.num() as u64;
    assert_eq!(summary.output_path, out);
    assert_eq!(summary.frames, frames);
    assert_eq!(
        summary.audio_frames,
        VERTICAL_DURATION_SECONDS as u64 * DEFAULT_SAMPLE_RATE as u64
    );
    assert!(out.exists(), "親ディレクトリごと作られる");
    assert!(!partial_path(&out).exists(), "一時ファイルは残らない");
    let written: EncodeReport = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
    assert_eq!(written.frames, frames, "エンコーダーの報告が出力と合う");

    let snap = metrics.snapshot();
    assert_eq!(snap.spans["export.run"].count, 1);
    assert_eq!(snap.spans["export.render"].count, frames);
    assert_eq!(
        snap.spans["export.encode"].count,
        frames.div_ceil(EXPORT_BATCH_FRAMES as u64)
    );
    assert_eq!(snap.counters["export.frames"], frames as f64);
    let _ = std::fs::remove_dir_all(temp("dir"));
}

#[test]
fn 半端なフレームで切れる_fps_では音を映像の長さに揃える() {
    let media = SampleMedia {
        fps: mm_types::Rational::new(30_000, 1001).unwrap(),
        ..SampleMedia::default()
    };
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("ntsc").join("out.json");
    let (summary, _) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        export_project(&mut engine, &project, &out_options(&out)).unwrap()
    });
    // 2 秒は 59.94 フレーム。59 フレーム（1.9686 秒）で切れ、音も同じ長さ。
    assert_eq!(summary.frames, 59);
    let video_end = RationalTime::from_frames(59, media.fps).unwrap();
    let expected = mm_node_api::audio_frames_in(
        TimeRange::new(RationalTime::ZERO, video_end).unwrap(),
        DEFAULT_SAMPLE_RATE,
    ) as u64;
    assert_eq!(summary.audio_frames, expected, "音は映像の長さに揃う");
    assert!(summary.audio_frames < 2 * DEFAULT_SAMPLE_RATE as u64);
    let _ = std::fs::remove_dir_all(temp("ntsc"));
}

/// 送るたびに失敗し、何回呼ばれたかと最初のエラーを残す。
#[derive(Default)]
struct FailingEncoder {
    pushes: usize,
}

impl Encoder for FailingEncoder {
    fn begin(&mut self, _settings: &EncodeSettings) -> Result<()> {
        Ok(())
    }

    fn push_frames(&mut self, _frames: &[RgbaImage]) -> Result<()> {
        self.pushes += 1;
        Err(MmError::Corrupt(format!("push {}", self.pushes)))
    }

    fn push_audio(&mut self, _block: &mm_audio_api::AudioBlock) -> Result<()> {
        Ok(())
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        Err(MmError::Internal("finish は呼ばれない".into()))
    }
}

#[test]
fn エンコーダーの失敗は最初のエラーで止まる() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &temp("unused.json"));
    let settings = EncodeSettings {
        size: media.size,
        fps: media.fps,
        codec: "sample".into(),
        output_path: temp("fail.json"),
        audio: None,
    };
    let control = ExportControl::default();
    let plan = Plan {
        target: &Id::new("mp4"),
        composition: &comp,
        range: project.composition(&comp).unwrap().range().unwrap(),
        control: &control,
        parallel: 0,
        parallel_min_frames: None,
        pass: PassPosition { index: 1, of: 1 },
    };
    let mut encoder = FailingEncoder::default();
    let (result, _) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        export_frames(&mut engine, &project, &plan, &settings, &mut encoder)
    });
    let err = result.expect_err("失敗する");
    assert!(
        matches!(&err, MmError::Corrupt(m) if m == "push 1"),
        "最初のエラー: {err}"
    );
    assert_eq!(encoder.pushes, 1, "失敗の後は描き続けない");
}

use mm_types::Id;

#[test]
fn 無いレンダーターゲットは拒否する() {
    let project = Project::new();
    assert!(pick_target(&project, None).is_err());
}

/// 縦の 1 本のフィクスチャに、別の大きさと出力先のレンダーターゲット `small` を足したプロジェクト。
fn two_targets(dir: &Path) -> Project {
    let media = SampleMedia::default();
    let (mut project, _, _) = vertical_project(&media, &dir.join("big.json"));
    let mut small = project.render_targets[0].clone();
    small.id = Id::new("small");
    small.width = media.size.width / 2;
    small.height = media.size.height / 2;
    small.output_path = dir.join("small.json").to_string_lossy().into_owned();
    project.render_targets.push(small);
    project
}

fn all_ids(project: &Project) -> Vec<RenderTargetId> {
    project
        .render_targets
        .iter()
        .map(|t| t.id.clone())
        .collect()
}

#[test]
fn レンダーターゲットごとに別のファイルへ書き出す() {
    let dir = temp("multi");
    let project = two_targets(&dir);
    let (batch, _) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        export_targets(
            &mut engine,
            &project,
            &all_ids(&project),
            &ExportOptions::default(),
        )
        .unwrap()
    });
    assert!(!batch.cancelled);
    let names: Vec<_> = batch.done.iter().map(|s| s.target.to_string()).collect();
    assert_eq!(names, ["mp4", "small"]);
    assert!(dir.join("big.json").exists() && dir.join("small.json").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 同じ出力先を指すレンダーターゲットは始める前に拒否する() {
    let dir = temp("shared");
    let mut project = two_targets(&dir);
    project.render_targets[1].output_path = project.render_targets[0].output_path.clone();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let err = export_targets(
        &mut engine,
        &project,
        &all_ids(&project),
        &ExportOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("同じ出力先"), "{err}");
    assert!(!dir.exists(), "何も書かない");

    let over = ExportOptions {
        output: Some(dir.join("x.json")),
        ..ExportOptions::default()
    };
    let project = two_targets(&dir);
    assert!(export_targets(&mut engine, &project, &all_ids(&project), &over).is_err());
}

#[test]
fn 範囲を指定すると_その区間だけ書き出す() {
    let media = SampleMedia::default();
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("range").join("out.json");
    let half =
        TimeRange::from_start_duration(RationalTime::seconds(1), RationalTime::seconds(1)).unwrap();
    let (summary, _) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        let options = ExportOptions {
            range: Some(half),
            ..out_options(&out)
        };
        export_project(&mut engine, &project, &options).unwrap()
    });
    assert_eq!(summary.frames, media.fps.num() as u64, "1 秒分");
    assert_eq!(summary.audio_frames, DEFAULT_SAMPLE_RATE as u64);

    let outside = TimeRange::new(RationalTime::ZERO, RationalTime::seconds(9)).unwrap();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let options = ExportOptions {
        range: Some(outside),
        ..out_options(&out)
    };
    let err = export_project(&mut engine, &project, &options).unwrap_err();
    assert!(matches!(err, MmError::InvalidArgument(_)), "{err}");
    let _ = std::fs::remove_dir_all(temp("range"));
}

#[test]
fn 取り消すと一時ファイルを残さず_前の出力も壊さない() {
    let media = SampleMedia::default();
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("cancel").join("out.json");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, b"previous").unwrap();

    let flag = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicU64::new(0));
    let (outcome, _) = with_metrics(|| {
        let (f, s) = (flag.clone(), seen.clone());
        let control = ExportControl::default()
            .with_cancel(flag.clone())
            .with_progress(Arc::new(move |p| {
                s.store(p.frames_done, Ordering::Relaxed);
                if p.frames_done == 10 {
                    f.store(true, Ordering::Relaxed);
                }
            }));
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        let options = ExportOptions {
            control,
            ..out_options(&out)
        };
        export_run(&mut engine, &project, &options).unwrap()
    });
    assert_eq!(outcome, ExportOutcome::Cancelled);
    let total = VERTICAL_DURATION_SECONDS as u64 * media.fps.num() as u64;
    assert!(
        (10..total).contains(&seen.load(Ordering::Relaxed)),
        "途中で止まる: {}",
        seen.load(Ordering::Relaxed)
    );
    assert!(!partial_path(&out).exists(), "一時ファイルは消える");
    assert_eq!(std::fs::read(&out).unwrap(), b"previous");
    let _ = std::fs::remove_dir_all(temp("cancel"));
}

#[test]
fn 進み具合は最後のフレームで全体に届く() {
    let media = SampleMedia::default();
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("progress").join("out.json");
    let last = Arc::new(std::sync::Mutex::new(None));
    let sink = last.clone();
    let control = ExportControl::default().with_progress(Arc::new(move |p| {
        *sink.lock().unwrap() = Some((p.frames_done, p.frames_total));
    }));
    let (summary, _) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        let options = ExportOptions {
            control,
            ..out_options(&out)
        };
        export_project(&mut engine, &project, &options).unwrap()
    });
    assert_eq!(
        *last.lock().unwrap(),
        Some((summary.frames, summary.frames))
    );
    let _ = std::fs::remove_dir_all(temp("progress"));
}

#[test]
fn 検証に失敗した書き出しは前の出力を残し_一時ファイルを消す() {
    let media = SampleMedia::default();
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("verify").join("out.json");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    std::fs::write(&out, b"previous").unwrap();
    let options = ExportOptions {
        verify: true,
        ..out_options(&out)
    };
    let (result, _) = with_metrics(|| {
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        export_run(&mut engine, &project, &options)
    });
    // サンプルのエンコーダーは JSON を書くので、ffprobe の検証は通らない。
    assert!(result.is_err(), "検証が失敗する");
    assert_eq!(std::fs::read(&out).unwrap(), b"previous");
    assert!(!partial_path(&out).exists(), "一時ファイルは消える");
    let _ = std::fs::remove_dir_all(temp("verify"));
}

#[test]
fn 一方の一時ファイルが他方の出力と同じ名前になる組は始める前に拒否する() {
    let dir = temp("partial_clash");
    let mut project = two_targets(&dir);
    project.render_targets[0].output_path = dir.join("x.mp4").to_string_lossy().into_owned();
    project.render_targets[1].output_path =
        dir.join("x.partial.mp4").to_string_lossy().into_owned();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let err = export_targets(
        &mut engine,
        &project,
        &all_ids(&project),
        &ExportOptions::default(),
    )
    .unwrap_err();
    assert!(err.to_string().contains("同じ出力先"), "{err}");
    assert!(!dir.exists(), "何も書かない");
}

#[test]
fn 取り消しの旗が立っていても_別のエラーは取り消しにしない() {
    assert!(is_cancel_error(&MmError::Internal(CANCELLED_MARK.into())));
    assert!(!is_cancel_error(&MmError::Io(std::io::Error::other(
        "ディスクが満杯"
    ))));
    assert!(!is_cancel_error(&MmError::Internal(
        "別の内部エラー".into()
    )));
}
