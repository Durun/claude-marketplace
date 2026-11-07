//! 音のトラック（`audio[].clips[]`）を、映像を持たない素材のレイヤーにする。音量とフェードは音量のキーフレームにする。

use mm_doc::{Interpolation, Keyframe, Layer, LayerAudio, Node, Param};
use mm_types::{Id, MmError, ParamValue, Rational, RationalTime, Result};
use serde_json::Value;

use super::report::Gap;
use super::syntax::{self, Anim};
use super::visual::{animated, Ctx};

/// フェードの端の振幅。movie-maker の音量は dB なので、無音の代わりに -60 dB に置く。
const FADE_FLOOR: f64 = 1e-3;
/// フェードを近似する区間の数。geneva のフェードは振幅の直線で、movie-maker の音量は dB の直線なので、
/// 振幅の直線を折れ線の dB で近似する。
const FADE_SEGMENTS: u32 = 8;

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
    let fades = Fades {
        fade_in: time_of("fade_in")?.filter(|t| *t > RationalTime::ZERO),
        fade_out: time_of("fade_out")?.filter(|t| *t > RationalTime::ZERO),
        duration,
    };
    let node = Node::footage(&Id::new(asset));
    let mut layer = Layer::new(id, node, start, in_point, in_point.checked_add(duration)?);
    layer.audio = Some(LayerAudio {
        role: role.to_owned(),
        gain_db: gain_param(&gain, &fades, start)?,
        pan: Param::constant(ParamValue::Number(0.0)),
        mute: false,
        solo: false,
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

/// クリップのフェード（時刻はクリップの始まりから）。
struct Fades {
    fade_in: Option<RationalTime>,
    fade_out: Option<RationalTime>,
    duration: RationalTime,
}

impl Fades {
    fn is_empty(&self) -> bool {
        self.fade_in.is_none() && self.fade_out.is_none()
    }

    /// 時刻 `t` の振幅（0..1）。
    fn amplitude(&self, t: RationalTime) -> f64 {
        let t = t.to_f64();
        let rise = self.fade_in.map_or(1.0, |f| t / f.to_f64());
        let fall = self
            .fade_out
            .map_or(1.0, |f| (self.duration.to_f64() - t) / f.to_f64());
        rise.min(fall).clamp(0.0, 1.0)
    }

    /// フェードを折れ線にする時刻。
    fn times(&self) -> Result<Vec<RationalTime>> {
        let mut out = Vec::new();
        let steps =
            |from: RationalTime, len: RationalTime, out: &mut Vec<RationalTime>| -> Result<()> {
                out.push(from);
                for k in 1..=FADE_SEGMENTS {
                    out.push(from.checked_add(len.scale(Rational::new(k, FADE_SEGMENTS)?)?)?);
                }
                Ok(())
            };
        if let Some(f) = self.fade_in {
            steps(RationalTime::ZERO, f, &mut out)?;
        }
        if let Some(f) = self.fade_out {
            steps(self.duration.checked_sub(f)?, f, &mut out)?;
        }
        Ok(out)
    }
}

/// 音量のパラメータ。音量のキーとフェードの折れ線の時刻に、両方を足した dB を置く。
/// クリップの外のキーは捨て、クリップの両端にその時刻の値のキーを置いて、端の間の動きを保つ。
fn gain_param(gain: &Anim, fades: &Fades, start: RationalTime) -> Result<Param> {
    if !gain.is_animated() && fades.is_empty() {
        return Ok(Param::constant(ParamValue::Number(gain.keys[0].value[0])));
    }
    let mut times: Vec<RationalTime> = gain.keys.iter().map(|k| k.time).collect();
    times.extend(fades.times()?);
    times.extend([RationalTime::ZERO, fades.duration]);
    times.retain(|t| *t >= RationalTime::ZERO && *t <= fades.duration);
    times.sort();
    times.dedup();
    let mut keys = Vec::with_capacity(times.len());
    for t in times {
        let db = gain.at(t)[0] + 20.0 * fades.amplitude(t).max(FADE_FLOOR).log10();
        keys.push(Keyframe::new(
            start.checked_add(t)?,
            ParamValue::Number(db),
            Interpolation::Linear,
        ));
    }
    Ok(animated(keys))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(n: i128, d: u32) -> RationalTime {
        RationalTime::new(n, d).unwrap()
    }

    #[test]
    fn フェードは振幅の直線を_db_の折れ線で近似する() {
        let gain = Anim::constant([-3.0, -3.0]);
        let fades = Fades {
            fade_in: Some(secs(1, 2)),
            fade_out: Some(secs(1, 1)),
            duration: secs(4, 1),
        };
        let p = gain_param(&gain, &fades, secs(10, 1)).unwrap();
        let at = |t: f64| {
            p.keyframes
                .iter()
                .find(|k| (k.time.to_f64() - t).abs() < 1e-9)
                .and_then(|k| k.value.as_number())
                .unwrap()
        };
        assert!((at(10.0) - (-63.0)).abs() < 1e-9, "先頭は -60 dB の下限");
        assert!(
            (at(10.25) - (-3.0 + 20.0 * 0.5f64.log10())).abs() < 1e-9,
            "フェードの中ほどは振幅が半分"
        );
        assert!(
            (at(10.5) - (-3.0)).abs() < 1e-9,
            "フェードの後はクリップの音量"
        );
        assert!((at(13.0) - (-3.0)).abs() < 1e-9, "フェードアウトの前");
        assert!((at(14.0) - (-63.0)).abs() < 1e-9, "終わりは下限");
        assert_eq!(p.keyframes.len(), 2 * (FADE_SEGMENTS as usize + 1));
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
        let fades = Fades {
            fade_in: None,
            fade_out: None,
            duration: secs(4, 1),
        };
        let p = gain_param(&gain, &fades, secs(10, 1)).unwrap();
        let got: Vec<(f64, f64)> = p
            .keyframes
            .iter()
            .map(|k| (k.time.to_f64(), k.value.as_number().unwrap()))
            .collect();
        assert_eq!(got, vec![(10.0, -16.0), (14.0, -8.0)]);
    }

    #[test]
    fn フェードが無く音量が固定なら固定の値() {
        let gain = Anim::constant([1.5, 1.5]);
        let fades = Fades {
            fade_in: None,
            fade_out: None,
            duration: secs(1, 1),
        };
        let p = gain_param(&gain, &fades, secs(0, 1)).unwrap();
        assert!(p.keyframes.is_empty());
        assert_eq!(p.value.as_number(), Some(1.5));
    }
}
