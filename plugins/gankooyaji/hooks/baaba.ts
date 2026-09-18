// オヤジが怒鳴り終えたあと、下からゆっくり昇ってきてたしなめるババア。
// 1 文字セルに上下 2 ピクセルを詰めるので、ドットは偶数行で持つ。
// G=白髪 S=肌 K=眼鏡の縁 E=レンズ P=瞳 N=鼻 R=開いた口 .=透明
import { FACE_HEIGHT, type Sprite } from './face.ts'
import { SETTLED_FRAME, lastFrame } from './entrance.ts'

const PIXELS = [
  '..............',
  '.....GGGG.....',
  '....GGGGGG....',
  '...GGGGGGGG...',
  '..GGGGGGGGGG..',
  '..GSSSSSSSSG..',
  '..SKKKSSKKKS..',
  '..SKEPKKPEKS..',
  '..SKKKSSKKKS..',
  '..SSSSNNSSSS..',
  '..SSSSSSSSSS..',
  '..SSRRRRRRSS..',
  '..SSSRRRRSSS..',
  '...SSSSSSSS...',
  '....SSSSSS....',
  '..............',
] as const

// 口を閉じたコマ。開いた口の 2 行だけを差し替える。
const MOUTH_SHUT = ['..SSSSSSSSSS..', '..SSSKKKKSSS..'] as const

export const BAABA: Sprite = {
  pixels: PIXELS,
  ink: {
    '.': null,
    G: '#d9d4cf',
    S: '#eccba8',
    K: '#4a3a30',
    E: '#dff1ff',
    P: '#2d1f14',
    N: '#d29c74',
    R: '#7d1a1a',
  },
  mouthShut: MOUTH_SHUT,
  mouthRow: 11,
}

/** オヤジが喋り終えてから昇り始めるまでの間。 */
const PAUSE_FRAMES = 6

/** 顔が下から出切るまでのコマ数。16 ピクセルを 2 コマに 1 ピクセルずつ昇る。 */
const RISE_FRAMES = FACE_HEIGHT * 2 * 2

/** そのコマの縦位置。ピクセルで数え、面の高さぶん下がっていれば全部隠れている。 */
export const baabaPixelRow = (baabaAt: number) =>
  Math.max(0, Math.ceil((RISE_FRAMES + PAUSE_FRAMES - baabaAt) / 2))

/**
 * セリフの送りと口パクはオヤジと同じ関数で数える。
 * あちらは着地のコマから喋り出すので、昇り切ったコマがそこに重なるようにずらす。
 */
export const baabaSpeechFrame = (baabaAt: number) => baabaAt - PAUSE_FRAMES - RISE_FRAMES + SETTLED_FRAME

/** ババアの最後の文字が地の色に落ち着くまでのコマ数。オヤジが喋り終えたコマから数える。 */
export const baabaLastFrame = (speechLength: number) =>
  PAUSE_FRAMES + RISE_FRAMES - SETTLED_FRAME + lastFrame(speechLength)

// 出たばかりの文字から地の色へ落ちる 4 段。オヤジの赤に対して青へ落とす。
export const BAABA_PALETTE = {
  speech: ['#ffffff', '#dce8ff', '#a9c4ff', '#6b9bff'],
  emphasis: ['#ffffff', '#e6f7ff', '#b8ecff', '#7dd3fc'],
} as const
