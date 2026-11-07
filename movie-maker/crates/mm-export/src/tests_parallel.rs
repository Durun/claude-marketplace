//! 並行の書き出しと 2 パスの書き出しのテスト。
use super::*;
use mm_decode_api::EncodeReport;
use mm_decode_api::{EncodeSettings, Encoder};
use mm_runtime::{assemble, RuntimeConfig};
use mm_testkit::decode::{frame_index_of, SampleMedia};
use mm_testkit::fixtures::{vertical_project, VERTICAL_DURATION_SECONDS};
use mm_types::Id;
use mm_types::RgbaImage;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("mm_export_par_{name}_{}", std::process::id()))
}

/// 送られたフレームの番号と、`begin_pass` の呼ばれ方を残す。
#[derive(Default, Clone)]
struct Log {
    indices: Arc<Mutex<Vec<i64>>>,
    passes: Arc<Mutex<Vec<u32>>>,
}

struct Recorder {
    log: Log,
    passes: u32,
}

impl Encoder for Recorder {
    fn begin(&mut self, _settings: &EncodeSettings) -> Result<()> {
        Ok(())
    }

    fn passes(&self) -> u32 {
        self.passes
    }

    fn begin_pass(&mut self, pass: u32) -> Result<()> {
        self.log.passes.lock().unwrap().push(pass);
        // 2 回目の映像は、1 回目と別に並べて見られるよう区切りを入れる
        self.log.indices.lock().unwrap().push(-1);
        Ok(())
    }

    fn push_frames(&mut self, frames: &[RgbaImage]) -> Result<()> {
        let mut v = self.log.indices.lock().unwrap();
        v.extend(frames.iter().map(|f| frame_index_of(f.pixel(0, 0))));
        Ok(())
    }

    fn push_audio(&mut self, _block: &mm_audio_api::AudioBlock) -> Result<()> {
        Ok(())
    }

    fn finish(&mut self) -> Result<EncodeReport> {
        Err(MmError::Internal("このテストでは閉じない".into()))
    }
}

fn settings_of(media: &SampleMedia) -> EncodeSettings {
    EncodeSettings {
        size: media.size,
        fps: media.fps,
        codec: "sample".into(),
        output_path: temp("unused.json"),
        audio: None,
    }
}

#[test]
fn 並行で描いたフレームの番号は_1_つのレンダラーと同じ順に送られる() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &temp("unused.json"));
    let range = project.composition(&comp).unwrap().range().unwrap();
    let settings = settings_of(&media);
    let config = RuntimeConfig::sample();

    let serial = Log::default();
    let mut engine = assemble(&config).unwrap();
    let control = ExportControl::default();
    let plan = Plan {
        target: &Id::new("mp4"),
        composition: &comp,
        range,
        control: &control,
        parallel: 0,
        parallel_min_frames: None,
        pass: PassPosition { index: 1, of: 1 },
    };
    let mut rec = Recorder {
        log: serial.clone(),
        passes: 1,
    };
    export_frames(&mut engine, &project, &plan, &settings, &mut rec).unwrap();
    let expected = serial.indices.lock().unwrap().clone();
    assert_eq!(expected.len() as i64, VERTICAL_DURATION_SECONDS * 30);

    // チャンクを 7 フレームにして、ワーカー 3 本で 9 個のチャンクを先取りさせる
    for workers in [2, 3, 5] {
        let log = Log::default();
        let mut rec = Recorder {
            log: log.clone(),
            passes: 1,
        };
        let job = parallel::ParallelJob {
            project: &project,
            composition: &comp,
            range,
            settings: &settings,
            workers,
            config: config.clone(),
            chunk_frames: Some(7),
        };
        let mut sunk = Vec::new();
        let n = parallel::export_parallel(&job, &mut rec, &mut |i, _| sunk.push(i)).unwrap();
        assert_eq!(n, expected.len(), "ワーカー {workers}");
        assert_eq!(*log.indices.lock().unwrap(), expected, "ワーカー {workers}");
        let first = range.frame_indices(settings.fps).start;
        let want: Vec<i64> = (first..first + n as i64).collect();
        assert_eq!(sunk, want, "進み具合の通知もフレームの順");
    }
}

