// 各段階の姿を端末に出す。`npx tsx preview.ts` で走る。
// cells は Raster へ渡すのと同じ文字列を、そのまま ANSI に戻して描く。

import { render } from './hooks/draw.ts'
import { feed, newPet, OUTPUT_PER_POOP, STAGE_LABEL, stageOf, type Pet } from './hooks/pet.ts'

const COLUMNS = 44
const ROWS = 12

const show = (pet: Pet, label: string) => {
  const cells = render(COLUMNS, ROWS, pet, 0)
  const words = new Uint32Array(Buffer.from(cells, 'base64').buffer.slice(0))
  const color = (v: number, layer: 38 | 48) =>
    v === 0x01000000 ? `\x1b[${layer + 1}m` : `\x1b[${layer};2;${(v >> 16) & 255};${(v >> 8) & 255};${v & 255}m`
  let out = `\n${label}  ${STAGE_LABEL[stageOf(pet)]}\n`
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLUMNS; col += 1) {
      const at = (row * COLUMNS + col) * 3
      out += `${color(words[at + 1] ?? 0, 38)}${color(words[at + 2] ?? 0, 48)}${String.fromCodePoint(words[at] ?? 32)}`
    }
    out += '\x1b[0m\n'
  }
  console.log(out)
}

let pet = newPet('preview', '/tmp/work', new Date('2026-09-18T00:00:00Z'))
show(pet, '生まれる前')
for (const percent of [3, 12, 22, 32, 42]) {
  pet = feed(pet, 60_000, OUTPUT_PER_POOP, percent)
  show(pet, `${percent}%`)
}
show(feed(pet, 0, OUTPUT_PER_POOP * 5, 42), 'ウンチを溜めた')
