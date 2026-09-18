import { FACE_HEIGHT, FACE_WIDTH, type Cell } from './face.ts'

export const FRAME_MS = 45

/** 顔が定位置に着くコマ。ここから喋り始める。 */
export const SETTLED_FRAME = 9

/** 破片が落ち切るコマ。顔は先に止まり、破片だけが残って落ちる。 */
const DEBRIS_END_FRAME = 13

/** 1 コマで送るセリフの文字数。 */
const CHARS_PER_FRAME = 1

/** 最後の文字が地の色に落ち着き、口を閉じるまでのコマ数。 */
export const lastFrame = (speechLength: number) => {
  const spokenOut = SETTLED_FRAME + Math.ceil(speechLength / CHARS_PER_FRAME) - 1
  return Math.max(DEBRIS_END_FRAME, spokenOut + FADE_FRAMES + 1)
}

/** そのコマまでに出ている文字数。着地したコマから 1 コマぶんずつ増える。 */
const spokenLength = (frame: number) =>
  frame < SETTLED_FRAME ? 0 : (frame - SETTLED_FRAME + 1) * CHARS_PER_FRAME

/** 喋っている間だけ 1 コマおきに口を開く。出し切ったら閉じる。 */
export const mouthOpen = (frame: number, speechLength: number) => {
  if (frame < SETTLED_FRAME) return false
  // 直前のコマまでで出し切っていれば、もう喋っていない。
  if (spokenLength(frame - 1) >= speechLength) return false
  return (frame - SETTLED_FRAME) % 2 === 0
}

export const WALL_WIDTH = 3

/** 顔が収まる位置。ここから右は壁より手前になる。 */
const RESTING_OFFSET = 1

/** 着地したときの顔の位置。ピクセルで数える。飛び出した直後はこれより上にいる。 */
export const RESTING_PIXEL = 2

/** 面の行数。跳び上がったぶんの 1 行を顔の上に取る。 */
export const FACE_AREA_HEIGHT = FACE_HEIGHT + 1

/** 顔の右に、飛んだ破片が落ちていくぶんの空きを取る。 */
const DEBRIS_RUNWAY = 9

export const FACE_AREA_WIDTH = FACE_WIDTH + RESTING_OFFSET + DEBRIS_RUNWAY

/**
 * コマごとの顔の横位置。壁の裏で 2 コマ溜めてから 8 セル飛び出し、
 * 残り 7 セルを 5 コマかけて詰める。戻さないので揺り返しは起きない。
 */
const OFFSETS = [
  -FACE_WIDTH,
  -FACE_WIDTH,
  -FACE_WIDTH,
  -6,
  -3,
  -1,
  0,
  0,
  RESTING_OFFSET,
  RESTING_OFFSET,
] as const

/**
 * 語を 2 つ以上見つけたときの横位置。溜めずに飛び出し、定位置を 3 セル行き過ぎてから戻る。
 * 行き過ぎと揺り返しが、普段の減速だけの登場との差になる。
 */
const OFFSETS_INTENSE = [
  -FACE_WIDTH,
  -6,
  4,
  4,
  3,
  1,
  0,
  RESTING_OFFSET + 1,
  RESTING_OFFSET,
  RESTING_OFFSET,
] as const

export const faceOffset = (frame: number, intense = false) => {
  const table = intense ? OFFSETS_INTENSE : OFFSETS
  return table[Math.min(frame, table.length - 1)] ?? RESTING_OFFSET
}

/**
 * コマごとの顔の縦位置。飛び出した勢いで 2 ピクセル浮き、1 ピクセルずつ落ちて着く。
 * 浮きも落ちもセルの半分の幅で動くので、行単位で跳ぶより滑らかになる。
 */
const PIXEL_ROWS = [
  RESTING_PIXEL,
  RESTING_PIXEL,
  RESTING_PIXEL,
  0,
  0,
  0,
  1,
  1,
  RESTING_PIXEL,
  RESTING_PIXEL,
] as const

/** 語を 2 つ以上見つけたときの縦位置。浮いてから落ち、着いた反動でもう一度跳ねる。 */
const PIXEL_ROWS_INTENSE = [
  RESTING_PIXEL,
  0,
  0,
  1,
  RESTING_PIXEL,
  1,
  RESTING_PIXEL,
  RESTING_PIXEL,
  RESTING_PIXEL,
  RESTING_PIXEL,
] as const

export const facePixelRow = (frame: number, intense = false) => {
  const table = intense ? PIXEL_ROWS_INTENSE : PIXEL_ROWS
  return table[Math.min(frame, table.length - 1)] ?? RESTING_PIXEL
}

const WALL_INTACT = '▓▓▓'
const WALL_CRACKED = '▓▚▒'
const WALL_RIM = '▓▚ '
const WALL_OPEN = '   '

/**
 * コマごとの壁の姿。まず中央にひびが入り、次のコマで抜けて縁が欠ける。
 * 穴は開けたままにして、破れた壁を静止後も残す。
 */
export const wallRow = (frame: number, row: number, intense = false): string => {
  if (frame === 0) return WALL_INTACT
  // 激しい登場では溜めを飛ばして 1 コマで抜き、穴も端まで広げる。
  if (intense) {
    const distance = Math.abs(row - (RESTING_PIXEL / 2 + (FACE_HEIGHT - 1) / 2))
    if (distance < 3) return WALL_OPEN
    return WALL_RIM
  }
  // 穴は着地後の顔の高さに合わせて開ける。
  const distance = Math.abs(row - (RESTING_PIXEL / 2 + (FACE_HEIGHT - 1) / 2))
  if (frame === 1) return distance < 1 ? WALL_CRACKED : WALL_INTACT
  if (frame === 2) return distance < 2 ? WALL_CRACKED : WALL_INTACT
  if (distance < 2) return WALL_OPEN
  if (distance < 3) return WALL_RIM
  return WALL_CRACKED
}

