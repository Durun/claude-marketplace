// 成長・餌・排泄の計算と、面の符号化の長さを確かめる。`npx tsx check.ts` で走る。

import assert from 'node:assert/strict'
import { artLines, render } from './hooks/draw.ts'
import { feed, flush, newPet, OUTPUT_PER_POOP, poopCount, stageOf, traitsOf } from './hooks/pet.ts'

const born = new Date('2026-09-18T00:00:00Z')
let pet = newPet('session-1', '/tmp/work', born)

assert.equal(stageOf(pet), 'egg', '食べる前は卵')

pet = feed(pet, 1000, 0, 3)
assert.equal(stageOf(pet), 'baby', '最初の食事でかえる')

for (const [percent, stage] of [
  [9.9, 'baby'],
  [10, 'child'],
  [20, 'adult'],
  [30, 'ojisan'],
  [40, 'ojiisan'],
] as const) {
  assert.equal(stageOf(feed(pet, 0, 0, percent)), stage, `${percent}% は ${stage}`)
}

// ウンチは出力トークンだけで増え、流すと 0 に戻る。
pet = feed(pet, 0, OUTPUT_PER_POOP * 3, 15)
assert.equal(poopCount(pet), 3)
const dirty = feed(pet, 0, OUTPUT_PER_POOP * 3, 15)
assert.equal(poopCount(dirty), 6)
assert.ok(dirty.health < pet.health, '溜めすぎると健康が減る')
assert.equal(poopCount(flush(dirty)), 0, '流すと溜まりが消える')
assert.ok(flush(dirty).health > dirty.health, '流すと健康が戻る')

// 形は変わらず、色だけが食べた量で濃くなる。
const young = traitsOf(pet)
const grown = traitsOf({ ...pet, input: 300_000 })
assert.equal(young.eye, grown.eye)
assert.equal(young.body, grown.body)
assert.notEqual(young.color, grown.color)
assert.notEqual(traitsOf({ ...pet, id: 'session-2' }).color, young.color, '別の子は別の色')

// 基準の姿は、元の四分ブロックの絵にそのまま戻る。
assert.deepEqual(artLines(), [' \u2590\u259b\u2588\u2588\u2588\u259b\u2588', '\u259d\u259c\u2588\u2588\u2588\u2588\u2588\u2588\u2580', '  \u259d\u259d \u259d\u259d'])

// cells は columns * rows * 3 語を base64 にしたもの。
const columns = 40
const rows = 12
const cells = render(columns, rows, pet, 0)
assert.equal(cells.length, Math.ceil((columns * rows * 12) / 3) * 4)
assert.ok(/^[A-Za-z0-9+/]+=*$/.test(cells))

console.log('ok')