#[test]
fn 並行でも書き出しの数と音は同じで_半端な_fps_の音も揃う() {
    let media = SampleMedia {
        fps: mm_types::Rational::new(30_000, 1001).unwrap(),
        ..SampleMedia::default()
    };
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let mut summaries = Vec::new();
    for parallel in [0, 3] {
        let out = temp(&format!("ntsc{parallel}")).join("out.json");
        let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
        let options = ExportOptions {
            output: Some(out.clone()),
            parallel,
            parallel_min_frames: Some(1),
            ..ExportOptions::default()
        };
        let s = export_project(&mut engine, &project, &options).unwrap();
        let written: EncodeReport = serde_json::from_slice(&std::fs::read(&out).unwrap()).unwrap();
        assert_eq!(written.frames, s.frames);
        summaries.push((s.frames, s.audio_frames));
        let _ = std::fs::remove_dir_all(out.parent().unwrap());
    }
    assert_eq!(
        summaries[0], summaries[1],
        "並行でも映像と音の数が変わらない"
    );
    assert_eq!(summaries[0].0, 59);
}

#[test]
fn 並行の書き出しを取り消すと一時ファイルを残さず戻る() {
    let media = SampleMedia::default();
    let (project, _, _) = vertical_project(&media, &temp("unused.json"));
    let out = temp("cancel").join("out.json");
    let flag = Arc::new(AtomicBool::new(false));
    let seen = Arc::new(AtomicU64::new(0));
    let (f, s) = (flag.clone(), seen.clone());
    let control = ExportControl::default()
        .with_cancel(flag.clone())
        .with_progress(Arc::new(move |p| {
            s.store(p.frames_done, Ordering::Relaxed);
            if p.frames_done >= 10 {
                f.store(true, Ordering::Relaxed);
            }
        }));
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let options = ExportOptions {
        output: Some(out.clone()),
        parallel: 3,
        parallel_min_frames: Some(1),
        control,
        ..ExportOptions::default()
    };
    let outcome = export_run(&mut engine, &project, &options).unwrap();
    assert_eq!(outcome, ExportOutcome::Cancelled);
    assert!(!partial_path(&out).exists(), "一時ファイルは消える");
    let total = VERTICAL_DURATION_SECONDS as u64 * media.fps.num() as u64;
    assert!(seen.load(Ordering::Relaxed) < total, "途中で止まる");
    let _ = std::fs::remove_dir_all(temp("cancel"));
}

#[test]
fn ワーカーを組めない設定は並行の書き出しのエラーになる() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &temp("unused.json"));
    let range = project.composition(&comp).unwrap().range().unwrap();
    let settings = settings_of(&media);
    // decode に選べない実装（fs はキャッシュの実装）を選んで、ワーカーのエンジンを組めない設定にする
    let config = RuntimeConfig {
        decode: mm_runtime::Backend::Fs,
        ..RuntimeConfig::sample()
    };
    let mut rec = Recorder {
        log: Log::default(),
        passes: 1,
    };
    let job = parallel::ParallelJob {
        project: &project,
        composition: &comp,
        range,
        settings: &settings,
        workers: 2,
        config,
        chunk_frames: Some(10),
    };
    assert!(parallel::export_parallel(&job, &mut rec, &mut |_, _| {}).is_err());
}

