//! 再生。専用のスレッドが `Engine` を持ち、再生の主の時計に合わせてフレームを描いて描画スレッドに渡し、
//! 音を `mm-audio-cpal` に足す。コマンド（再生、停止、シーク）はチャンネルで受ける。

mod intake;
mod player;

use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use mm_doc::Project;
use mm_types::{CompositionId, MmError, Result};

use crate::win::state::Shared;

pub use player::{runtime_config, PlayerOptions};

/// 再生スレッドへのコマンド。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    Play,
    Pause,
    Toggle,
    /// 秒で指す。範囲の外は端に収める。
    Seek(f64),
    /// 現在のフレームから `n` フレーム動かす（再生中なら止める）。
    Step(i64),
    /// `Shared::edits` に積んだ操作を適用する。
    Edit,
    /// 逆向きに再生する（映像だけ。J キー）。他のコマンドで止まる。
    Reverse,
    /// 直前に適用した編集を取り消す・やり直す。
    Undo,
    Redo,
    /// `Shared::core` の最新のプロジェクトのデータに置き換え、`Shared::reload_touched` のキャッシュを捨てる。
    Reload,
    Quit,
}

/// 開いたコンポジションの情報。
#[derive(Debug, Clone)]
pub struct PlaybackInfo {
    pub duration: f64,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    /// 時計の種類（`audio`、`wall`）。
    pub clock: &'static str,
    /// 音の出力デバイスの名前とレート。無ければ `None`。
    pub audio_device: Option<String>,
    pub backends: Vec<(String, String)>,
}

/// 再生スレッドのハンドル。落とすと止めて待つ。
pub struct Playback {
    tx: Sender<Command>,
    join: Option<JoinHandle<()>>,
}

