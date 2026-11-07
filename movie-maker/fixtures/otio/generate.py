"""Python の opentimelineio で fixtures/otio/python/*.otio を作り直す。

使い方: pip install opentimelineio && python fixtures/otio/generate.py
CI には opentimelineio を入れて往復の試験（crates/mm-doc/tests/otio_python.rs）を走らせるので、
ここで作ったファイルは opentimelineio を入れられない環境でも読みの試験が通るための凍結。
"""

import pathlib

import opentimelineio as otio

OUT = pathlib.Path(__file__).parent / "python"


def rt(value, rate):
    return otio.opentime.RationalTime(value, rate)


def tr(start, duration, rate):
    return otio.opentime.TimeRange(rt(start, rate), rt(duration, rate))


def clip(name, url, start, duration, rate, media_frames):
    ref = otio.schema.ExternalReference(
        target_url=url, available_range=tr(0, media_frames, rate)
    )
    return otio.schema.Clip(
        name=name, media_reference=ref, source_range=tr(start, duration, rate)
    )


def basic():
    tl = otio.schema.Timeline(name="basic")
    v1 = otio.schema.Track(name="V1", kind=otio.schema.TrackKind.Video)
    tl.tracks.append(v1)
    a = clip("a", "file:///D:/media/a%20b.mp4", 24, 48, 24, 240)
    a.markers.append(
        otio.schema.Marker(name="in clip", marked_range=tr(30, 0, 24), color="GREEN")
    )
    v1.append(a)
    v1.append(otio.schema.Gap(source_range=tr(0, 12, 24)))
    b = clip("b", "file:///D:/media/b.mp4", 0, 24, 24, 240)
    b.effects.append(otio.schema.LinearTimeWarp(time_scalar=2.0))
    v1.append(otio.schema.Transition(
        transition_type="SMPTE_Dissolve", in_offset=rt(6, 24), out_offset=rt(6, 24)))
    v1.append(b)
    v1.markers.append(otio.schema.Marker(name="chapter 1", marked_range=tr(0, 0, 24)))
    v1.markers.append(otio.schema.Marker(name="chapter 2", marked_range=tr(60, 0, 24)))
    otio.adapters.write_to_file(tl, str(OUT / "basic.otio"))


def ntsc():
    tl = otio.schema.Timeline(name="ntsc")
    v1 = otio.schema.Track(name="V1", kind=otio.schema.TrackKind.Video)
    tl.tracks.append(v1)
    rate = 30000 / 1001
    v1.append(clip("c1", "file:///D:/media/c.mov", 100, 90, rate, 900))
    v1.append(clip("c2", "file:///D:/media/c.mov", 400, 45, rate, 900))
    disabled = clip("c3", "file:///D:/media/d.mov", 0, 30, rate, 900)
    disabled.enabled = False
    v1.append(disabled)
    otio.adapters.write_to_file(tl, str(OUT / "ntsc.otio"))


def video_and_audio():
    tl = otio.schema.Timeline(name="va")
    v1 = otio.schema.Track(name="V1", kind=otio.schema.TrackKind.Video)
    v2 = otio.schema.Track(name="V2", kind=otio.schema.TrackKind.Video)
    a1 = otio.schema.Track(name="A1", kind=otio.schema.TrackKind.Audio)
    for t in (v1, v2, a1):
        tl.tracks.append(t)
    v1.append(clip("v1c", "file:///D:/media/v.mp4", 0, 48, 24, 480))
    v2.append(clip("v2c", "file:///D:/media/w.mp4", 0, 24, 24, 480))
    a1.append(clip("a1c", "file:///D:/media/v.wav", 0, 48, 24, 480))
    otio.adapters.write_to_file(tl, str(OUT / "video_and_audio.otio"))


if __name__ == "__main__":
    OUT.mkdir(exist_ok=True)
    basic()
    ntsc()
    video_and_audio()
