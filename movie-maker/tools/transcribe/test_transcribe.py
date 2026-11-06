"""transcribe.py の試験。モデルは使わず、付属ファイルがスキーマに合うことと、時刻と ID の変換を確かめる。

    python -I -m unittest discover          （tools/transcribe/ で）
    MM_TRANSCRIBE_E2E=1 python -I -m unittest discover   モデルを使う通しの試験も回す
"""

import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

import jsonschema

import transcribe

HERE = Path(__file__).resolve().parent
SAMPLE_WAV = HERE.parent.parent / "fixtures" / "ingest" / "out" / "speech-ja.wav"


class RationalTimeTest(unittest.TestCase):
    def test_秒は既約の有理数になる(self):
        self.assertEqual(transcribe.rational_time(0.5), {"num": 1, "den": 2})
        self.assertEqual(transcribe.rational_time(0), {"num": 0, "den": 1})
        self.assertEqual(transcribe.rational_time(4.98), {"num": 249, "den": 50})

    def test_ミリ秒に丸める(self):
        self.assertEqual(transcribe.rational_time(1.0004), {"num": 1, "den": 1})


class SidecarTest(unittest.TestCase):
    schema = json.loads(transcribe.find_schema().read_text(encoding="utf-8"))

    def sidecar(self, segments):
        return transcribe.build_sidecar("ab" * 32, "test 1.0", "ja", segments)

    def test_スキーマに合う(self):
        seg = {
            "start": 0.0,
            "end": 2.5,
            "text": " こんにちは ",
            "words": [{"start": 0.0, "end": 1.0, "text": " こんにちは"}],
        }
        s = self.sidecar([seg])
        jsonschema.validate(s, self.schema)
        self.assertEqual(s["segments"][0]["text"], "こんにちは")
        self.assertEqual(s["segments"][0]["words"][0]["text"], "こんにちは")

    def test_単語が無ければ_words_を書かない(self):
        s = self.sidecar([{"start": 0, "end": 1, "text": "a", "words": []}])
        self.assertNotIn("words", s["segments"][0])
        jsonschema.validate(s, self.schema)

    def test_種類が違えばスキーマが拒否する(self):
        s = self.sidecar([])
        s["kind"] = "unknown"
        with self.assertRaises(jsonschema.ValidationError):
            jsonschema.validate(s, self.schema)

    def test_区間の時刻が欠けていれば拒否する(self):
        s = self.sidecar([{"start": 0, "end": 1, "text": "a"}])
        del s["segments"][0]["end"]
        with self.assertRaises(jsonschema.ValidationError):
            jsonschema.validate(s, self.schema)


class ContentHashTest(unittest.TestCase):
    def test_rust_側と同じ_blake3(self):
        with tempfile.TemporaryDirectory() as d:
            p = Path(d) / "a.bin"
            p.write_bytes(b"hello")
            # blake3("hello")。mm-ingest の hash テストが同じ値を確かめる。
            self.assertEqual(
                transcribe.content_hash(p),
                "ea8f163db38682925e4491c5e58d4bb3506ef8c14eb78a86e908c5624a67200f",
            )


class ContentHashOptionTest(unittest.TestCase):
    def parse(self, value):
        return transcribe.parse_args(["m.wav", "--out", "o.json", "--content-hash", value])

    def test_64_桁の小文字の16進は受け取る(self):
        self.assertEqual(self.parse("ab" * 32).content_hash, "ab" * 32)

    def test_パスに使えない値は拒否する(self):
        for bad in ["../x", "..\\x", "ab" * 31, "AB" * 32, "ab" * 32 + "/", ""]:
            with self.subTest(bad=bad), self.assertRaises(SystemExit):
                self.parse(bad)


@unittest.skipUnless(os.environ.get("MM_TRANSCRIBE_E2E") and SAMPLE_WAV.is_file(), "通しの試験は MM_TRANSCRIBE_E2E=1 とフィクスチャの音声が要る")
class EndToEndTest(unittest.TestCase):
    def test_日本語のフィクスチャの音声がスキーマどおりに書かれる(self):
        with tempfile.TemporaryDirectory() as d:
            cache = Path(d) / "cache"
            subprocess.run(
                [sys.executable, "-I", str(HERE / "transcribe.py"), str(SAMPLE_WAV), "--cache", str(cache), "--language", "ja"],
                check=True,
            )
            (out,) = (cache / "ingest").glob("*.transcript.json")
            data = json.loads(out.read_text(encoding="utf-8"))
            schema = json.loads(transcribe.find_schema().read_text(encoding="utf-8"))
            jsonschema.validate(data, schema)
            text = "".join(s["text"] for s in data["segments"])
            self.assertIn("文字起こし", text)


if __name__ == "__main__":
    unittest.main()
