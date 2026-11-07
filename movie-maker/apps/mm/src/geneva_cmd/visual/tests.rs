use super::*;
use serde_json::json;

fn media() -> HashMap<String, Media> {
    let m = |w, h, still| Media {
        size: Some([w, h]),
        duration: RationalTime::seconds(100),
        fps: None,
        still,
    };
    HashMap::from([
        ("png".to_owned(), m(400, 100, true)),
        ("cap".to_owned(), m(3840, 2160, false)),
    ])
}

fn one(clip: Value) -> (Layer, Report) {
    let media = media();
    let mut ctx = Ctx {
        fps: Rational::integer(30),
        frame: [1920.0, 1080.0],
        media: &media,
        report: Report::default(),
    };
    let mut layers = visual_layers(&mut ctx, &[json!({"clips": [clip]})]).unwrap();
    (layers.remove(0), ctx.report)
}

fn vec2(p: &Param) -> [f64; 2] {
    p.value.as_vec2().unwrap()
}

fn side(p: &Option<Param>) -> Option<f64> {
    p.as_ref().and_then(|p| p.value.as_number())
}

#[test]
fn 画像は左上のアンカーを位置に置き_素材の画素のまま() {
    let (l, _) = one(json!({
        "source": {"kind": "image", "asset": "png"}, "start": "30f", "duration": "10f",
        "transform": {"position": {"x": 1513, "y": 285}, "anchor": "0% 0%"}
    }));
    assert!(l.crop.is_empty());
    assert_eq!(vec2(&l.transform.position), [1513.0, 285.0]);
    assert_eq!(vec2(&l.transform.scale), [1.0, 1.0]);
    assert_eq!(l.start, RationalTime::seconds(1));
}

#[test]
fn 画面を覆う切り抜きも変形とレイヤーの切り抜きで表す() {
    let (l, _) = one(json!({
        "source": {"kind": "video", "asset": "cap", "in": 2.5, "audio": false},
        "start": "0f", "duration": "30f",
        "crop": {"x": 60, "y": 0, "width": 3720, "height": 2092}
    }));
    let k = 1920.0 / 3720.0;
    let s = vec2(&l.transform.scale);
    assert!((s[0] - k).abs() < 1e-12);
    // 切り抜きの左上（素材の 60, 0）が画面の左上に来る。縦は収めた矩形の余り（0.26 画素）の半分だけ下がる
    let p = vec2(&l.transform.position);
    let below = (1080.0 - 2092.0 * k) / 2.0;
    assert!((p[0] + 60.0 * k).abs() < 1e-9, "{p:?}");
    assert!((p[1] - below).abs() < 1e-9, "{p:?}");
    assert_eq!(side(&l.crop.left), Some(60.0));
    assert_eq!(side(&l.crop.right), Some(60.0));
    assert_eq!(side(&l.crop.top), None);
    assert_eq!(side(&l.crop.bottom), Some(68.0));
    assert!(l.audio.expect("映像の音").mute);
}

#[test]
fn 画面の中に縁が出る切り抜きはレイヤーの切り抜きで切る() {
    let (l, _) = one(json!({
        "source": {"kind": "video", "asset": "cap", "in": 1, "audio": false},
        "start": "0f", "duration": "30f", "fit": "none",
        "crop": {"x": 501, "y": 0, "width": 1259, "height": 1005},
        "transform": {"anchor": "0 0", "position": {"x": 1473, "y": 723}, "scale": 0.5}
    }));
    // 素材の (501, 0) が画面の (1473, 723) に来る
    let p = vec2(&l.transform.position);
    assert_eq!(p, [1473.0 - 0.5 * 501.0, 723.0]);
    assert_eq!(vec2(&l.transform.scale), [0.5, 0.5]);
    assert_eq!(side(&l.crop.left), Some(501.0));
    assert_eq!(side(&l.crop.right), Some(3840.0 - 501.0 - 1259.0));
    assert_eq!(side(&l.crop.bottom), Some(2160.0 - 1005.0));
}

#[test]
fn 角丸のマスクは切り抜きの矩形に重ねた素材の画素の矩形にする() {
    let (l, report) = one(json!({
        "source": {"kind": "video", "asset": "cap", "audio": false},
        "start": "0f", "duration": "30f", "fit": "none",
        "crop": {"x": 1220, "y": 1080, "width": 1530, "height": 861},
        "mask": {"radius": 55.25, "feather": 1},
        "transform": {"anchor": "0 0", "position": {"x": 48, "y": 104}, "scale": 0.47}
    }));
    assert!(
        report.to_json().as_array().unwrap().is_empty(),
        "マスクを除かない"
    );
    let rect = l.masks[0].rect.as_ref().unwrap();
    assert_eq!(vec2(&rect.position), [1220.0, 1080.0]);
    assert_eq!(vec2(&rect.size), [1530.0, 861.0]);
    assert_eq!(rect.radius.value.as_number(), Some(55.25));
    assert_eq!(l.masks[0].feather.value.as_number(), Some(1.0));
}

#[test]
fn 楕円のマスクは除いて数える() {
    let (l, report) = one(json!({
        "source": {"kind": "image", "asset": "png"}, "duration": "10f",
        "mask": {"shape": "ellipse"}
    }));
    assert!(l.masks.is_empty());
    assert_eq!(report.to_json()[0]["what"], "mask");
}
