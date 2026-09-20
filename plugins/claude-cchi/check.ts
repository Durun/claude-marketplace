// 成長・餌・排泄の計算と、面の符号化の長さを確かめる。`npx tsx check.ts` で走る。

import assert from 'node:assert/strict'
import { artLines, bowlX, petWidth, render } from './hooks/draw.ts'
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
  learnWords,
  MAX_WORDS,
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
sprinkle(scene, world, TOKENS_PER_GRAIN * 8, bowlX(width))
assert.equal(scene.falling.length, 8, '入力トークンが粒になって降る')
for (let i = 0; i < 200 && scene.mode !== 'eat'; i += 1) advance(scene, world, width)
assert.equal(scene.mode, 'eat', '器まで歩いて食べ始める')
assert.ok(scene.x < world.width / 2, '器のある左側に立っている')
// 降りきってから数える。落ちてくる粒の方が多いと、食べていても器は増える。
for (let i = 0; i < 100 && scene.falling.length > 0; i += 1) advance(scene, world, width)
const beforeChew = scene.food
for (let i = 0; i < 10; i += 1) advance(scene, world, width)
assert.ok(scene.food < beforeChew, '食べると器の粒が減る')

// 器は口の真下に来る。体が大きくなっても、口が餌から外れない。
assert.ok(Math.abs(bowlX(width) - (scene.x + (width * 10) / 22)) <= 2, '口の下に器がある')

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

// 話し方は段階で変わる。子供はまだ一語しか出せない。
assert.equal(wordFor('egg', 'おこられた'), null)
assert.equal(wordFor('baby', 'おこられた'), null)
assert.equal(wordFor('child', 'でーた こわれた'), 'でーた')
assert.equal(wordFor('adult', 'でーた こわれた'), 'でーた こわれた')

// 覚えるのは要約した 1 文。同じ文は覚え直し、数を超えたら古いものから忘れる。
const memory = { text: 'ClickHouse の JSONEachRow は孤立サロゲートを弾く', heardFrom: null }
let learner = remember(newPet('s', '/w', born), memory)
learner = remember(learner, memory)
assert.deepEqual(learner.knowledge, [memory], '同じ文は重ねない')
for (let i = 0; i < MAX_FACTS + 3; i += 1) {
  learner = remember(learner, { text: `できごと${i}`, heardFrom: null })
}
assert.equal(learner.knowledge.length, MAX_FACTS)
assert.equal(learner.knowledge.some((m) => m.text === memory.text), false, '古いものから忘れる')

// 言えることは記憶とは別に溜まる。同じ言い回しは 1 つにまとめ、上限を超えたら古いものから忘れる。
let talker = learnWords(newPet('s', '/w', born), ['こわれた', 'なおした', ''])
talker = learnWords(talker, ['こわれた'])
assert.deepEqual(talker.words, ['こわれた', 'なおした'], '同じ言い回しと空文字は増やさない')
talker = learnWords(
  talker,
  Array.from({ length: MAX_WORDS + 5 }, (_, i) => `ことば${i}`),
)
assert.equal(talker.words.length, MAX_WORDS)
assert.equal(talker.words.includes('なおした'), false, '古い言い回しから忘れる')

// 家とひろばのどちらかにしか居ない。止まったセッションの子はずっとひろば。
const now = Date.now()
const living = { ...newPet('s', '/w', born), health: 100, input: 1, seenAt: now }
assert.equal(inPlaza(living, now), false, '動いているセッションの子は家に居る')
assert.equal(inPlaza({ ...living, away: true }, now), true, '遊びに行っている間はひろば')
assert.equal(inPlaza({ ...living, seenAt: now - STALE_MS - 1 }, now), true, '止まった子はひろば')
assert.equal(inPlaza({ ...living, health: 0 }, now), false, '死んだ子はひろばに居ない')

// 頃合いが来たら歩いて画面の外へ出かけ、また画面の外から帰ってくる。
const field = { width: columns * 2, ground: rows * 2 - 3 }
const traveller = newScene()
const walk = (scene: typeof traveller) => {
  for (let i = 0; i < field.width * 2; i += 1) advance(scene, field, 20)
}
traveller.step = traveller.tripAt
travel(traveller, field, 20)
assert.equal(traveller.mode, 'leave', '頃合いで出口へ歩き出す')
assert.equal(traveller.away, false, '歩いている間はまだ家に居る')
walk(traveller)
assert.equal(traveller.away, true, '画面の外へ抜けたらひろばに移る')
traveller.step = traveller.tripAt
travel(traveller, field, 20)
assert.equal(traveller.mode, 'arrive', '帰りは画面の外から歩いて入る')
assert.equal(traveller.away, false)
assert.ok(traveller.x > field.width, '入ってくる前は画面の外に居る')
walk(traveller)
assert.ok(traveller.x <= field.width, '歩いて家の中へ入る')

// 歩く位置はセルの境目に揃う。ずれると細かい模様がセルをまたいで揺れる。
const walker = newScene()
walker.mode = 'walk'
walker.target = field.width
for (let i = 0; i < 40; i += 1) {
  advance(walker, field, 20)
  assert.equal(walker.x % 2, 0, '歩く位置は偶数の画素に乗る')
}

// 健康が減ると顔に影が差す。体の色そのものは変わらない。
const wellField = { width: columns * 2, ground: rows * 2 - 3 }
const faceOf = (health: number) => render(columns, rows, { ...pet, health }, newScene(), wellField)
assert.equal(faceOf(100), faceOf(60), '健康があるうちは見た目が変わらない')
assert.notEqual(faceOf(60), faceOf(20), '健康が減ると顔に影が差して口がへの字になる')

// 出かけている間、家の絵に本人は居ない。
const home = render(columns, rows, pet, newScene(), { width: columns * 2, ground: rows * 2 - 3 })
const empty = render(columns, rows, pet, { ...newScene(), away: true }, {
  width: columns * 2,
  ground: rows * 2 - 3,
})
assert.notEqual(home, empty, '家から本人が消える')

console.log('ok')
