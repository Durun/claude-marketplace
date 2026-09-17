// コマ送りを端末に描いて目視するための道具。
//   node --experimental-strip-types preview.ts               並べて出す
//   node --experimental-strip-types preview.ts --play        アニメーションで再生する
//   node --experimental-strip-types preview.ts --play "セリフ"  セリフを変えて再生する
//   node --experimental-strip-types preview.ts --play --intense  カットイン付きの激しい登場で再生する
import { faceRows, runs } from './hooks/face.ts'
import {
  FACE_AREA_HEIGHT,
  FACE_AREA_WIDTH,
  FRAME_MS,
  applyDebris,
  faceOffset,
  facePixelRow,
  lastFrame,
  mouthOpen,
  speechLength,
  speechParts,
  spokenRuns,
  wallRow,
} from './hooks/entrance.ts'
import { CUTIN_FRAMES, cutinRows } from './hooks/cutin.ts'

const ESC = String.fromCharCode(27)
const RESET = ESC + '[0m'
const hex = (h, bg) => {
  const [r, g, b] = [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16))
  return ESC + '[' + (bg ? 48 : 38) + ';2;' + r + ';' + g + ';' + b + 'm'
}

const args = process.argv.slice(2)
const SPEECH = args.find((a) => !a.startsWith('--')) ?? '**主語**がないぜ主語が**ぁ〜〜！！**'
const INTENSE = args.includes('--intense')
const PARTS = speechParts(SPEECH)
const LEAD = INTENSE ? CUTIN_FRAMES : 0
// カットインは帯いっぱいに描くので、端末の幅で受ける。
const BODY_COLUMNS = process.stdout.columns ?? 80
const FRAMES = LEAD + lastFrame(speechLength(PARTS)) + 1

// セリフは顔の縦の中ほどに置く。
const SPEECH_ROW = Math.floor(FACE_AREA_HEIGHT / 2)

const draw = (frame) => {
  const at = frame - LEAD
  const cutin = at < 0
  const said = spokenRuns(at, PARTS)
  const cells = cutin
    ? cutinRows(frame, BODY_COLUMNS)
    : applyDebris(
        faceRows(
          faceOffset(at, INTENSE),
          facePixelRow(at, INTENSE),
          FACE_AREA_WIDTH,
          FACE_AREA_HEIGHT,
          mouthOpen(at, speechLength(PARTS)),
        ),
        at,
        INTENSE,
      )
  return cells.map((row, i) => {
    let out = ESC + '[90m' + (cutin ? '' : wallRow(at, i, INTENSE)) + RESET
    for (const r of runs(row)) {
      out += (r.color ? hex(r.color, false) : '') + (r.backgroundColor ? hex(r.backgroundColor, true) : '') + r.char + RESET
    }
    if (i !== SPEECH_ROW || said.length === 0) return out
    const spoken = said.map((run) => hex(run.color, false) + run.text).join('')
    return out + ' ' + spoken + RESET
  })
}

if (args.includes('--play')) {
  const height = FACE_AREA_HEIGHT + 1
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms))
  process.stdout.write(ESC + '[?25l')
  console.log('\n'.repeat(height - 1))
  for (let loop = 0; loop < 3; loop++) {
    for (let frame = 0; frame < FRAMES; frame++) {
      const lines = draw(frame)
      while (lines.length < height) lines.push('')
      process.stdout.write(ESC + '[' + height + 'A' + ESC + '[J')
      console.log(lines.join('\n'))
      await sleep(FRAME_MS)
    }
    await sleep(900)
  }
  process.stdout.write(ESC + '[?25h')
} else {
  for (let frame = 0; frame < FRAMES; frame++) {
    console.log('--- frame ' + frame + ' ---')
    console.log(draw(frame).join('\n'))
  }
}
