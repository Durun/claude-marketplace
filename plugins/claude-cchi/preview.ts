// 目の 4 種と、成長段階ごとの姿を端末に出す。`npx tsx preview.ts` で走る。
// cells は Raster へ渡すのと同じ文字列を、そのまま ANSI に戻して描く。

import { render } from './hooks/draw.ts'
import { newScene } from './hooks/scene.ts'
import { feed, newPet, stageOf, STAGE_LABEL, traitsOf, type Pet } from './hooks/pet.ts'

const COLUMNS = 44
const ROWS = 12
const WORLD = { width: COLUMNS * 2, ground: ROWS * 2 - 3 }

const show = (pet: Pet, label: string) => {
  const cells = render(COLUMNS, ROWS, pet, newScene(), WORLD)
  const words = new Uint32Array(Buffer.from(cells, 'base64').buffer.slice(0))
  const color = (v: number, layer: 38 | 48) =>
    v === 0x01000000
      ? `\x1b[${layer + 1}m`
      : `\x1b[${layer};2;${(v >> 16) & 255};${(v >> 8) & 255};${v & 255}m`
  let out = `\n${label}\n`
  for (let row = 0; row < ROWS; row += 1) {
    for (let col = 0; col < COLUMNS; col += 1) {
      const at = (row * COLUMNS + col) * 3
      out += `${color(words[at + 1] ?? 0, 38)}${color(words[at + 2] ?? 0, 48)}${String.fromCodePoint(words[at] ?? 32)}`
    }
    out += '\x1b[0m\n'
  }
  console.log(out)
}

/** 目の形ごとに 1 匹ずつ探す。形は生まれた日から決まるので、日を送って見つける。 */
const EYE_LABEL = ['まる目', 'たれ目', 'つり目', 'てん目']
const samples = new Map<number, Pet>()
for (let i = 0; i < 600 && samples.size < EYE_LABEL.length; i += 1) {
  const pet = feed(newPet(`sample${i}`, '/w', new Date(2026, 0, 1 + i)), 60_000, 0, 22, new Date())
  const { eye } = traitsOf(pet)
  if (!samples.has(eye)) samples.set(eye, pet)
}
for (const [eye, pet] of [...samples].sort((a, b) => a[0] - b[0])) {
  show(pet, `${EYE_LABEL[eye]}  健康 ${pet.health}`)
  if (eye === 0) show({ ...pet, health: 20 }, `${EYE_LABEL[eye]}  健康 20`)
}

// 成長段階は使用率で決まる。1 匹を育てながら並べる。
let growing = newPet('growth', '/w', new Date(2026, 0, 1))
for (const percent of [3, 12, 22, 32, 42]) {
  growing = feed(growing, 60_000, 0, percent, new Date())
  show(growing, `${STAGE_LABEL[stageOf(growing)]}  使用率 ${percent}%`)
}
