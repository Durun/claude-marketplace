#!/usr/bin/env -S npx tsx
// 端末だけで遊ぶ版。Claude Code を立てずに同じ絵とゲームが動く。
// 実行: npx tsx cli.ts        遊ぶ（space か j で跳ぶ、r でやり直し、q で終わり）
//       npx tsx cli.ts --auto 自動操作で流す。1 コマの所要時間も出す
//       npx tsx cli.ts --ss 3  1 画素あたりの光線を増やして輪郭をなめらかにする
// 遊んでいる最中に p を押すと、そのコマを PNG に落とす。見え方の相談に使う。
import { FRAME_MS, initial, step, type Game } from './hooks/game.ts'
import { writeFileSync } from 'node:fs'
import { deflateSync } from 'node:zlib'
import { renderPixels, SUPER_SAMPLE } from './hooks/scene.ts'

const ESC = String.fromCharCode(27)
const MAX_ROWS = 26
const MAX_COLUMNS = 140

const ss = process.argv.includes('--ss') ? Number(process.argv[process.argv.indexOf('--ss') + 1]) : SUPER_SAMPLE
const superSample = Number.isFinite(ss) && ss >= 1 ? Math.min(Math.round(ss), 4) : SUPER_SAMPLE

const auto = process.argv.includes('--auto')
const frames = Number(process.argv[process.argv.indexOf('--auto') + 1])
const autoFrames = auto ? (Number.isFinite(frames) ? frames : 400) : 0

const size = () => ({
  columns: Math.max(24, Math.min(process.stdout.columns ?? 100, MAX_COLUMNS)),
  rows: Math.max(6, Math.min((process.stdout.rows ?? 24) - 3, MAX_ROWS)),
})

const paint = (pixels: Uint32Array, columns: number, rows: number) => {
  let out = ''
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < columns; col += 1) {
      const top = pixels[row * 2 * columns + col] ?? 0
      const bottom = pixels[(row * 2 + 1) * columns + col] ?? 0
      out += `${ESC}[38;2;${(top >> 16) & 255};${(top >> 8) & 255};${top & 255}m`
      out += `${ESC}[48;2;${(bottom >> 16) & 255};${(bottom >> 8) & 255};${bottom & 255}m▀`
    }
    out += `${ESC}[0m\n`
  }
  return out
}

/** 最後に描いた画素。p を押したときに PNG へ落とす。 */
let shot: { pixels: Uint32Array; columns: number; rows: number } | null = null

const crc = (bytes: Uint8Array) => {
  let v = 0xffffffff
  for (const byte of bytes) {
    v ^= byte
    for (let k = 0; k < 8; k += 1) v = v & 1 ? (v >>> 1) ^ 0xedb88320 : v >>> 1
  }
  return (v ^ 0xffffffff) >>> 0
}

const chunk = (type: string, data: Uint8Array) => {
  const body = new Uint8Array(12 + data.length)
  const view = new DataView(body.buffer)
  view.setUint32(0, data.length)
  for (let i = 0; i < 4; i += 1) body[4 + i] = type.charCodeAt(i)
  body.set(data, 8)
  view.setUint32(body.length - 4, crc(body.subarray(4, body.length - 4)))
  return body
}

/** 今のコマを等倍 4 倍で PNG に落とす。画素の並びをそのまま見られる。 */
const capture = () => {
  if (!shot) return
  const zoom = 4
  const { pixels, columns, rows } = shot
  const w = columns * zoom
  const h = rows * 2 * zoom
  const raw = new Uint8Array((w * 3 + 1) * h)
  let c = 0
  for (let y = 0; y < h; y += 1) {
    raw[c] = 0
    c += 1
    for (let x = 0; x < w; x += 1) {
      const color = pixels[Math.floor(y / zoom) * columns + Math.floor(x / zoom)] ?? 0
      raw[c] = (color >> 16) & 255
      raw[c + 1] = (color >> 8) & 255
      raw[c + 2] = color & 255
      c += 3
    }
  }
  const ihdr = new Uint8Array(13)
  const view = new DataView(ihdr.buffer)
  view.setUint32(0, w)
  view.setUint32(4, h)
  ihdr[8] = 8
  ihdr[9] = 2
  const name = `claude-run-${game.score}.png`
  writeFileSync(
    name,
    Buffer.concat(
      [
        new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]),
        chunk('IHDR', ihdr),
        chunk('IDAT', new Uint8Array(deflateSync(raw))),
        chunk('IEND', new Uint8Array(0)),
      ].map((part) => Buffer.from(part)),
    ),
  )
  captured = name
}

let captured = ''
let game: Game = initial()
let jumpRequested = false
let spent = 0
let drawn = 0

const draw = () => {
  const { columns, rows } = size()
  const at = Date.now()
  const pixels = renderPixels(columns, rows, game, superSample)
  shot = { pixels, columns, rows }
  spent += Date.now() - at
  drawn += 1
  const shotLine = captured === '' ? '' : `  ${captured} に保存した`
  const line = game.over
    ? `当たった  score ${game.score}  best ${game.best}  r でやり直し  q で終わり`
    : `score ${game.score}  best ${game.best}  space か j で跳ぶ  q で終わり`
  process.stdout.write(`${ESC}[H${paint(pixels, columns, rows)}${line}${shotLine}${ESC}[K`)
}

const quit = () => {
  process.stdout.write(`${ESC}[?25h${ESC}[?1049l`)
  if (auto) process.stderr.write(`${(spent / Math.max(drawn, 1)).toFixed(1)} ms/frame\n`)
  process.exit(0)
}

process.stdout.write(`${ESC}[?1049h${ESC}[?25l${ESC}[2J`)

if (!auto && process.stdin.isTTY) {
  process.stdin.setRawMode(true)
  process.stdin.resume()
  process.stdin.on('data', (chunk) => {
    const key = chunk.toString()
    if (key === 'q' || key === '') quit()
    if (key === ' ' || key === 'j') jumpRequested = true
    if (key === 'r' && game.over) game = initial(game.best)
    if (key === 'p') capture()
  })
}

let ticks = 0
const timer = setInterval(() => {
  if (auto) {
    // 柱の手前で必ず跳ぶ自動操作。当たり判定と画角の変化を通しで見る。
    jumpRequested = game.obstacles.some((o) => o.x / game.speed > 7 && o.x / game.speed < 15) && game.runnerY === 0
  }
  if (!game.over) {
    game = step(game, jumpRequested)
    if (game.over) game = { ...game, best: Math.max(game.best, game.score) }
  }
  jumpRequested = false
  draw()
  ticks += 1
  if (auto && ticks >= autoFrames) {
    clearInterval(timer)
    quit()
  }
}, FRAME_MS)
