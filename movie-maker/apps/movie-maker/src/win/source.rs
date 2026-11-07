//! Source のビューアー。Media で選んだ素材 1 本を、素材だけを載せた別のコンポジションとして別の再生スレッドで再生する。
//!
//! Program の再生スレッドとは `Shared`（時計、コマンド、フレームのスロット）も `Engine` も別なので、
//! Program の再生位置や編集に触れずに、見る・シークする・イン点とアウト点を付ける。
//! フレームは CPU の画像で受ける（再生スレッドごとに GPU のデバイスが別で、サーフェスのデバイスのテクスチャとして使えないため）。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use mm_doc::{Asset, Composition, Layer, Node, Project};
use mm_types::{AssetId, CompositionId, Id, Rational, RationalTime};
use serde::Serialize;

use crate::win::playback::{Command, Playback, PlayerOptions};
use crate::win::state::Shared;

/// Source の素材だけを載せたコンポジションの ID。
const SOURCE_COMPOSITION: &str = "source";
/// その唯一のレイヤーの ID。
const SOURCE_LAYER: &str = "l-source";

/// 開いている素材と、その再生スレッド。落とすと再生スレッドを止めて待つ。
pub struct SourceHandle {
    /// Source の時計とフレーム。
    pub shared: Arc<Shared>,
    _playback: Playback,
    pub asset: AssetId,
    /// 素材のフレームレート。イン点・アウト点を付ける位置をフレームに丸めるのに使う。
    pub fps: Rational,
    pub duration: f64,
}

/// Source の置き場。`Shared` に 1 つ持つ。
#[derive(Default)]
pub struct SourceSlot {
    handle: Mutex<Option<Arc<SourceHandle>>>,
    /// 開く・閉じるたびに進む。描画スレッドがフレームの番号を数え直す判定に使う。
    pub serial: AtomicU64,
    /// キー（再生、シーク）が Source に向くか。Source の領域をクリックすると真、Program か Timeline で偽。
    active: AtomicBool,
    options: Mutex<Option<PlayerOptions>>,
}

impl SourceSlot {
    /// Source の再生スレッドの設定を置く。Program と同じ設定（音の有無、実装の組み合わせ）を使う。
    pub fn configure(&self, options: PlayerOptions) {
        *self.options.lock().expect("source options") = Some(options);
    }

    /// 開いている素材。
    pub fn handle(&self) -> Option<Arc<SourceHandle>> {
        self.handle.lock().expect("source handle").clone()
    }

    /// キーが Source に向くか。
    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::SeqCst)
    }

    /// キーを向ける先を切り替える。
    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::SeqCst);
    }

    /// 素材 `asset` を Source で開く。前の素材は閉じる。
    pub fn open(&self, project: &Project, comp: &Composition, asset: &str) -> Result<(), String> {
        let asset: AssetId = Id::new(asset);
        let found = project.asset(&asset).map_err(|e| e.to_string())?;
        let options = self
            .options
            .lock()
            .expect("source options")
            .clone()
            .unwrap_or_default();
        let (synthetic, fps) = source_project(found, comp)?;
        let shared = Arc::new(Shared::new());
        shared.cpu_frames.store(true, Ordering::SeqCst);
        let comp_id: CompositionId = Id::new(SOURCE_COMPOSITION);
        let (playback, _) = Playback::spawn(shared.clone(), Arc::new(synthetic), comp_id, options)
            .map_err(|e| e.to_string())?;
        let handle = Arc::new(SourceHandle {
            shared,
            _playback: playback,
            asset,
            fps,
            duration: found.duration.to_f64(),
        });
        // 古い素材の再生スレッドは、ロックの外で落とす（止めて待つため）。
        let old = self.handle.lock().expect("source handle").replace(handle);
        self.serial.fetch_add(1, Ordering::SeqCst);
        self.set_active(true);
        drop(old);
        Ok(())
    }

    /// Source を閉じる。
    pub fn close(&self) {
        let old = self.handle.lock().expect("source handle").take();
        self.serial.fetch_add(1, Ordering::SeqCst);
        self.set_active(false);
        drop(old);
    }

    /// Source の再生スレッドへコマンドを送る。素材が開いていなければ捨てる。
    pub fn send(&self, command: Command) {
        if let Some(h) = self.handle() {
            h.shared.send(command);
        }
    }
}

