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

fn one(clip: Value) -> (Layer, Vec<Composition>) {
    let media = media();
    let mut ctx = Ctx {
        fps: Rational::integer(30),
        frame: [1920.0, 1080.0],
        media: &media,
        report: Report::default(),
        nested: Vec::new(),
    };
    let mut layers = visual_layers(&mut ctx, &[json!({"clips": [clip]})]).unwrap();
    (layers.remove(0), ctx.nested)
}

fn vec2(p: &Param) -> [f64; 2] {
    p.value.as_vec2().unwrap()
}

#[test]
fn 画像は左上のアンカーを位置に置き_素材の画素のまま() {
    let (l, nested) = one(json!({
        "source": {"kind": "image", "asset": "png"}, "start": "30f", "duration": "10f",
        "transform": {"position": {"x": 1513, "y": 285}, "anchor": "0% 0%"}
    }));
    assert!(nested.is_empty());
    assert_eq!(vec2(&l.transform.position), [1513.0, 285.0]);
    assert_eq!(vec2(&l.transform.scale), [1.0, 1.0]);
    assert_eq!(l.start, RationalTime::seconds(1));
}

#[test]
fn 画面を覆う切り抜きは変形だけで表す() {
    let (l, nested) = one(json!({
        "source": {"kind": "video", "asset": "cap", "in": 2.5, "audio": false},
        "start": "0f", "duration": "30f",
        "crop": {"x": 60, "y": 0, "width": 3720, "height": 2092}
    }));
    assert!(nested.is_empty());
    let k = 1920.0 / 3720.0;
    let s = vec2(&l.transform.scale);
    assert!((s[0] - k).abs() < 1e-12);
    // 切り抜きの左上（素材の 60, 0）が画面の左上に来る。縦は収めた矩形の余り（0.26 画素）の半分だけ下がる
    let p = vec2(&l.transform.position);
    let below = (1080.0 - 2092.0 * k) / 2.0;
    assert!((p[0] + 60.0 * k).abs() < 1e-9, "{p:?}");
    assert!((p[1] - below).abs() < 1e-9, "{p:?}");
    assert!(l.audio.expect("映像の音").mute);
}

#[test]
fn 画面の中に縁が出る切り抜きは入れ子で切る() {
    let (l, nested) = one(json!({
        "source": {"kind": "video", "asset": "cap", "in": 1, "audio": false},
        "start": "0f", "duration": "30f", "fit": "none",
        "crop": {"x": 501, "y": 0, "width": 1259, "height": 1005},
        "transform": {"anchor": "0 0", "position": {"x": 1473, "y": 723}, "scale": 0.5}
    }));
    assert_eq!(nested.len(), 1);
    assert_eq!((nested[0].width, nested[0].height), (1259, 1005));
    let inner = &nested[0].layers[0];
    assert_eq!(vec2(&inner.transform.position), [-501.0, 0.0]);
    assert_eq!(vec2(&l.transform.position), [1473.0, 723.0]);
    assert_eq!(vec2(&l.transform.scale), [0.5, 0.5]);
}
