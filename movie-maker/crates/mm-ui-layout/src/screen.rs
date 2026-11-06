//! サーフェス全体の割り付け。上に Program、下に Timeline。

use crate::geom::Rect;
use crate::timeline::RULER_H;

/// Program と Timeline の間隔。
const GAP: f32 = 4.0;
/// Timeline を全体表示にしたときの左右の余白（px）。
pub const VIEW_MARGIN: f32 = 16.0;
/// Timeline の高さの下限（目盛りとレイヤー 2 本）。
const MIN_TIMELINE_H: f32 = RULER_H + 60.0;
/// Timeline が占める高さの上限（サーフェスに対する比）。
const MAX_TIMELINE_RATIO: f32 = 0.5;

/// サーフェスの割り付け。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceLayout {
    pub program: Rect,
    pub timeline: Rect,
    /// Source のビューアー。素材を開いているときだけ、Program の左に並ぶ。
    pub source: Option<Rect>,
}

/// 幅 `w`、高さ `h` のサーフェスを割る。Timeline は中身の高さ `timeline_content_h` に合わせ、上限は半分。
pub fn split_surface(w: f32, h: f32, timeline_content_h: f32) -> SurfaceLayout {
    split_viewers(w, h, timeline_content_h, false)
}

/// [`split_surface`] に加えて、`with_source` なら上の段を左の Source と右の Program に半分ずつ割る。
pub fn split_viewers(w: f32, h: f32, timeline_content_h: f32, with_source: bool) -> SurfaceLayout {
    let max_h = (h * MAX_TIMELINE_RATIO).max(0.0);
    let th = (timeline_content_h + GAP).max(MIN_TIMELINE_H).min(max_h);
    let ph = (h - th - GAP).max(0.0);
    let timeline = Rect::new(0.0, ph + GAP, w, th);
    if !with_source {
        return SurfaceLayout {
            program: Rect::new(0.0, 0.0, w, ph),
            timeline,
            source: None,
        };
    }
    let half = ((w - GAP) / 2.0).max(0.0);
    SurfaceLayout {
        source: Some(Rect::new(0.0, 0.0, half, ph)),
        program: Rect::new(half + GAP, 0.0, half, ph),
        timeline,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 二つの領域はサーフェスを隙間つきで埋める() {
        let l = split_surface(1000.0, 800.0, 100.0);
        assert_eq!(l.program.bottom() + GAP, l.timeline.y);
        assert_eq!(l.timeline.bottom(), 800.0);
        assert_eq!((l.program.w, l.timeline.w), (1000.0, 1000.0));
    }

    #[test]
    fn source_を開くと上の段を左右に半分ずつ割り_timeline_は変わらない() {
        let plain = split_surface(1000.0, 800.0, 100.0);
        let l = split_viewers(1000.0, 800.0, 100.0, true);
        let source = l.source.expect("Source");
        assert_eq!(source.right() + GAP, l.program.x);
        assert_eq!(l.program.right(), 1000.0);
        assert_eq!(l.timeline, plain.timeline);
        assert_eq!((source.h, l.program.h), (plain.program.h, plain.program.h));
        assert_eq!(plain.source, None);
    }

    #[test]
    fn timeline_は半分を超えない() {
        let l = split_surface(1000.0, 800.0, 5000.0);
        assert_eq!(l.timeline.h, 400.0);
    }

    #[test]
    fn 小さなサーフェスでも矩形が負にならない() {
        let l = split_surface(10.0, 20.0, 100.0);
        assert!(l.program.h >= 0.0 && l.timeline.h >= 0.0);
    }
}
