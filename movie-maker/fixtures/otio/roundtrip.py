"""opentimelineio で .otio を読み、中身を JSON で報告し、書き直す。

使い方: python -I roundtrip.py <入力.otio> <書き直す先.otio>
標準出力に 1 行の JSON（トラックの本数、項目の種類、マーカーの数、メタデータの movie-maker が残っているか）を出す。
crates/mm-doc/tests/otio/python.rs が、Rust の書いたファイルを opentimelineio が読めること、
opentimelineio が書き直したファイルを Rust が同じに読み戻せることを確かめるのに使う。
"""

import json
import sys

import opentimelineio as otio


def main():
    src, dst = sys.argv[1], sys.argv[2]
    tl = otio.adapters.read_from_file(src)
    video = [t for t in tl.tracks if t.kind == otio.schema.TrackKind.Video]
    track = video[0]
    items = [type(c).__name__ for c in track]
    clips = [c for c in track if isinstance(c, otio.schema.Clip)]
    report = {
        "video_tracks": len(video),
        "items": items,
        "markers": len(track.markers),
        "duration_seconds": track.duration().to_seconds(),
        "clip_names": [c.name for c in clips],
        "meta_kept": all("movie-maker" in c.metadata for c in track if not isinstance(c, otio.schema.Transition)),
        "track_meta_kept": "movie-maker" in track.metadata,
    }
    otio.adapters.write_to_file(tl, dst)
    print(json.dumps(report))


main()
