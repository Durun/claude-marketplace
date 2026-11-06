"""素材の音声を faster-whisper で文字起こしし、取り込みの付属ファイル（transcript）を書く。

付属ファイルの形は crates/mm-ingest-api/schema/sidecar.schema.json（kind = "transcript"）。
書き先は `<project>/cache/ingest/<コンテンツハッシュ>.transcript.json` で、`mm ingest` が次に読む。

    python -I transcribe.py <素材> --cache <project>/cache
    python -I transcribe.py <素材> --out out.json --content-hash <hex>
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from fractions import Fraction
from pathlib import Path
from typing import Any

SCHEMA_VERSION = 1
# 時刻を有理数にするときの分母（ミリ秒）。Whisper の時刻は 20 ms 刻みなので、丸めで変わらない。
TIME_DEN = 1000
# 素材のハッシュを読む 1 回の大きさ。
HASH_CHUNK = 1 << 20
# 既定のモデル。日本語は large-v3 が最も安定する。
DEFAULT_MODEL = "large-v3"
HASH_PATTERN = re.compile(r"[0-9a-f]{64}")
SCHEMA_RELATIVE = Path("crates/mm-ingest-api/schema/sidecar.schema.json")


def content_hash(path: Path) -> str:
    """素材の BLAKE3（16 進、64 文字）。Rust 側 `mm_ingest::hash_file` と同じ値になる。"""
    import blake3

    h = blake3.blake3()
    with path.open("rb") as f:
        while chunk := f.read(HASH_CHUNK):
            h.update(chunk)
    return h.hexdigest()


def rational_time(seconds: float) -> dict[str, int]:
    """秒を `{"num", "den"}`（既約）にする。"""
    t = Fraction(round(seconds * TIME_DEN), TIME_DEN)
    return {"num": t.numerator, "den": t.denominator}


def build_sidecar(
    hash_hex: str, producer: str, language: str, segments: list[dict[str, Any]]
) -> dict[str, Any]:
    """付属ファイル（transcript）の JSON。`segments` は start、end（秒）、text、words を持つ辞書の列。"""
    return {
        "schema_version": SCHEMA_VERSION,
        "content_hash": hash_hex,
        "producer": producer,
        "kind": "transcript",
        "language": language,
        "segments": [_segment_json(s) for s in segments],
    }


def _segment_json(seg: dict[str, Any]) -> dict[str, Any]:
    out: dict[str, Any] = {
        "start": rational_time(seg["start"]),
        "end": rational_time(seg["end"]),
        "text": seg["text"].strip(),
    }
    words = [
        {
            "start": rational_time(w["start"]),
            "end": rational_time(w["end"]),
            "text": w["text"].strip(),
        }
        for w in seg.get("words", [])
    ]
    if words:
        out["words"] = words
    return out


def find_schema() -> Path | None:
    """スクリプトの位置から上へたどって、凍結したスキーマを探す。"""
    for parent in Path(__file__).resolve().parents:
        candidate = parent / SCHEMA_RELATIVE
        if candidate.is_file():
            return candidate
    return None


def validate(sidecar: dict[str, Any], schema_path: Path) -> None:
    """スキーマに合わなければ例外。jsonschema が無ければスキップする。"""
    try:
        import jsonschema
    except ImportError:
        print("jsonschema が無いのでスキーマの検査をスキップする", file=sys.stderr)
        return
    schema = json.loads(schema_path.read_text(encoding="utf-8"))
    jsonschema.validate(sidecar, schema)


def add_nvidia_dll_dirs() -> None:
    """pip の nvidia-* パッケージの DLL を Windows の検索の道に足す（CTranslate2 が cuBLAS と cuDNN を探す）。"""
    if os.name != "nt":
        return
    import site

    roots = [Path(p) for p in site.getsitepackages()]
    for root in roots:
        for bin_dir in (root / "nvidia").glob("*/bin"):
            os.add_dll_directory(str(bin_dir))
            os.environ["PATH"] = str(bin_dir) + os.pathsep + os.environ.get("PATH", "")


def transcribe(media: Path, model_name: str, language: str | None, device: str, compute_type: str):
    """faster-whisper で文字起こしする。(言語, 区間の列, producer) を返す。"""
    add_nvidia_dll_dirs()
    import faster_whisper
    from faster_whisper import WhisperModel

    model = WhisperModel(model_name, device=device, compute_type=compute_type)
    it, info = model.transcribe(
        str(media), language=language, word_timestamps=True, vad_filter=True
    )
    segments = [
        {
            "start": s.start,
            "end": s.end,
            "text": s.text,
            "words": [{"start": w.start, "end": w.end, "text": w.word} for w in (s.words or [])],
        }
        for s in it
    ]
    return info.language, segments, f"faster-whisper {faster_whisper.__version__} ({model_name})"


def hash_arg(value: str) -> str:
    """`--content-hash` の値。書き先のファイル名に使うので、BLAKE3 の 16 進（64 文字）に限る。"""
    if not HASH_PATTERN.fullmatch(value):
        raise argparse.ArgumentTypeError("小文字の 16 進 64 文字（BLAKE3）でなければならない")
    return value


def parse_args(argv: list[str]) -> argparse.Namespace:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("media", type=Path, help="素材のパス（動画でも音声でもよい）")
    dest = p.add_mutually_exclusive_group(required=True)
    dest.add_argument("--cache", type=Path, help="<project>/cache。cache/ingest/<ハッシュ>.transcript.json に書く")
    dest.add_argument("--out", type=Path, help="書き先のファイル")
    p.add_argument("--content-hash", type=hash_arg, help="素材のコンテンツハッシュ。省くと BLAKE3 を計算する")
    p.add_argument("--model", default=DEFAULT_MODEL)
    p.add_argument("--language", help="言語コード（ja、en）。省くと自動で判定する")
    p.add_argument("--device", default="cuda", choices=["cuda", "cpu", "auto"])
    p.add_argument("--compute-type", default="float16", help="cuda は float16、cpu は int8 が速い")
    p.add_argument("--schema", type=Path, help="検査に使うスキーマ。省くとリポジトリの凍結したもの")
    return p.parse_args(argv)


def main(argv: list[str]) -> int:
    args = parse_args(argv)
    if not args.media.is_file():
        print(f"素材が無い: {args.media}", file=sys.stderr)
        return 2
    hash_hex = args.content_hash or content_hash(args.media)
    out = args.out or args.cache / "ingest" / f"{hash_hex}.transcript.json"
    language, segments, producer = transcribe(
        args.media, args.model, args.language, args.device, args.compute_type
    )
    sidecar = build_sidecar(hash_hex, producer, language, segments)
    schema = args.schema or find_schema()
    if schema is not None:
        validate(sidecar, schema)
    out.parent.mkdir(parents=True, exist_ok=True)
    tmp = out.with_name(out.name + ".part")
    tmp.write_text(json.dumps(sidecar, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    tmp.replace(out)
    print(f"{out}: {len(segments)} 区間（{language}）")
    return 0


if __name__ == "__main__":
    # Windows の標準出力は日本語の文字コードのことがあるので、UTF-8 に固定する。
    sys.stdout.reconfigure(encoding="utf-8")
    sys.exit(main(sys.argv[1:]))
