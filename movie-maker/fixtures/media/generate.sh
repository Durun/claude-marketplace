#!/bin/sh
# 見本の素材を ffmpeg の CLI で out/ に作る。生成物は git に入れない。
# コマの番号は左上の 16x16 の 16 個の箱（下位ビットが左）に白黒で焼く。
# 圧縮に強く、デコード後の画素から番号を読み戻せる（読み方は mm-decode-ffmpeg/tests/common.rs）。
# 人の目用の番号は drawtext で重ねる（フォントが見つからなければ省く）。
set -eu

here=$(cd "$(dirname "$0")" && pwd)
out="$here/out"
mkdir -p "$out"
cd "$out"

FFMPEG=${FFMPEG:-ffmpeg}
SIZE=320x180

find_font() {
  for f in /c/Windows/Fonts/arial.ttf /mnt/c/Windows/Fonts/arial.ttf \
    /usr/share/fonts/truetype/dejavu/DejaVuSans.ttf /System/Library/Fonts/Supplemental/Arial.ttf; do
    [ -f "$f" ] && { echo "$f"; return; }
  done
}

font=$(find_font || true)
label=""
# コロンやバックスラッシュの入るパスを filter に渡さないよう、フォントを out/ に写して相対で指す
if [ -n "$font" ]; then
  cp "$font" "$out/font.ttf"
  label=",drawtext=fontfile=font.ttf:text='%{n}':x=24:y=60:fontsize=48:fontcolor=white"
fi

# N は入力のコマの通し番号。16 ビットを箱にして、それ以外は元の輝度のまま。
barcode="geq=lum='if(lt(Y,16)*lt(X,256),16+219*mod(floor(N/pow(2,floor(X/16))),2),p(X,Y))':cb=128:cr=128"

# lavfi の入力にする文字列を返す。$1 = fps（分数可）、$2 = コマ数。
video_source() {
  fps=$1
  frames=$2
  echo "color=c=0x606060:s=${SIZE}:r=${fps},format=yuv420p,${barcode}${label},trim=end_frame=${frames}"
}

x264="-c:v libx264 -preset veryfast -crf 12 -pix_fmt yuv420p -color_primaries bt709 -color_trc bt709 -colorspace bt709 -color_range tv"

# CFR 30fps、60 コマ、B フレームあり、キーフレーム間隔 12、AAC の 440Hz つき
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 60)" \
  -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=2,aformat=channel_layouts=stereo" \
  $x264 -g 12 -bf 2 -c:a aac -b:a 128k -shortest "$out/cfr.mp4"

# 30000/1001 fps、30 コマ（分数の fps でコマの番号がずれない確認）
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30000/1001 30)" \
  $x264 -g 10 -bf 2 -video_track_timescale 30000 "$out/ntsc.mp4"

# VFR: 3 コマに 1 回、時刻が 12ms 遅れる。時間軸は 90000。
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 60)" \
  -vf "settb=1/90000,setpts='(N/30+0.012*mod(N,3))/TB'" \
  $x264 -g 12 -bf 0 -fps_mode passthrough -enc_time_base 1/90000 -video_track_timescale 90000 "$out/vfr.mp4"

# 最初のコマが 0 秒でない素材（時間軸の頭が 0.5 秒）
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 30)" \
  $x264 -g 10 -bf 0 -output_ts_offset 0.5 -avoid_negative_ts disabled -video_track_timescale 90000 "$out/offset.mp4"

# B フレームがあり、頭が 0.5 秒ずれた素材
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 30)" \
  $x264 -g 10 -bf 2 -output_ts_offset 0.5 -avoid_negative_ts disabled -video_track_timescale 90000 "$out/offset_bf.mp4"

# B フレームがある VFR
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 60)" \
  -vf "settb=1/90000,setpts='(N/30+0.012*mod(N,3))/TB'" \
  $x264 -g 12 -bf 2 -fps_mode passthrough -enc_time_base 1/90000 -video_track_timescale 90000 "$out/vfr_bf.mp4"

# 再エンコードしない切り出し。キーフレームの手前から始まり、edit list の外のパケットが DISCARD になる。
"$FFMPEG" -y -v error -ss 0.5 -i "$out/cfr.mp4" -c copy -an "$out/editlist.mp4"

# 48 kHz・ステレオでない音（44.1 kHz のモノラル、32 kHz の 5.1）
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 30)" \
  -f lavfi -i "sine=frequency=440:sample_rate=44100:duration=1,aformat=channel_layouts=mono" \
  $x264 -g 10 -bf 0 -c:a aac -b:a 96k -shortest "$out/mono44.mp4"
"$FFMPEG" -y -v error -f lavfi -i "$(video_source 30 30)" \
  -f lavfi -i "sine=frequency=440:sample_rate=32000:duration=1,aformat=channel_layouts=5.1" \
  $x264 -g 10 -bf 0 -c:a aac -b:a 192k -shortest "$out/surround.mp4"

# トーンだけの音
"$FFMPEG" -y -v error -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=2,aformat=channel_layouts=stereo" \
  -c:a pcm_s16le "$out/tone.wav"

# 壊れた動画: moov が末尾にある MP4 を途中で切る。パスに "broken" を含める。
size=$(wc -c <"$out/cfr.mp4")
head -c $((size / 3)) "$out/cfr.mp4" >"$out/broken.mp4"

date >"$out/.stamp-2"
