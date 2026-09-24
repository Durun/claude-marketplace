// 成長・餌・排泄の計算と、面の符号化の長さを確かめる。`npx tsx check.ts` で走る。

import assert from 'node:assert/strict'
import { artLines, bowlX, petWidth, render } from './hooks/draw.ts'
import {
  clipSay,
  crowdNames,
  displayWidth,
  fitPad,
  freshTalk,
  hasContent,
  markBorrowed,
  migrate,
  strip,
  type Utterance,
} from './hooks/register.ts'
import { gloomyOf, initial, lookOf, step, TICKS_PER_HEALTH, withMood } from './hooks/run.ts'
import { render as renderCourse } from './hooks/course.ts'
import {
  feed,
  flush,
  heal,
  isDead,
  isSilent,
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
  hexColor,
  learnWords,
  MAX_WORDS,
  sayText,
  wordFor,
} from './hooks/pet.ts'
import {
  advance,
  comeHome,
  newScene,
  runOff,
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
const said = [{ text: 'でーた こわれた', color: null }]
assert.equal(wordFor('egg', said), null)
assert.equal(wordFor('baby', said), null)
assert.deepEqual(wordFor('child', said), [{ text: 'でーた', color: null }])
// 子供の一語には助詞を付けない。
assert.deepEqual(wordFor('child', [{ text: 'ほんを ひらいたら べんきょうする', color: null }]), [
  { text: 'ほん', color: null },
])
assert.deepEqual(wordFor('child', [{ text: 'はが いたい', color: null }]), [
  { text: 'はが', color: null },
])
assert.deepEqual(wordFor('adult', said), said)

// 覚えるのは要約した文。同じ文は覚え直し、数を超えたら古いものから忘れる。
const memory = {
  text: 'ClickHouse の JSONEachRow は孤立サロゲートを列の位置で弾く。String 列は通り、最上位の値は Code 25 になる。',
  heardFrom: null,
  color: null,
}
let learner = remember(newPet('s', '/w', born), memory)
learner = remember(learner, memory)
assert.deepEqual(learner.knowledge, [memory], '同じ文は重ねない')
for (let i = 0; i < MAX_FACTS + 3; i += 1) {
  learner = remember(learner, { text: `できごと${i}`, heardFrom: null, color: null })
}
assert.equal(learner.knowledge.length, MAX_FACTS)
assert.equal(learner.knowledge.some((m) => m.text === memory.text), false, '古いものから忘れる')

// 聞いた言葉には、聞かせてくれた子の色が乗る。
const heard = { text: 'おそかった', heardFrom: 'ひろばっち', color: '#7fc8a9' }
assert.equal(remember(learner, heard).knowledge.at(-1)?.color, '#7fc8a9')
assert.equal(hexColor(0x7fc8a9), '#7fc8a9')

// 言えることは記憶とは別に溜まる。同じ言い回しは 1 つにまとめ、上限を超えたら古いものから忘れる。
// 他の子から借りた言葉は、その子の色を付けたまま持つ。
const borrowed = [
  { text: 'でーたが ', color: null },
  { text: 'おそかった', color: '#7fc8a9' },
]
let talker = learnWords(newPet('s', '/w', born), [borrowed, [{ text: 'なおした', color: null }], []])
talker = learnWords(talker, [borrowed])
assert.equal(talker.words.length, 2, '同じ言い回しと空の言い回しは増やさない')
assert.equal(talker.words[0]?.[1]?.color, '#7fc8a9', '借りた言葉は相手の色のまま')
assert.equal(sayText(borrowed), 'でーたが おそかった')
talker = learnWords(
  talker,
  Array.from({ length: MAX_WORDS + 5 }, (_, i) => [{ text: `ことば${i}`, color: null }]),
)
assert.equal(talker.words.length, MAX_WORDS)
assert.equal(talker.words.some((say) => sayText(say) === 'なおした'), false, '古い言い回しから忘れる')

// 教わった言葉は [ ] で囲んで返ってくる。囲みの中だけに相手の色を付ける。
assert.deepEqual(markBorrowed('でーたが [おそかった]', '#7fc8a9'), [
  { text: 'でーたが ', color: null },
  { text: 'おそかった', color: '#7fc8a9' },
])
assert.deepEqual(markBorrowed('こわれた', '#7fc8a9'), [{ text: 'こわれた', color: null }])

// モデルが付けてくる飾りは、記憶に残す前に落とす。
assert.equal(strip('knowledge: 広場での会話は 4 段で成り立つ'), '広場での会話は 4 段で成り立つ')
assert.equal(strip('**1つめ**'), '1つめ')
assert.equal(strip('`hooks/draw.ts` の描画順を見る'), 'hooks/draw.ts の描画順を見る')
assert.equal(strip('ClickHouse の識別子は case-sensitive'), 'ClickHouse の識別子は case-sensitive')

// 書くことが無かったという返事は、記憶にも言えることにもしない。
assert.equal(hasContent('-'), false)
assert.equal(hasContent('\\-'), false, 'マークダウンで逃した印も捨てる')
assert.equal(hasContent(''), false)
assert.equal(hasContent('ひろば は にぎやか'), true)

// ひろばの一言は、自分の声と、一度覚えたものを除いて耳に入る。
{
  const listener = { ...newPet('s', '/w', born), heardAt: 100 }
  const at = (petId: string, at: number): Utterance => ({
    petId,
    name: 'よそっち',
    color: '#7fc8a9',
    say: [{ text: `ことば${at}`, color: null }],
    at,
  })
  const heardTalk = [at('other', 100), at(listener.id, 150), at('other', 200)]
  assert.deepEqual(
    freshTalk(heardTalk, listener).map((u) => u.at),
    [200],
    '覚えた一言と自分の声は聞き直さない',
  )
}

// 前の版の記憶は言えることへ移し、記憶は空から貯め直す。
{
  const old = {
    ...newPet('s', '/w', born),
    knowledge: [{ subject: 'push', predicate: 'まってる', heardFrom: null }],
  } as unknown as Parameters<typeof migrate>[0]
  const moved = migrate(old)
  assert.deepEqual(moved.knowledge, [], '戻せない記憶は残さない')
  assert.deepEqual(moved.words, [[{ text: 'push まってる', color: null }]], '言えることへ移す')
  assert.deepEqual(migrate(moved), moved, '移した後は変わらない')
}

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

// 遊びは走ったぶんだけ健康を戻す。柱に当たるとそこで止まる。
const runner = { ...pet, health: 40 }
assert.equal(heal(runner, 5).health, 45, '走った分だけ健康が戻る')
assert.equal(heal({ ...runner, health: 98 }, 5).health, 100, '健康は満杯を超えない')
assert.equal(heal({ ...runner, health: 0 }, 5).health, 0, '死んだ子は走れない')

const look = lookOf(runner)
let run = initial(look)
for (let i = 0; i < TICKS_PER_HEALTH; i += 1) run = step(run, false)
assert.equal(run.healed, 1, `${TICKS_PER_HEALTH} コマ走ると健康が 1 戻る`)
assert.ok(!run.over, '始めの柱はまだ遠い')

// 跳ばずに走り続ければ、いつか柱に当たって止まる。
let crashed = initial(look)
for (let i = 0; i < 400 && !crashed.over; i += 1) crashed = step(crashed, false)
assert.ok(crashed.over, '跳ばなければ柱に当たる')
const stopped = step(crashed, true)
assert.equal(stopped.score, crashed.score, '当たった後は進まない')
assert.equal(stopped.healed, crashed.healed, '当たった後は健康も戻らない')
assert.equal(stopped.ticks, crashed.ticks + 1, '当たった後もコマは進む。カメラだけが動き続ける')

// 走る面の cells は Raster が読める長さで出る。姿が違えば絵も違う。
const courseCells = renderCourse(columns, rows, run)
assert.equal(Buffer.from(courseCells, 'base64').length, columns * rows * 3 * 4)
const other = { ...run, look: { ...look, color: 0x3366ff, eye: (look.eye + 1) % 4 } }
assert.notEqual(renderCourse(columns, rows, other), courseCells, '目と色が違えば別の子に見える')

// 顔は家の面と同じものが出る。まばたきも、具合の悪いときのへの字の口も走りに付いてくる。
// 望遠の真横では顔が数画素しかないので、カメラが寄り切ったところで見比べる。
// 口は 1 画素ほどしかないので、顔が潰れない広さで見比べる。
const CLOSE_COLUMNS = 120
const CLOSE_ROWS = 24
const close = { ...run, depth: 1 }
const closeCells = renderCourse(CLOSE_COLUMNS, CLOSE_ROWS, close)
assert.notEqual(
  renderCourse(CLOSE_COLUMNS, CLOSE_ROWS, { ...close, look: { ...look, blink: true } }),
  closeCells,
  'まばたきすると目が閉じる',
)
assert.equal(look.gloomy, true, '健康 40 の子は具合が悪い')
assert.notEqual(
  renderCourse(CLOSE_COLUMNS, CLOSE_ROWS, { ...close, look: { ...look, gloomy: false } }),
  closeCells,
  '具合が悪いと口がへの字になる',
)
assert.equal(gloomyOf({ ...pet, health: 40 }), true)
assert.equal(gloomyOf({ ...pet, health: 60 }), false)
assert.equal(withMood(run, runner), run, '具合が変わらなければ姿は組み直さない')
assert.equal(
  withMood(run, { ...pet, health: 100 }).look.gloomy,
  false,
  '走って健康が戻ると口が直る',
)

// 面の端に立っていても、札と吹き出しは面からはみ出さない。はみ出すと折り返して下の区画がずれる。
assert.equal(fitPad(60, 'なまえっち', 40), 30, '右端に寄せても中身のぶんは残す')
assert.equal(fitPad(-4, 'なまえっち', 40), 0, '左端より外には置かない')
assert.equal(fitPad(3, 'x'.repeat(50), 40), 0, '面より広い中身は端から置く')
assert.ok(
  displayWidth(sayText(clipSay([{ text: 'ほんはべんきょう', color: null }], 6))) <= 6,
  '吹き出しの中身は面の幅まで切り詰める',
)
const named = [
  { ...pet, id: 'a', name: 'ながいなまえっち' },
  { ...pet, id: 'b', name: 'ながいなまえっち' },
]
assert.ok(displayWidth(crowdNames(24, named)) <= 24, 'ひろばの名前行は面からはみ出さない')

// [ あそぶ ] を押すと、面の外まで走ってから遊びが始まる。
const player = newScene()
runOff(player, field, 20)
assert.equal(player.mode, 'dash', '押した直後は走り出す')
for (let i = 0; i < 200 && player.mode === 'dash'; i += 1) advance(player, field, 20)
assert.equal(player.mode, 'gone', '面の外まで走り切ると遊びが始まる')
assert.ok(player.x > field.width, '遊んでいる間は家に居ない')
assert.equal(player.away, false, '走る面はひろばではない')
const before = { ...player }
advance(player, field, 20)
assert.equal(player.mode, 'gone', '遊んでいる間は家のことをしない')
assert.equal(player.x, before.x)
comeHome(player, field, 20)
assert.equal(player.mode, 'arrive', '遊びをやめると歩いて帰ってくる')
for (let i = 0; i < 200 && player.mode === 'arrive'; i += 1) advance(player, field, 20)
assert.ok(player.x <= field.width, '家の中まで戻る')

console.log('ok')

// 喋らない子はアーカイブの対象。ことばを 1 つでも持つか、死んでいれば対象外。
assert.ok(isSilent({ ...pet, health: 50, words: [] }))
assert.ok(!isSilent(learnWords({ ...pet, health: 50, words: [] }, [[{ text: 'ほん は べんきょう', color: null }]])))
assert.ok(!isSilent({ ...pet, health: 0, words: [] }), '死んだ子はお墓の側')
