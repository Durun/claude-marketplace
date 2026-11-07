//! Mixer パネルの中身。開いたコンポジションのトラックとミキサーの設定（`mixer_state`）と、再生中のメーター。
//!
//! メーターは再生スレッドが音を先読みで混ぜるたびに 1 区間ずつ記録し、同期のスレッドが鳴っている位置の区間を選んで送る。
//! 先読みの分（0.4 秒）だけ先の値を出さないよう、区間を再生位置で引く。

use std::collections::VecDeque;

use mm_audio_api::{AudioBlock, MasterMeter, MasterReading, MixerFactory};
use mm_doc::{
    Asset, CompMixer, Layer, LayerAudio, Project, FOOTAGE_KIND, FOOTAGE_PARAM, NESTED_KIND,
    NESTED_PARAM, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_eval::DEFAULT_AUDIO_ROLE;
use mm_runtime::TrackLevel;
use serde::Serialize;

use crate::win::state::OpenDoc;

/// バスの選択肢に必ず出す役割。字幕動画の声、BGM、効果音。
const DEFAULT_ROLES: [&str; 3] = [DEFAULT_AUDIO_ROLE, "bgm", "sfx"];
/// 記録しておくメーターの区間の上限。先読みの 0.4 秒を 0.1 秒の区間で持てば足り、余りは捨てる。
const MAX_FRAMES: usize = 32;

/// Mixer のトラック 1 本（`ui/sdk` の `MixerTrack`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixerTrack {
    pub layer: String,
    pub kind: String,
    pub label: String,
    pub audio: Option<LayerAudio>,
    pub animated: bool,
}

/// Mixer を開いたときの中身（`ui/sdk` の `MixerState`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixerState {
    pub composition: String,
    pub mixer: Option<CompMixer>,
    pub tracks: Vec<MixerTrack>,
    pub roles: Vec<String>,
}

/// 開いたコンポジションの Mixer の中身。音を持てるレイヤーだけを、上のレイヤーから並べる。
pub fn state_of(doc: &OpenDoc) -> Result<MixerState, String> {
    let comp = doc
        .project
        .composition(&doc.comp)
        .map_err(|e| e.to_string())?;
    let tracks: Vec<MixerTrack> = comp
        .layers
        .iter()
        .rev()
        .filter(|l| [FOOTAGE_KIND, NESTED_KIND, SEQUENCE_KIND].contains(&l.source.kind.as_str()))
        .map(|l| track_of(&doc.project, l))
        .collect();
    let mut roles: Vec<String> = DEFAULT_ROLES.iter().map(|r| (*r).to_owned()).collect();
    let used = tracks
        .iter()
        .filter_map(|t| t.audio.as_ref().map(|a| &a.role));
    let buses = comp.mixer.iter().flat_map(|m| m.buses.keys());
    for role in used.chain(buses) {
        if !roles.contains(role) {
            roles.push(role.clone());
        }
    }
    Ok(MixerState {
        composition: comp.id.to_string(),
        mixer: comp.mixer.clone(),
        tracks,
        roles,
    })
}

fn track_of(project: &Project, layer: &Layer) -> MixerTrack {
    let animated = layer
        .audio
        .as_ref()
        .is_some_and(|a| !a.gain_db.keyframes.is_empty() || !a.pan.keyframes.is_empty());
    MixerTrack {
        layer: layer.id.to_string(),
        kind: layer.source.kind.clone(),
        label: label_of(project, layer),
        audio: layer.audio.clone(),
        animated,
    }
}

