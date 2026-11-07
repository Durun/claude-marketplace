//! プレビューの字幕の焼き込み。書き出しと同じ関数の結果を描くことを、描くプロジェクトのデータで確かめる。

use std::sync::Arc;

use mm_doc::{
    Cut, Layer, Node, Project, Sequence, FOOTAGE_KIND, FOOTAGE_PARAM, SEQUENCE_KIND, SEQUENCE_PARAM,
};
use mm_runtime::RuntimeConfig;
use mm_store::{Store, TranscriptInput, WordInput};
use mm_testkit::decode::SampleMedia;
use mm_testkit::fixtures::vertical_project;
use mm_types::{CompositionId, Id, ParamValue, RationalTime};

use super::player::{Player, PlayerOptions};
use crate::win::state::Shared;

fn seconds(s: i64) -> RationalTime {
    RationalTime::seconds(s)
}

/// 素材 clip の 3〜5 秒を切る sequence を置いたコンポジションと、clip の文字起こしを持つ `project.mmdb`。
fn project_with_store(dir: &std::path::Path, burn_in: bool) -> (Project, CompositionId) {
    let (mut project, comp, _) = vertical_project(&SampleMedia::default(), &dir.join("a.json"));
    let clip = ParamValue::AssetRef(Id::new("clip"));
    let footage = Node::new(FOOTAGE_KIND, 1).with_param(FOOTAGE_PARAM, clip);
    project.sequences.push(Sequence {
        id: Id::new("s"),
        cuts: vec![Cut::new("k1", footage, seconds(3), seconds(5))],
        origin: None,
    });
    let node = Node::new(SEQUENCE_KIND, 1).with_param(SEQUENCE_PARAM, ParamValue::Text("s".into()));
    project.compositions[0].layers =
        vec![Layer::new("seq", node, seconds(0), seconds(0), seconds(2))];
    project.render_targets[0].delivery.burn_in_captions = burn_in;
    let mut store = Store::create(&dir.join("store"), &project, 0).unwrap();
    let words = [("こんにちは。", 3), ("世界。", 4)].map(|(text, at)| WordInput {
        segment: 0,
        start: seconds(at),
        end: RationalTime::new(i128::from(at) * 1000 + 800, 1000).unwrap(),
        text: text.into(),
    });
    let input = TranscriptInput {
        producer: "test".into(),
        language: "ja".into(),
        words: words.to_vec(),
    };
    let hash = project.assets[0].content_hash.clone();
    store
        .put_transcript(&Id::new("clip"), &hash, &input)
        .unwrap();
    (project, comp)
}

fn player(project: &Project, comp: CompositionId, dir: &std::path::Path) -> Player {
    let options = PlayerOptions {
        no_audio: true,
        config: Some(RuntimeConfig::sample()),
        store_dir: Some(dir.join("store")),
    };
    Player::new(
        Arc::new(Shared::new()),
        Arc::new(project.clone()),
        comp,
        options,
    )
    .expect("再生を始める")
    .0
}

fn text_layers(project: &Project, comp: &CompositionId) -> usize {
    let layers = &project.composition(comp).unwrap().layers;
    layers.iter().filter(|l| l.source.kind == "text").count()
}

#[test]
fn 焼き込むレンダーターゲットがあるとプレビューは書き出しと同じ字幕のレイヤーを描く() {
    let dir = std::env::temp_dir().join(format!("mm_preview_burn_in_{}", std::process::id()));
    let (project, comp) = project_with_store(&dir, true);
    let mut player = player(&project, comp.clone(), &dir);
    let shown = player.shown_project();
    assert!(text_layers(&shown, &comp) >= 2, "字幕の影と本体が足される");
    let words = crate::win::deliver::transcript_words(Some(&dir.join("store")), &project);
    let engine = mm_runtime::assemble(&RuntimeConfig::sample()).unwrap();
    let exported = mm_export::burn_in_captions(
        &project,
        &comp,
        &words,
        engine.services().text_shaper.as_ref(),
    )
    .unwrap();
    assert_eq!(*shown, exported, "書き出しの描くプロジェクトのデータと同じ");
    assert!(
        Arc::ptr_eq(&shown, &player.shown_project()),
        "元が同じ間は作り直さない"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn 焼き込まない設定のプレビューは字幕を足さない() {
    let dir = std::env::temp_dir().join(format!("mm_preview_plain_{}", std::process::id()));
    let (project, comp) = project_with_store(&dir, false);
    let mut player = player(&project, comp.clone(), &dir);
    assert_eq!(*player.shown_project(), project);
    let _ = std::fs::remove_dir_all(&dir);
}