/// 素材 1 本を、素材の長さ全体を映すコンポジションにしたプロジェクト。大きさは Program のコンポジションに合わせる。
fn source_project(asset: &Asset, program: &Composition) -> Result<(Project, Rational), String> {
    let fps = asset.fps.unwrap_or(program.fps);
    let mut project = Project::new();
    project.assets.push(asset.clone());
    project.compositions.push(
        Composition::new(
            Id::new(SOURCE_COMPOSITION),
            program.width,
            program.height,
            fps,
            asset.duration,
        )
        .with_layers(vec![Layer::new(
            SOURCE_LAYER,
            Node::footage(&asset.id),
            RationalTime::ZERO,
            RationalTime::ZERO,
            asset.duration,
        )]),
    );
    project.validate().map_err(|e| e.to_string())?;
    Ok((project, fps))
}

/// WebView に出す Source の状態。
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SourceState {
    /// 開いている素材。
    pub asset: Option<String>,
    pub t: f64,
    pub playing: bool,
    pub duration: f64,
    pub fps: f64,
    pub mark_in: Option<f64>,
    pub mark_out: Option<f64>,
    /// 取り込み済みの素材（開く素材を選ぶ一覧）。
    pub assets: Vec<SourceAsset>,
    /// 直近の編集の失敗など。
    pub message: String,
}

/// 開ける素材。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SourceAsset {
    pub id: String,
    pub duration: f64,
}

/// いまの Source の状態。イン点・アウト点は開いているプロジェクトのデータ（`bins.marks`）から読む。
pub fn state_of(shared: &Shared) -> SourceState {
    let doc = shared.doc();
    let assets = doc
        .as_ref()
        .map(|d| {
            d.project
                .assets
                .iter()
                .map(|a| SourceAsset {
                    id: a.id.to_string(),
                    duration: a.duration.to_f64(),
                })
                .collect()
        })
        .unwrap_or_default();
    let mut state = SourceState {
        assets,
        message: shared.edit.message(),
        ..SourceState::default()
    };
    let Some(h) = shared.source.handle() else {
        return state;
    };
    state.asset = Some(h.asset.to_string());
    state.t = h.shared.playhead();
    state.playing = h.shared.playing.load(Ordering::SeqCst);
    state.duration = h.duration;
    state.fps = h.fps.to_f64();
    let marks = doc.and_then(|d| {
        d.project
            .bins
            .marks
            .iter()
            .find(|m| m.asset == h.asset)
            .cloned()
    });
    state.mark_in = marks
        .as_ref()
        .and_then(|m| m.in_point.map(RationalTime::to_f64));
    state.mark_out = marks.and_then(|m| m.out_point.map(RationalTime::to_f64));
    state
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::win::sequence_edit::tests::sample_project;

    #[test]
    fn 素材だけを載せたコンポジションは素材の長さ全体を映す() {
        let p = sample_project();
        let (src, fps) = source_project(&p.assets[0], &p.compositions[0]).unwrap();
        let comp = src.composition(&Id::new(SOURCE_COMPOSITION)).unwrap();
        assert_eq!(comp.duration, p.assets[0].duration);
        assert_eq!(comp.layers.len(), 1);
        assert_eq!(comp.layers[0].source.kind, "footage");
        assert_eq!(fps, Rational::integer(30));
        assert_eq!((comp.width, comp.height), (64, 36));
    }

    #[test]
    fn 開いていない_source_は空の状態を返しコマンドを捨てる() {
        let shared = Shared::new();
        let s = state_of(&shared);
        assert_eq!(s.asset, None);
        shared.source.send(Command::Play);
        assert!(!shared.source.is_active());
    }
}
