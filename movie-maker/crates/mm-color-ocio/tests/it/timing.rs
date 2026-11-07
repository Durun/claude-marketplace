//! 設定の読み込み、シェーダーの生成、naga の変換、LUT にベイクする時間を `mm-trace` のスパンで測る。
//! 表は `--nocapture` で出る。release で測ること（debug は桁が違う）。

use mm_color_api::ColorTransform;
use mm_color_ocio::OcioColor;
use mm_trace::MetricsLayer;
use mm_types::ColorSpace;
use tracing_subscriber::prelude::*;

const REPEAT: usize = 20;

#[test]
fn 変換にかかる時間をスパンで測る() {
    let (layer, handle) = MetricsLayer::new();
    let subscriber = tracing_subscriber::registry().with(layer);
    tracing::subscriber::with_default(subscriber, || {
        for _ in 0..REPEAT {
            // 毎回読み直して、起動時の設定の読み込みの分布を取る。
            let ocio = OcioColor::builtin().expect("設定");
            let (srgb, working) = (ColorSpace::srgb(), ColorSpace::working());
            ocio.program(&srgb, &working).expect("sRGB -> 作業色空間");
            ocio.program(&ColorSpace("ACEScg".into()), &working)
                .expect("ACEScg -> 作業色空間");
            ocio.display_program(&ColorSpace("ACEScg".into()), None, None)
                .expect("ACES の表示");
        }
    });
    let snap = handle.snapshot();
    eprintln!(
        "{:<26}{:>6}{:>10}{:>10}{:>10}",
        "span", "n", "p50 us", "p95 us", "max us"
    );
    for (name, s) in &snap.spans {
        eprintln!(
            "{name:<26}{:>6}{:>10}{:>10}{:>10}",
            s.count, s.p50_us, s.p95_us, s.max_us
        );
    }
    for name in [
        "color_ocio.load_config",
        "color_ocio.gen_glsl",
        "color_ocio.naga_convert",
        "color_ocio.bake_lut",
    ] {
        assert!(snap.spans.contains_key(name), "{name} のスパンが出ていない");
    }
}