/// 表示の名前。撮影素材は素材のファイル名、入れ子と sequence は指す先の ID、どれでもなければレイヤーの ID。
fn label_of(project: &Project, layer: &Layer) -> String {
    let param = match layer.source.kind.as_str() {
        FOOTAGE_KIND => FOOTAGE_PARAM,
        NESTED_KIND => NESTED_PARAM,
        _ => SEQUENCE_PARAM,
    };
    let value = layer.source.params.get(param).map(|p| &p.value);
    let name = match value {
        Some(mm_types::ParamValue::AssetRef(id)) => project.asset(id).ok().map(file_name),
        Some(mm_types::ParamValue::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    };
    name.unwrap_or_else(|| layer.id.to_string())
}

fn file_name(asset: &Asset) -> String {
    let path = asset.path.replace('\\', "/");
    path.rsplit('/').next().unwrap_or(&path).to_owned()
}

/// トラック 1 本のメーター（`ui/sdk` の `TrackMeter`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TrackMeter {
    pub layer: String,
    pub peak_db: Option<f64>,
}

/// マスターのメーター（`ui/sdk` の `MasterMeter`）。
#[derive(Debug, Clone, PartialEq, Default, Serialize)]
pub struct MasterMeterView {
    pub peak_db: Vec<Option<f64>>,
    pub true_peak_db: Option<f64>,
    pub short_term_lufs: Option<f64>,
    pub integrated_lufs: Option<f64>,
}

/// 再生中のメーターの値（イベント `mm:mixer-meters`、`ui/sdk` の `MixerMeters`）。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MixerMeters {
    pub t: f64,
    pub playing: bool,
    pub tracks: Vec<TrackMeter>,
    pub master: MasterMeterView,
}

/// 線形の振幅を dBFS に。無音は `None`（JSON に -inf を書けない）。
fn to_db(amplitude: f32) -> Option<f64> {
    (amplitude > 0.0).then(|| 20.0 * f64::from(amplitude).log10())
}

/// 混ぜた 1 区間のメーターの値。
#[derive(Debug, Clone)]
struct MeterFrame {
    start: f64,
    tracks: Vec<TrackMeter>,
    master: MasterMeterView,
}

/// 再生スレッドが記録し、同期のスレッドが読むメーターの区間の並び。
#[derive(Default)]
pub struct MeterLane {
    meter: Option<Box<dyn MasterMeter>>,
    frames: VecDeque<MeterFrame>,
}

impl MeterLane {
    /// シークしたとき。区間を捨て、統合ラウドネスとトゥルーピークを最初から測る。
    pub fn reset(&mut self) {
        self.frames.clear();
        if let Some(m) = self.meter.as_mut() {
            m.reset();
        }
    }

    /// `start` 秒から鳴る区間 `block` と、それを混ぜたときのトラックの値を記録する。
    pub fn record(
        &mut self,
        factory: &dyn MixerFactory,
        start: f64,
        block: &AudioBlock,
        levels: Vec<TrackLevel>,
    ) {
        if self.meter.is_none() {
            self.meter = factory.master_meter(block.sample_rate, block.channels);
        }
        let reading = self
            .meter
            .as_mut()
            .and_then(|m| m.feed(block).ok())
            .unwrap_or_default();
        let tracks = levels
            .into_iter()
            .map(|l| TrackMeter {
                layer: l.layer.to_string(),
                peak_db: to_db(l.peak),
            })
            .collect();
        self.frames.push_back(MeterFrame {
            start,
            tracks,
            master: master_view(&reading),
        });
        while self.frames.len() > MAX_FRAMES {
            self.frames.pop_front();
        }
    }

    /// 再生位置 `t` で鳴っている区間の値。過ぎた区間は捨てる。まだ鳴っていなければ `None`。
    pub fn at(&mut self, t: f64) -> Option<(Vec<TrackMeter>, MasterMeterView)> {
        while self.frames.get(1).is_some_and(|f| f.start <= t) {
            self.frames.pop_front();
        }
        let f = self.frames.front().filter(|f| f.start <= t)?;
        Some((f.tracks.clone(), f.master.clone()))
    }
}

fn master_view(r: &MasterReading) -> MasterMeterView {
    MasterMeterView {
        peak_db: r.peak.iter().map(|p| to_db(*p)).collect(),
        true_peak_db: r.true_peak.and_then(to_db),
        short_term_lufs: r.short_term_lufs.map(f64::from),
        integrated_lufs: r.integrated_lufs.map(f64::from),
    }
}

#[cfg(test)]
mod tests;
