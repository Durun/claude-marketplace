//! 音のトラック（`audio[].clips[]`）を、映像を持たない素材のレイヤーにする。音量は音量のキーフレーム、
//! フェードはレイヤーの音のフェード（振幅の直線。geneva のフェードと同じ曲線）にする。

use mm_doc::{Fade, Interpolation, Keyframe, Layer, LayerAudio, Node, Param};
use mm_types::{Id, MmError, ParamValue, RationalTime, Result};
use serde_json::Value;

use super::report::Gap;
use super::syntax::{self, Anim};
use super::visual::{animated, Ctx};

/// 全部の音のトラックをレイヤーにする。
pub fn audio_layers(ctx: &mut Ctx<'_>, tracks: &[Value]) -> Result<Vec<Layer>> {
    let mut out = Vec::new();
    for (ti, track) in tracks.iter().enumerate() {
        if track.get("enabled").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = track
            .get("id")
            .and_then(Value::as_str)
            .map_or_else(|| format!("audio{ti}"), str::to_owned);
        let clips = track
            .get("clips")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let mut prev_end = RationalTime::ZERO;
        for (ci, clip) in clips.iter().enumerate() {
            let layer = audio_clip(ctx, &name, &format!("{name}-{ci}"), clip, prev_end)?;
            prev_end = layer
                .start
                .checked_add(layer.out_point.checked_sub(layer.in_point)?)?;
            out.push(layer);
        }
    }
    Ok(out)
}

fn audio_clip(
    ctx: &mut Ctx<'_>,
    role: &str,
    id: &str,
    clip: &Value,
    prev_end: RationalTime,
) -> Result<Layer> {
    let asset = clip.get("asset").and_then(Value::as_str).unwrap_or("");
    let time_of = |k: &str| clip.get(k).map(|v| syntax::time(v, ctx.fps)).transpose();
    let in_point = time_of("in")?.unwrap_or(RationalTime::ZERO);
    let start = time_of("start")?.unwrap_or(prev_end);
    let duration = match (time_of("duration")?, time_of("out")?) {
        (Some(d), _) => d,
        (None, Some(out)) => out.checked_sub(in_point)?,
        (None, None) => rest_of(ctx, asset, in_point)?,
    };
    if clip
        .get("speed")
        .and_then(Value::as_f64)
        .is_some_and(|s| s != 1.0)
    {
        ctx.report.add(Gap::AudioSpeed);
    }
    let gain = match clip.get("gain_db") {
        Some(v) => syntax::anim(v, ctx.fps, &syntax::number)?,
        None => Anim::constant([0.0, 0.0]),
    };
    if gain.has_easing() {
        ctx.report.add(Gap::Easing);
    }
    let fade = |k: &str| -> Result<Option<Fade>> {
        Ok(time_of(k)?
            .filter(|t| *t > RationalTime::ZERO)
            .map(Fade::linear))
    };
    let node = Node::footage(&Id::new(asset));
    let mut layer = Layer::new(id, node, start, in_point, in_point.checked_add(duration)?);
    layer.audio = Some(LayerAudio {
        gain_db: gain_param(&gain, start, duration)?,
        fade_in: fade("fade_in")?,
        fade_out: fade("fade_out")?,
        ..LayerAudio::new(role)
    });
    Ok(layer)
}

fn rest_of(ctx: &Ctx<'_>, asset: &str, in_point: RationalTime) -> Result<RationalTime> {
    let media = ctx
        .media
        .get(asset)
        .ok_or_else(|| MmError::NotFound(format!("素材 {asset}")))?;
    media.duration.checked_sub(in_point)
}

/// 音量のパラメータ。geneva の音量のキーはクリップの始まりからの時刻なので、`start` だけずらす。
/// クリップの外のキーは捨て、クリップの両端にその時刻の値のキーを置いて、端の間の動きを保つ。
fn gain_param(gain: &Anim, start: RationalTime, duration: RationalTime) -> Result<Param> {
    if !gain.is_animated() {
        return Ok(Param::constant(ParamValue::Number(gain.keys[0].value[0])));
    }
    let mut times: Vec<RationalTime> = gain.keys.iter().map(|k| k.time).collect();
    times.extend([RationalTime::ZERO, duration]);
    times.retain(|t| *t >= RationalTime::ZERO && *t <= duration);
    times.sort();
    times.dedup();
    let mut keys = Vec::with_capacity(times.len());
    for t in times {
        keys.push(Keyframe::new(
            start.checked_add(t)?,
            ParamValue::Number(gain.at(t)[0]),
            Interpolation::Linear,
        ));
    }
    Ok(animated(keys))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_doc::FadeCurve;
    use mm_types::Rational;
    use serde_json::json;
    use std::collections::HashMap;

    use super::super::report::Report;

    fn secs(n: i128, d: u32) -> RationalTime {
        RationalTime::new(n, d).unwrap()
    }

    fn convert(clip: Value) -> Layer {
        let media = HashMap::new();
        let mut ctx = Ctx {
            fps: Rational::integer(30),
            frame: [1920.0, 1080.0],
            media: &media,
            report: Report::default(),
        };
        audio_clip(&mut ctx, "bgm", "bgm-0", &clip, RationalTime::ZERO).unwrap()
    }

    #[test]
    fn フェードは振幅の直線のフェードにする() {
        let l = convert(json!({
            "asset": "a", "start": "300f", "duration": "120f", "gain_db": -3,
            "fade_in": 0.5, "fade_out": 1.0
        }));
        let a = l.audio.unwrap();
        assert_eq!(a.gain_db.value.as_number(), Some(-3.0));
        assert!(a.gain_db.keyframes.is_empty(), "音量はキーフレームにしない");
        let fade_in = a.fade_in.unwrap();
        assert_eq!(fade_in.duration, secs(1, 2));
        assert_eq!(fade_in.curve, FadeCurve::Linear);
        assert_eq!(a.fade_out.unwrap().duration, secs(1, 1));
    }

    #[test]
    fn 長さ_0_のフェードは無いものにする() {
        let l = convert(json!({"asset": "a", "duration": "30f", "fade_in": 0.0}));
        assert!(l.audio.unwrap().fade_in.is_none());
    }

    #[test]
    fn 音量のキーはレイヤーの始まりだけずらす() {
        let gain = syntax::anim(
            &json!({"keyframes": [["0s", 0.0], ["1s", -12.0]]}),
            Rational::integer(30),
            &syntax::number,
        )
        .unwrap();
        let p = gain_param(&gain, secs(10, 1), secs(1, 1)).unwrap();
        let times: Vec<_> = p.keyframes.iter().map(|k| k.time).collect();
        assert_eq!(times, vec![secs(10, 1), secs(11, 1)]);
    }

    #[test]
    fn クリップの外のキーはクリップの端の値で残す() {
        let key = |t: i128, v: f64| syntax::Key {
            time: secs(t, 1),
            value: [v, v],
            eased: false,
        };
        let gain = Anim {
            keys: vec![key(-2, -20.0), key(8, 0.0)],
        };
        let p = gain_param(&gain, secs(10, 1), secs(4, 1)).unwrap();
        let got: Vec<(f64, f64)> = p
            .keyframes
            .iter()
            .map(|k| (k.time.to_f64(), k.value.as_number().unwrap()))
            .collect();
        assert_eq!(got, vec![(10.0, -16.0), (14.0, -8.0)]);
    }
}
