// コマ送りを端末に描いて目視するための道具。
//   node --experimental-strip-types preview.ts               並べて出す
//   node --experimental-strip-types preview.ts --play        アニメーションで再生する
//   node --experimental-strip-types preview.ts --play "セリフ"  セリフを変えて再生する
//   node --experimental-strip-types preview.ts --play "セリフ" "ババアのセリフ"  ババアのセリフも変えて再生する
//   node --experimental-strip-types preview.ts --play --intense  カットイン付きの激しい登場で再生する
import { FACE_WIDTH, faceRows, runs } from './hooks/face.ts'
import { BAABA, BAABA_PALETTE, baabaLastFrame, baabaPixelRow, baabaSpeechFrame } from './hooks/baaba.ts'
import {
  WALL_WIDTH,
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
const BAABA_SPEECH = positional[1] ?? 'まあ落ち着いて。\n**「面」**は**「画面の描画領域」**のことだと思うわよ。'
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

const paintRuns = (row) => {
  let out = ''
  for (const r of runs(row)) {
    out += (r.color ? hex(r.color, false) : '') + (r.backgroundColor ? hex(r.backgroundColor, true) : '') + r.char + RESET
  }
  return out
}

/** ババアのセリフを行ごとに右寄せした文字列にする。改行で切り、幅 width に収める。 */
const baabaLines = (at, width) => {
  const said = spokenRuns(baabaSpeechFrame(at - OYAJI_END), BAABA_PARTS, BAABA_PALETTE)
  const lines = [{ plain: '', colored: '' }]
  for (const run of said) {
    for (const ch of run.text) {
      if (ch === '\n') lines.push({ plain: '', colored: '' })
      else {
        const line = lines[lines.length - 1]
        line.plain += ch
        line.colored += hex(run.color, false) + ch
      }
    }
  }
  return lines.filter((l) => l.plain !== '').map((l) => ' '.repeat(Math.max(0, width - columnsOf(l.plain))) + l.colored + RESET)
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
  if (cutin) return cells.map((row) => paintRuns(row))
  const baabaAt = at - OYAJI_END
  const face = faceRows(0, baabaPixelRow(baabaAt), FACE_WIDTH, FACE_AREA_HEIGHT, mouthOpen(baabaSpeechFrame(baabaAt), speechLength(BAABA_PARTS)), BAABA)
  // 中央の列はオヤジのセリフの行の下にババアのセリフを右寄せで積む。
  const middleWidth = BODY_COLUMNS - WALL_WIDTH - FACE_AREA_WIDTH - FACE_WIDTH - 2
  const middle = []
  for (let i = 0; i < FACE_AREA_HEIGHT; i++) middle.push('')
  const spoken = said.map((run) => hex(run.color, false) + run.text).join('')
  if (said.length > 0) middle[SPEECH_ROW] = spoken + RESET + ' '.repeat(Math.max(0, middleWidth - columnsOf(said.map((r) => r.text).join(''))))
  baabaLines(at, middleWidth).forEach((line, j) => { middle[SPEECH_ROW + 1 + j] = line })
  return cells.map((row, i) => {
    const wall = ESC + '[90m' + wallRow(at, i, INTENSE) + RESET
    const mid = middle[i] || ' '.repeat(middleWidth)
    return wall + paintRuns(row) + ' ' + mid + ' ' + paintRuns(face[i])
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
