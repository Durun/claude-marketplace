// 成長・餌・排泄の計算と、面の符号化の長さを確かめる。`npx tsx check.ts` で走る。

import assert from 'node:assert/strict'
import { artLines, petWidth, render } from './hooks/draw.ts'
import {
  feed,
  flush,
  isDead,
  newPet,
  OUTPUT_PER_POOP,
  poopCount,
  rebirth,
  stageOf,
  traitsOf,
} from './hooks/pet.ts'
import { advance, newScene, sprinkle, startFlush, TOKENS_PER_GRAIN } from './hooks/scene.ts'

const born = new Date('2026-09-18T00:00:00Z')
let pet = newPet('session-1', '/tmp/work', born)

assert.equal(stageOf(pet), 'egg', '食べる前は卵')

pet = feed(pet, 1000, 0, 3, born)
assert.equal(stageOf(pet), 'baby', '最初の食事でかえる')

for (const [percent, stage] of [
  [9.9, 'baby'],
  [10, 'child'],
  [20, 'adult'],
  [30, 'ojisan'],
  [40, 'ojiisan'],
] as const) {
  assert.equal(stageOf(feed(pet, 0, 0, percent, born)), stage, `${percent}% は ${stage}`)
}

// ウンチは出力トークンだけで増え、流すと 0 に戻る。
pet = feed(pet, 0, OUTPUT_PER_POOP * 3, 15, born)
assert.equal(poopCount(pet), 3)
const dirty = feed(pet, 0, OUTPUT_PER_POOP * 6, 15, born)
assert.equal(poopCount(dirty), 9)
assert.ok(dirty.health < pet.health, '溜めすぎると健康が減る')
assert.equal(poopCount(flush(dirty)), 0, '流すと溜まりが消える')
assert.ok(flush(dirty).health > dirty.health, '流すと健康が戻る')

// 形も色もかえった時点で決まる。食べても変わらず、おじいさんだけ褪せる。
const young = traitsOf(pet)
assert.deepEqual(traitsOf({ ...pet, input: 300_000 }), young, '食べても見た目は変わらない')
assert.notEqual(traitsOf({ ...pet, percent: 42 }).color, young.color, 'おじいさんは色が褪せる')
assert.notEqual(traitsOf({ ...pet, id: 'session-2' }).color, young.color, '別の子は別の色')

// 基準の姿は、元の四分ブロックの絵にそのまま戻る。
assert.deepEqual(artLines(), [' \u2590\u259b\u2588\u2588\u2588\u259b\u2588', '\u259d\u259c\u2588\u2588\u2588\u2588\u2588\u2588\u2580', '  \u259d\u259d \u259d\u259d'])

// cells は columns * rows * 3 語を base64 にしたもの。
const columns = 40
const rows = 12
const cells = render(columns, rows, pet, newScene(), { width: columns * 2, ground: rows * 2 - 3 })
assert.equal(cells.length, Math.ceil((columns * rows * 12) / 3) * 4)
assert.ok(/^[A-Za-z0-9+/]+=*$/.test(cells))

// 餌は降って器に溜まり、Claudeっちは器まで歩いて食べる。
const world = { width: columns * 2, ground: rows * 2 - 3 }
const width = petWidth(columns, rows, pet)
const scene = newScene()
scene.x = world.width - width - 10
sprinkle(scene, world, TOKENS_PER_GRAIN * 8)
assert.equal(scene.falling.length, 8, '入力トークンが粒になって降る')
for (let i = 0; i < 200 && scene.mode !== 'eat'; i += 1) advance(scene, world, width)
assert.equal(scene.mode, 'eat', '器まで歩いて食べ始める')
assert.ok(scene.x < world.width / 2, '器のある左側に立っている')
const beforeChew = scene.food
for (let i = 0; i < 10; i += 1) advance(scene, world, width)
assert.ok(scene.food < beforeChew, '食べると器の粒が減る')

// 流すと、ウンチはトイレへ運ばれてから消える。
scene.poops = [30, 45]
startFlush(scene)
let done = false
for (let i = 0; i < 200 && !done; i += 1) done = advance(scene, world, width)
assert.ok(done, '流し終えたことを呼び手に返す')
assert.equal(scene.poops.length, 0)
assert.equal(scene.flushing, false)

// 健康が尽きると死に、生まれ変わると次の代として作り直される。
const died = new Date('2026-09-20T00:00:00Z')
let dying = { ...pet, health: 4 }
dying = feed(dying, 0, OUTPUT_PER_POOP * 20, 15, died)
assert.ok(isDead(dying), '溜めすぎると死ぬ')
assert.equal(dying.diedAt, died.toISOString())
const frozen = feed(dying, 999, 999, 15, died)
assert.deepEqual(frozen, dying, '死んだ子はもう食べない')

const next = rebirth(dying, 'session-1', died)
assert.equal(next.generation, dying.generation + 1)
assert.notEqual(next.id, dying.id, 'ひろばで前の代と別に並ぶ')
assert.equal(next.health, 100)
assert.equal(isDead(next), false)

console.log('ok')