/** 破片が飛び始めるコマ。壁が抜けるのと同時。 */
const BREAK_FRAME = 3

/** 1 コマあたりの落下加速度。セル単位で、横長の文字セルでも落ちて見える値に取る。 */
const GRAVITY = 0.8

// 穴の縁から前方へ飛ぶ破片。vx は右への速さ、vy は負が上向き。
const CHUNKS = [
  { char: '▞', row: 2.5, vx: 3.4, vy: -1.2 },
  { char: '▪', row: 3.5, vx: 4.2, vy: -0.6 },
  { char: '▚', row: 4.5, vx: 2.6, vy: 0.0 },
  { char: '・', row: 3.0, vx: 5.0, vy: -0.9 },
  { char: '▫', row: 4.0, vx: 3.0, vy: 0.3 },
  { char: '✦', row: 2.0, vx: 2.2, vy: -1.4 },
] as const

/** 破片を顔の描画面に重ねる。顔より手前を飛ぶので顔の上にも描く。面から出たものは消える。 */
/** 激しい登場で壁が抜けるコマ。溜めが無いぶん早い。 */
const BREAK_FRAME_INTENSE = 1

// 激しい登場で増える破片。速く、上下へ広く散る。
const CHUNKS_EXTRA = [
  { char: '▘', row: 1.5, vx: 5.6, vy: -1.8 },
  { char: '▗', row: 5.0, vx: 4.6, vy: 0.6 },
  { char: '✧', row: 1.0, vx: 3.6, vy: -2.0 },
  { char: '▖', row: 5.5, vx: 6.2, vy: -0.2 },
  { char: '・', row: 2.5, vx: 6.8, vy: -1.6 },
] as const

export const applyDebris = (rows: Cell[][], frame: number, intense = false): Cell[][] => {
  const breakFrame = intense ? BREAK_FRAME_INTENSE : BREAK_FRAME
  if (frame < breakFrame || frame >= DEBRIS_END_FRAME) return rows
  const t = frame - breakFrame
  for (const chunk of intense ? [...CHUNKS, ...CHUNKS_EXTRA] : CHUNKS) {
    const column = Math.round(chunk.vx * t)
    const row = Math.round(chunk.row + chunk.vy * t + (GRAVITY * t * t) / 2)
    const cells = rows[row]
    if (!cells?.[column]) continue
    cells[column] = { char: chunk.char, color: '#c9a227', backgroundColor: cells[column].backgroundColor }
  }
  return rows
}

// haiku が太字を指す印。囲みは取り除いて描く。
const EMPHASIS = /\*\*([^*]+)\*\*/g

export type SpeechPart = { text: string; emphasis: boolean }

/** セリフを太字の区間とそれ以外に切り分ける。印は落とす。 */
export const speechParts = (raw: string): SpeechPart[] => {
  const parts: SpeechPart[] = []
  let at = 0
  for (const found of raw.matchAll(EMPHASIS)) {
    const start = found.index
    if (start > at) parts.push({ text: raw.slice(at, start), emphasis: false })
    parts.push({ text: found[1] ?? '', emphasis: true })
    at = start + found[0].length
  }
  if (at < raw.length) parts.push({ text: raw.slice(at), emphasis: false })
  return parts.filter((part) => part.text !== '')
}

export const speechLength = (parts: SpeechPart[]) => parts.reduce((sum, part) => sum + part.text.length, 0)

// 出たばかりの文字から地の色へ落ちる 4 段。先頭ほど明るい。
// 強調した語は黄へ、それ以外は赤へ落とす。日本語は太字の字形を持たない書体が多いので、色で分ける。
const SPEECH_COLORS = ['#ffffff', '#ffd9d9', '#ffa8a8', '#ff6b6b'] as const
const EMPHASIS_COLORS = ['#ffffff', '#fff0c8', '#ffe29a', '#ffd166'] as const

export type Palette = { speech: readonly string[]; emphasis: readonly string[] }
const OYAJI_PALETTE: Palette = { speech: SPEECH_COLORS, emphasis: EMPHASIS_COLORS }

/** 出た文字が地の色に落ち着くまでのコマ数。 */
const FADE_FRAMES = SPEECH_COLORS.length - 1

export type SpeechRun = { text: string; color: string }

/**
 * そのコマで見えているセリフを、色が同じところでまとめて返す。
 * 文字は出てから 4 コマかけて地の色に落ち着く。
 */
export const spokenRuns = (frame: number, parts: SpeechPart[], palette: Palette = OYAJI_PALETTE): SpeechRun[] => {
  const shown = spokenLength(frame)
  const runs: SpeechRun[] = []
  let index = 0
  for (const part of parts) {
    for (const char of part.text) {
      if (index >= shown) return runs
      const age = Math.floor((shown - 1 - index) / CHARS_PER_FRAME)
      const ramp = part.emphasis ? palette.emphasis : palette.speech
      const color = ramp[Math.min(age, ramp.length - 1)] ?? SPEECH_COLORS[3]
      const last = runs.at(-1)
      if (last && last.color === color) last.text += char
      else runs.push({ text: char, color })
      index += 1
    }
  }
  return runs
}
