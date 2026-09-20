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
  inPlaza,
  MAX_FACTS,
  rebirth,
  remember,
  stageOf,
  STALE_MS,
  traitsOf,
  wordFor,
} from './hooks/pet.ts'
import {
  advance,
  newScene,
  sprinkle,
  startFlush,
  TOKENS_PER_GRAIN,
  travel,
} from './hooks/scene.ts'

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

// 話し方は段階で変わる。子供は単語だけ、大人から先は「主語 は 述語」。
const fact = { subject: 'くるま', predicate: 'はやい', heardFrom: null }
assert.equal(wordFor('egg', fact), null)
assert.equal(wordFor('baby', fact), null)
assert.equal(wordFor('child', fact), 'くるま')
assert.equal(wordFor('adult', fact), 'くるま は はやい')
assert.equal(wordFor('ojiisan', fact), 'くるま は はやい')

// 同じ主語は覚え直し、覚えられる数を超えたら古いものから忘れる。
let learner = remember(newPet('s', '/w', born), fact)
learner = remember(learner, { ...fact, predicate: 'おそい' })
assert.deepEqual(learner.knowledge, [{ ...fact, predicate: 'おそい' }], '同じ主語は上書きする')
for (let i = 0; i < MAX_FACTS + 3; i += 1) {
  learner = remember(learner, { subject: `もの${i}`, predicate: 'ある', heardFrom: null })
}
assert.equal(learner.knowledge.length, MAX_FACTS)
assert.equal(learner.knowledge.some((f) => f.subject === 'くるま'), false, '古いものから忘れる')

// 家とひろばのどちらかにしか居ない。止まったセッションの子はずっとひろば。
const now = Date.now()
const living = { ...newPet('s', '/w', born), health: 100, input: 1, seenAt: now }
assert.equal(inPlaza(living, now), false, '動いているセッションの子は家に居る')
assert.equal(inPlaza({ ...living, away: true }, now), true, '遊びに行っている間はひろば')
assert.equal(inPlaza({ ...living, seenAt: now - STALE_MS - 1 }, now), true, '止まった子はひろば')
assert.equal(inPlaza({ ...living, health: 0 }, now), false, '死んだ子はひろばに居ない')

// 頃合いが来たら出かけ、また帰ってくる。
const traveller = newScene()
assert.equal(traveller.away, false)
traveller.step = traveller.tripAt
assert.equal(travel(traveller), true)
assert.equal(traveller.away, true, '頃合いでひろばへ出かける')
traveller.step = traveller.tripAt
assert.equal(travel(traveller), true)
assert.equal(traveller.away, false, 'また家へ帰る')

// 出かけている間、家の絵に本人は居ない。
const home = render(columns, rows, pet, newScene(), { width: columns * 2, ground: rows * 2 - 3 })
const empty = render(columns, rows, pet, { ...newScene(), away: true }, {
  width: columns * 2,
  ground: rows * 2 - 3,
})
assert.notEqual(home, empty, '家から本人が消える')

console.log('ok')