impl Playback {
    /// 再生スレッドを始める。`Engine` の組み立てが失敗したらここで `Err`。
    pub fn spawn(
        shared: Arc<Shared>,
        project: Arc<Project>,
        composition: CompositionId,
        options: PlayerOptions,
    ) -> Result<(Self, PlaybackInfo)> {
        let (tx, rx) = channel();
        let (ready_tx, ready_rx) = channel();
        *shared.commands.lock().expect("commands") = Some(tx.clone());
        let join = std::thread::Builder::new()
            .name("playback".into())
            .spawn(
                move || match player::Player::new(shared, project, composition, options) {
                    Ok((mut player, info)) => {
                        let _ = ready_tx.send(Ok(info));
                        player.run(&rx);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                },
            )
            .map_err(MmError::Io)?;
        let info = ready_rx
            .recv()
            .map_err(|_| MmError::Internal("再生スレッドが立ち上がる前に終わった".into()))??;
        Ok((
            Self {
                tx,
                join: Some(join),
            },
            info,
        ))
    }
}

impl Drop for Playback {
    fn drop(&mut self) {
        let _ = self.tx.send(Command::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use mm_runtime::RuntimeConfig;
    use mm_testkit::decode::SampleMedia;
    use mm_testkit::fixtures::vertical_project;

    use super::*;

    fn start() -> (Arc<Shared>, Playback, PlaybackInfo) {
        let media = SampleMedia::default();
        let (project, comp, _) = vertical_project(&media, std::path::Path::new("out.json"));
        let shared = Arc::new(Shared::new());
        let options = PlayerOptions {
            no_audio: true,
            config: Some(RuntimeConfig::sample()),
            store_dir: None,
        };
        let (playback, info) = Playback::spawn(shared.clone(), Arc::new(project), comp, options)
            .expect("再生を始める");
        (shared, playback, info)
    }

    fn wait(limit: Duration, mut done: impl FnMut() -> bool) -> bool {
        let t = Instant::now();
        while t.elapsed() < limit {
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn 先頭から最後まで再生すると止まり_全フレームが出たかドロップしたかに数えられる() {
        let (shared, _playback, info) = start();
        let total = (info.duration * info.fps).round() as u64;
        shared.send(Command::Play);
        assert!(wait(Duration::from_secs(2), || shared
            .playing
            .load(Ordering::SeqCst)));
        assert!(
            wait(Duration::from_secs(10), || !shared
                .playing
                .load(Ordering::SeqCst)),
            "時間内に終わる"
        );
        let rendered = shared.rendered_frames.load(Ordering::SeqCst);
        let dropped = shared.dropped_frames.load(Ordering::SeqCst);
        assert!(
            rendered + dropped >= total,
            "描いた {rendered} とドロップした {dropped} で全 {total} フレームを数える"
        );
    }

    #[test]
    fn 操作を適用すると触ったレイヤーを返し_今のフレームを描き直す() {
        let (shared, _playback, _info) = start();
        assert!(wait(Duration::from_secs(2), || shared
            .frames
            .newer_than(0)
            .is_some()));
        let before = shared.frames.newer_than(0).expect("最初のフレーム").serial;
        let op: mm_ops::Op = mm_ops::SetParam {
            target: mm_ops::ParamRef::layer(&mm_types::Id::new("l-footage"), "opacity"),
            value: mm_types::ParamValue::Number(0.5),
            keyframes: None,
        }
        .into();
        let outcome = crate::win::edit::submit(&shared, vec![op]).expect("当たる");
        assert!(outcome.touched.iter().any(|id| id == "l-footage"));
        assert!(
            shared.frames.newer_than(before).is_some(),
            "適用した後のフレームが出る"
        );
        let bad: mm_ops::Op = mm_ops::SetParam {
            target: mm_ops::ParamRef::layer(&mm_types::Id::new("no-such"), "opacity"),
            value: mm_types::ParamValue::Number(0.5),
            keyframes: None,
        }
        .into();
        assert!(crate::win::edit::submit(&shared, vec![bad]).is_err());
    }

    #[test]
    fn 構造を変える操作の後は_公開するプロジェクトのデータが追いつき_コンポジションを消す操作は拒否する(
    ) {
        let (shared, _playback, _info) = start();
        assert!(wait(Duration::from_secs(2), || shared.doc().is_some()));
        let doc = shared.doc().expect("開いている");
        let layers = |s: &Shared| {
            s.doc()
                .expect("開いている")
                .project
                .composition(&doc.comp)
                .expect("comp")
                .layers
                .len()
        };
        let before = layers(&shared);
        let serial = shared.doc_serial.load(Ordering::SeqCst);

        let mut extra = doc.project.composition(&doc.comp).expect("comp").layers[0].clone();
        extra.id = mm_types::Id::new("l-extra");
        let add: mm_ops::Op = mm_ops::AddLayer {
            composition: doc.comp.clone(),
            layer: extra,
            index: None,
        }
        .into();
        crate::win::edit::submit(&shared, vec![add]).expect("足せる");
        assert_eq!(layers(&shared), before + 1, "レイヤーの一覧に出る");
        assert!(shared.doc_serial.load(Ordering::SeqCst) > serial);

        let remove: mm_ops::Op = mm_ops::RemoveLayer {
            layer: mm_types::Id::new("l-extra"),
        }
        .into();
        crate::win::edit::submit(&shared, vec![remove]).expect("外せる");
        assert_eq!(layers(&shared), before);

        let serial = shared.doc_serial.load(Ordering::SeqCst);
        let drop_open: mm_ops::Op = mm_ops::RemoveComposition {
            composition: doc.comp.clone(),
        }
        .into();
        let err = crate::win::edit::submit(&shared, vec![drop_open])
            .expect_err("開いているコンポジションは消せない");
        assert!(err.contains("コンポジション"), "{err}");
        assert_eq!(
            shared.doc_serial.load(Ordering::SeqCst),
            serial,
            "拒否したら公開しない"
        );
        assert_eq!(layers(&shared), before);
    }

    #[test]
    fn スクラブの大量のシークは最後の位置に落ち着く() {
        let (shared, _playback, info) = start();
        for i in 0..2000 {
            shared.send(Command::Seek(f64::from(i % 15) / 10.0));
        }
        shared.send(Command::Seek(1.0));
        assert!(
            wait(Duration::from_secs(5), || {
                shared
                    .frames
                    .newer_than(0)
                    .is_some_and(|f| (f.time - 1.0).abs() < 1.0 / info.fps)
            }),
            "最後のシークの位置のフレームが出る"
        );
        let rendered = shared.rendered_frames.load(Ordering::SeqCst);
        assert!(rendered < 1000, "古いシークを全部は描かない: {rendered}");
    }

    #[test]
    fn 停止中のシークとコマ送りはその位置のフレームを出す() {
        let (shared, _playback, info) = start();
        shared.send(Command::Seek(1.0));
        let at = |t: f64| {
            wait(Duration::from_secs(2), || {
                shared
                    .frames
                    .newer_than(0)
                    .is_some_and(|f| (f.time - t).abs() < 1e-9)
            })
        };
        assert!(at(1.0), "1 秒のフレーム");
        shared.send(Command::Step(1));
        assert!(at(1.0 + 1.0 / info.fps), "1 フレーム進む");
        shared.send(Command::Seek(f64::MAX));
        assert!(
            at(info.duration - 1.0 / info.fps),
            "終わりは最後のフレームに収まる"
        );
    }
}

#[cfg(test)]
mod burn_in_tests;
