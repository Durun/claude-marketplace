// コマ送りを端末に描いて目視するための道具。
//   node --experimental-strip-types preview.ts               並べて出す
//   node --experimental-strip-types preview.ts --play        アニメーションで再生する
//   node --experimental-strip-types preview.ts --play "セリフ"  セリフを変えて再生する
//   node --experimental-strip-types preview.ts --play "セリフ" "ババアのセリフ"  ババアのセリフも変えて再生する
//   node --experimental-strip-types preview.ts --play --intense  カットイン付きの激しい登場で再生する
import { FACE_HEIGHT, FACE_WIDTH, faceRows, runs } from './hooks/face.ts'
import { BAABA, BAABA_PALETTE, baabaLastFrame, baabaPixelRow, baabaSpeechFrame } from './hooks/baaba.ts'
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
const positional = args.filter((a) => !a.startsWith('--'))
const SPEECH = positional[0] ?? '**「面」**って一体何のことダァ**〜〜！？**'
const BAABA_SPEECH = positional[1] ?? 'まあ落ち着いて。**「面」**は**「画面の描画領域」**のことだと思うわよ。'
const INTENSE = args.includes('--intense')
const PARTS = speechParts(SPEECH)
const BAABA_PARTS = speechParts(BAABA_SPEECH)
const LEAD = INTENSE ? CUTIN_FRAMES : 0
// カットインは帯いっぱいに描くので、端末の幅で受ける。
const BODY_COLUMNS = process.stdout.columns ?? 80
const OYAJI_END = lastFrame(speechLength(PARTS))
const FRAMES = LEAD + OYAJI_END + baabaLastFrame(speechLength(BAABA_PARTS)) + 1

// 端末の桁数。全角は 2 桁で数える。右寄せの位置合わせに使う。
const columnsOf = (text) => [...text].reduce((n, ch) => n + (ch.charCodeAt(0) > 0xff ? 2 : 1), 0)

/** ババアの行。セリフを右寄せにして顔の左に置く。 */
const drawBaaba = (at) => {
  const baabaAt = at - OYAJI_END
  const speechFrame = baabaSpeechFrame(baabaAt)
  const said = spokenRuns(speechFrame, BAABA_PARTS, BAABA_PALETTE)
  const face = faceRows(0, baabaPixelRow(baabaAt), FACE_WIDTH, FACE_HEIGHT, mouthOpen(speechFrame, speechLength(BAABA_PARTS)), BAABA)
  return face.map((row, i) => {
    let out = ''
    for (const r of runs(row)) {
      out += (r.color ? hex(r.color, false) : '') + (r.backgroundColor ? hex(r.backgroundColor, true) : '') + r.char + RESET
    }
    if (i !== Math.floor(FACE_HEIGHT / 2) || said.length === 0) return ' '.repeat(BODY_COLUMNS - FACE_WIDTH) + out
    const plain = said.map((run) => run.text).join('')
    const spoken = said.map((run) => hex(run.color, false) + run.text).join('')
    return ' '.repeat(Math.max(0, BODY_COLUMNS - FACE_WIDTH - 1 - columnsOf(plain))) + spoken + RESET + ' ' + out
  })
}

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
  const rows = cells.map((row, i) => {
    let out = ESC + '[90m' + (cutin ? '' : wallRow(at, i, INTENSE)) + RESET
    for (const r of runs(row)) {
      out += (r.color ? hex(r.color, false) : '') + (r.backgroundColor ? hex(r.backgroundColor, true) : '') + r.char + RESET
    }
    if (i !== SPEECH_ROW || said.length === 0) return out
    const spoken = said.map((run) => hex(run.color, false) + run.text).join('')
    return out + ' ' + spoken + RESET
  })
  return cutin ? rows : [...rows, ...drawBaaba(at)]
}

if (args.includes('--play')) {
  const height = FACE_AREA_HEIGHT + FACE_HEIGHT + 1
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
