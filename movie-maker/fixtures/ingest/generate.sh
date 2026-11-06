#!/bin/sh
# 取り込みのフィクスチャを out/ に作る。生成物は git に入れない（transcript-ja.json だけは git に入れる）。
#   speech-ja.wav    日本語の短い音声（Windows の音声合成 Microsoft Haruka）。文字起こしのフィクスチャ
#   long-1080p.mp4   1 分の 1080p（H.264、音つき）。取り込みの時間の計測用
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
mkdir -p "$out"

FFMPEG=${FFMPEG:-ffmpeg}

# 音声合成は .NET の System.Speech（Windows のみ）。ja-JP の声が無ければ作らない。
if command -v powershell.exe >/dev/null 2>&1; then
  win_out=$(cygpath -w "$out/speech-ja.wav")
  powershell.exe -NoProfile -Command "
    Add-Type -AssemblyName System.Speech
    \$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
    \$voice = \$s.GetInstalledVoices() | Where-Object { \$_.VoiceInfo.Culture.Name -eq 'ja-JP' } | Select-Object -First 1
    if (\$voice) {
      \$s.SelectVoice(\$voice.VoiceInfo.Name)
      \$s.SetOutputToWaveFile('$win_out')
      \$s.Speak('こんにちは。これは動画編集ソフトの、文字起こしの試験です。')
      \$s.Dispose()
    }
  "
fi

# 動きのある映像（testsrc2）と 440Hz の音。1 分。
"$FFMPEG" -y -v error -f lavfi -i "testsrc2=s=1920x1080:r=30:d=60" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=60,aformat=channel_layouts=stereo" \
  -c:v libx264 -preset veryfast -crf 18 -pix_fmt yuv420p -c:a aac -b:a 192k -shortest "$out/long-1080p.mp4"
