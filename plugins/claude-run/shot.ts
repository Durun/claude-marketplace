// 開発用。指定のコマを PNG に落として見た目を確かめる。
// 実行: npx tsx shot.ts <コマ番号> <出力パス> [拡大率]
import { writeFileSync } from 'node:fs'
import { deflateSync } from 'node:zlib'
import { renderPixels } from './hooks/scene.ts'
import { initial, step, type Game } from './hooks/game.ts'

const COLUMNS = 84
const ROWS = 22

const at = Number(process.argv[2] ?? 0)
const path = process.argv[3] ?? 'shot.png'
const zoom = Number(process.argv[4] ?? 6)

let game: Game = initial()
for (let i = 0; i <= at; i += 1) {
  const near = game.obstacles.find((o) => o.x > 1.6 && o.x < 3.4)
  game = step(game, near !== undefined && game.runnerY === 0)
}
const width = COLUMNS
const height = ROWS * 2
const pixels = renderPixels(width, ROWS, game, 3)

const crc = (buf: Uint8Array) => {
  let c = 0xffffffff
  for (const byte of buf) {
    c ^= byte
    for (let k = 0; k < 8; k += 1) c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1
  }
  return (c ^ 0xffffffff) >>> 0
}

const chunk = (type: string, data: Uint8Array) => {
  const head = new Uint8Array(8)
  const view = new DataView(head.buffer)
  view.setUint32(0, data.length)
  for (let i = 0; i < 4; i += 1) head[4 + i] = type.charCodeAt(i)
  const body = new Uint8Array(head.length + data.length + 4)
  body.set(head, 0)
  body.set(data, 8)
  new DataView(body.buffer).setUint32(body.length - 4, crc(body.subarray(4, body.length - 4)))
  return body
}

const out = new Uint8Array((width * zoom * 3 + 1) * height * zoom)
let cursor = 0
for (let y = 0; y < height * zoom; y += 1) {
  out[cursor] = 0
  cursor += 1
  for (let x = 0; x < width * zoom; x += 1) {
    const c = pixels[Math.floor(y / zoom) * width + Math.floor(x / zoom)] ?? 0
    out[cursor] = (c >> 16) & 255
    out[cursor + 1] = (c >> 8) & 255
    out[cursor + 2] = c & 255
    cursor += 3
  }
}

const ihdr = new Uint8Array(13)
const hv = new DataView(ihdr.buffer)
hv.setUint32(0, width * zoom)
hv.setUint32(4, height * zoom)
ihdr[8] = 8
ihdr[9] = 2

const png = [
  new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10]),
  chunk('IHDR', ihdr),
  chunk('IDAT', new Uint8Array(deflateSync(out))),
  chunk('IEND', new Uint8Array(0)),
]
writeFileSync(path, Buffer.concat(png.map((p) => Buffer.from(p))))
process.stderr.write(`${path} (frame ${at})\n`)
