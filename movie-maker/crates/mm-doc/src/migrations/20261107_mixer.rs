//! ミキサー。コンポジションのバス・マスター・ダッキング（`Composition.mixer`）と、レイヤーの音のミュートとソロ
//! （`LayerAudio.mute`、`solo`）。

use serde_json::Value;

/// 足したフィールドは serde の既定（ミキサーなし、ミュートもソロもなし）で埋まるので、JSON は書き換えない。
pub(crate) fn up(_value: &mut Value) {}

#[cfg(test)]
mod tests {
    use crate::load;

    fn read(path: &str) -> crate::Project {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/doc");
        load(&std::fs::read_to_string(dir.join(path)).unwrap()).unwrap()
    }

    #[test]
    fn 凍結したミキサーとミュートとソロが読めて往復する() {
        let p = read("20261107_mixer/mixer.json");
        let comp = &p.compositions[0];
        let mixer = comp.mixer.as_ref().unwrap();
        assert_eq!(mixer.master_db, -1.5);
        assert_eq!(mixer.buses["bgm"].gain_db, -3.0);
        assert_eq!(mixer.ducking, Some(crate::Ducking::voice_over_bgm()));
        let audio = |i: usize| comp.layers[i].audio.as_ref().unwrap();
        assert!(audio(0).mute && !audio(0).solo);
        assert!(audio(1).solo && !audio(1).mute);
        assert!(p.migrations.iter().any(|m| m == "20261107_mixer"));
        assert_eq!(load(&serde_json::to_string(&p).unwrap()).unwrap(), p);
    }

    #[test]
    fn 移行の前のプロジェクトのデータはミキサーなしで読める() {
        let p = read("v9/burnin.json");
        assert!(p.compositions.iter().all(|c| c.mixer.is_none()));
        assert!(p.migrations.iter().any(|m| m == "20261107_mixer"));
    }
}