#[test]
fn 二パスのエンコーダーには同じ映像を_2_回送り_進み具合は_2_回分になる() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &temp("unused.json"));
    let range = project.composition(&comp).unwrap().range().unwrap();
    let settings = settings_of(&media);
    let log = Log::default();
    let mut rec = Recorder {
        log: log.clone(),
        passes: 2,
    };
    let last = Arc::new(Mutex::new((0, 0)));
    let sink = last.clone();
    let control = ExportControl::default().with_progress(Arc::new(move |p| {
        *sink.lock().unwrap() = (p.frames_done, p.frames_total);
    }));
    let plan = Plan {
        target: &Id::new("mp4"),
        composition: &comp,
        range,
        control: &control,
        parallel: 0,
        parallel_min_frames: None,
        pass: PassPosition { index: 1, of: 1 },
    };
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let frames = export_passes(&mut engine, &project, &plan, &settings, &mut rec).unwrap();
    assert_eq!(*log.passes.lock().unwrap(), [1, 2]);
    let indices = log.indices.lock().unwrap().clone();
    let halves: Vec<&[i64]> = indices
        .split(|i| *i == -1)
        .filter(|h| !h.is_empty())
        .collect();
    assert_eq!(halves.len(), 2);
    assert_eq!(halves[0], halves[1], "2 回とも同じフレームの並び");
    assert_eq!(frames, halves[0].len(), "数えるのは最後の 1 回");
    assert_eq!(
        *last.lock().unwrap(),
        (2 * frames as u64, 2 * frames as u64)
    );
}

#[test]
fn コーデックのコンテナと合わない拡張子は書き出す前に拒否する() {
    for (codec, name, ok) in [
        ("prores_422hq", "a.mp4", false),
        ("dnxhr_hq", "a.mxf", false),
        ("prores_4444", "a.MOV", true),
        ("h264", "a.mp4", true),
        ("h264_2pass", "a.M4V", true),
        ("hevc_nvenc", "a.mp4", true),
        ("h264_nvenc", "a.mov", false),
        ("libx264", "a.mkv", false),
        ("hevc_nvenc", "a", false),
        ("sample", "a.json", true),
    ] {
        let r = check_container(codec, Path::new(name));
        assert_eq!(r.is_ok(), ok, "{codec} {name}");
    }
}

#[test]
fn 音を短い区間ずつ混ぜても_1_回で混ぜたのと同じ() {
    let (project, comp, _) = vertical_project(&SampleMedia::default(), &temp("unused.json"));
    let range = project.composition(&comp).unwrap().range().unwrap();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let whole = engine.render_audio(&project, &comp, range).unwrap();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let step = mm_types::RationalTime::new(1, 3).unwrap();
    let pieces = passes::mix_audio(&mut engine, &project, &comp, range, step, &|| false)
        .unwrap()
        .unwrap();
    assert_eq!(pieces.samples.len(), whole.samples.len());
    assert_eq!(pieces, whole);
}

#[test]
fn 止める旗が立てば音を混ぜずに戻る() {
    let (project, comp, _) = vertical_project(&SampleMedia::default(), &temp("unused.json"));
    let range = project.composition(&comp).unwrap().range().unwrap();
    let mut engine = assemble(&RuntimeConfig::sample()).unwrap();
    let step = mm_types::RationalTime::new(1, 3).unwrap();
    let mixed = passes::mix_audio(&mut engine, &project, &comp, range, step, &|| true).unwrap();
    assert!(mixed.is_none());
}

#[test]
fn nvenc_は並行の指定があっても並行にしない() {
    let media = SampleMedia::default();
    let (project, comp, _) = vertical_project(&media, &temp("unused.json"));
    let range = project.composition(&comp).unwrap().range().unwrap();
    let control = ExportControl::default();
    let plan = Plan {
        target: &Id::new("mp4"),
        composition: &comp,
        range,
        control: &control,
        parallel: 4,
        parallel_min_frames: Some(1),
        pass: PassPosition { index: 1, of: 1 },
    };
    let mut settings = settings_of(&media);
    assert!(plan.concurrent(&settings), "モックのコーデックは並行にする");
    for codec in ["h264_nvenc", "hevc_nvenc"] {
        settings.codec = codec.into();
        assert!(!plan.concurrent(&settings), "{codec}");
    }
}
