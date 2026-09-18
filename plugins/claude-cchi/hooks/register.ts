import type { EngineInterface, Register, Timer } from 'claude-code'
import { petWidth, render, statusLine } from './draw.ts'
import { feed, flush, newPet, poopCount, stageOf, STAGE_LABEL, type Pet, type Stage } from './pet.ts'
import {
  advance,
  BOWL_X,
  excrete,
  newScene,
  sprinkle,
  startFlush,
  type Scene,
  type World,
} from './scene.ts'

const PANE = 'claude-cchi'
const PLAZA_PANE = 'claude-cchi-hiroba'
const SCREEN = 'screen'
const PLAZA_KEY = 'plaza'

/** 歩きと咀嚼が滑らかに見える速さ。 */
const FRAME_MS = 120

/** 面の高さ。Claudeっちの頭が上端に近いほど、頭上の名前が近くに出る。 */
const PANE_ROWS = 11
const MAX_COLUMNS = 72
const MIN_COLUMNS = 24

/** ひろばに残す数。古い順に落とす。 */
const PLAZA_LIMIT = 40

/** ひとことを言う間隔。毎ターン喋ると会話の邪魔になる。 */
const TALK_EVERY = 3

/** 吹き出しを出しておくコマ数。 */
const SAY_FRAMES = 60

/** ウンチが 1 つ出るまでのコマ数。溜まった分を少しずつ出す。 */
const POOP_INTERVAL = 12

let pet: Pet | null = null
let scene: Scene = newScene()
let ticker: Timer | null = null
let requestId = ''
let columns = 0
let cells = ''
let turns = 0
let sayUntil = 0
let plaza: Pet[] = []

const key = (id: string) => `pet:${id}`

const worldOf = (): World => ({ width: columns * 2, ground: PANE_ROWS * 2 - 3 })

const save = async ($: EngineInterface) => {
  if (!pet) return
  await $.store.set(key(pet.id), pet)
  plaza = [...plaza.filter((p) => p.id !== pet?.id), pet].slice(-PLAZA_LIMIT)
  await $.store.set(PLAZA_KEY, plaza)
}

const redraw = async ($: EngineInterface) => {
  if (!pet || requestId === '' || columns <= 0) return
  cells = render(columns, PANE_ROWS, pet, scene, worldOf())
  await $.ui.blit({ requestId, key: SCREEN, cells })
}

const start = ($: EngineInterface) => {
  ticker?.cancel()
  ticker = $.clock.every(FRAME_MS, async () => {
    if (!pet || columns <= 0) return
    const width = petWidth(columns, PANE_ROWS, pet)
    const flushed = advance(scene, worldOf(), width)
    // 溜まった分は一度に出さず、1 つずつしゃがんで出す。
    if (!scene.flushing && scene.poops.length < poopCount(pet) && scene.step % POOP_INTERVAL === 0) {
      excrete(scene)
      // 足元の後ろへ、少しずつずらして落とす。器の上には置かない。
      const behind = scene.facing === 1 ? -5 : width + 5
      const spread = (scene.poops.length % 3) * 8
      scene.poops.push(Math.max(BOWL_X + 12, scene.x + behind - scene.facing * spread))
    }
    if (flushed) {
      pet = flush(pet)
      await save($)
      await $.ui.invalidate('ui.render')
    }
    if (sayUntil > 0 && scene.step >= sayUntil) {
      sayUntil = 0
      pet = { ...pet, word: null }
      await $.ui.invalidate('ui.render')
    }
    await redraw($)
    // 頭上の札は Raster の外の行なので、歩いた分だけ木を組み直す。
    if (scene.mode === 'walk' && scene.step % 2 === 0) await $.ui.invalidate('ui.render')
  })
}

/** 会話の中身から 1 語の名前を付ける。子供になった一度だけ呼ぶ。 */
const nameIt = async ($: EngineInterface, p: Pet) => {
  const messages = await $.session.messages()
  const recent = messages
    .filter((m) => m.role === 'user' && m.text !== '')
    .slice(-6)
    .map((m) => m.text.slice(0, 200))
    .join('\n')
  const text = await $.model.complete({
    model: 'haiku',
    system:
      'あなたは育成ゲームの命名係。会話の話題にちなんだ、かわいい日本語の名前を 1 つだけ答える。' +
      '3〜6 文字。説明や記号を付けず、名前だけを出力する。',
    prompt: `会話:\n${recent}\n\nこの子の名前:`,
    maxTokens: 24,
  })
  const name = text.trim().split(/\s|\n/)[0]?.replace(/[「」"'。、]/g, '') ?? ''
  return name === '' ? null : name.slice(0, 8)
}

/** 段階に応じたひとこと。子供は単語、大人から先は文。 */
const talk = async ($: EngineInterface, p: Pet, stage: Stage, answer: string) => {
  if (stage === 'egg' || stage === 'baby') return null
  const asWord = stage === 'child'
  const persona =
    stage === 'ojisan'
      ? 'くだけた口調で、少しおせっかいに'
      : stage === 'ojiisan'
        ? 'ゆっくりとした口調で、昔話めかして'
        : '素直に'
  const text = await $.model.complete({
    model: 'haiku',
    system:
      `あなたは ${p.name ?? '名無し'} という育成ゲームの生き物。${STAGE_LABEL[stage]}。` +
      (asWord
        ? '覚えたての単語を 1 つだけ、たどたどしく言う。2〜6 文字。'
        : `${persona}、20 文字以内で一言だけ言う。`) +
      '記号や引用符を付けず、セリフだけを出力する。',
    prompt: `いま飼い主はこう言った:\n${answer.slice(0, 400)}`,
    maxTokens: 40,
  })
  const word = text.trim().split('\n')[0]?.replace(/^[「"']|[」"']$/g, '') ?? ''
  return word === '' ? null : word.slice(0, asWord ? 8 : 24)
}

/** 端末で 2 桁を使う文字の範囲。罫線や図形はどちらとも取れるので 1 桁に数える。 */
const WIDE_RANGES: readonly (readonly [number, number])[] = [
  [0x1100, 0x115f],
  [0x2e80, 0xa4cf],
  [0xac00, 0xd7a3],
  [0xf900, 0xfaff],
  [0xfe30, 0xfe6f],
  [0xff00, 0xff60],
  [0xffe0, 0xffe6],
]

/** 端末での見た目の幅。吹き出しの枠を中身に合わせるのに使う。 */
export const displayWidth = (text: string) =>
  [...text].reduce((w, ch) => {
    const code = ch.codePointAt(0) ?? 0
    return w + (WIDE_RANGES.some(([lo, hi]) => code >= lo && code <= hi) ? 2 : 1)
  }, 0)

/** 札や吹き出しを Claudeっちの真上に置くための左余白。 */
const labelPad = (text: string, width: number) =>
  Math.max(0, Math.round((scene.x + width / 2) / 2) - Math.round(displayWidth(text) / 2))

/** 吹き出しの 3 行。下辺の三角が Claudeっちを指す。 */
export const bubble = (word: string) => {
  const inner = displayWidth(word) + 2
  const tail = Math.floor(inner / 2)
  return [
    `╭${'─'.repeat(inner)}╮`,
    `│ ${word} │`,
    `╰${'─'.repeat(tail)}▽${'─'.repeat(inner - tail - 1)}╯`,
  ]
}

export const register: Register = (on) => {
  on('session.start', async ($, e, next) => {
    await $.command.register({
      name: 'claude-cchi',
      description:
        'Claudeっちを育てる。/claude-cchi で面を開き、/claude-cchi hiroba でこれまでの子を見る。',
    })
    const id = await $.session.id()
    const stored = (await $.store.get(key(id))) as Pet | undefined
    pet = stored ?? newPet(id, e.cwd, new Date())
    scene = newScene()
    plaza = ((await $.store.get(PLAZA_KEY)) as Pet[] | undefined) ?? []
    if (e.isInteractive) {
      await save($)
      await $.ui.open({ id: PANE, title: 'Claudeっち' })
      start($)
    }
    return next(e)
  })

  on('command.run', { command: 'claude-cchi' }, async ($, e) => {
    if (e.args.trim() === 'hiroba') {
      await $.ui.open({ id: PLAZA_PANE, title: 'ひろば' })
      return { text: `ひろばを開いた。これまでの Claudeっちは ${plaza.length} 匹。` }
    }
    await $.ui.open({ id: PANE, title: 'Claudeっち' })
    start($)
    return { text: pet ? statusLine(pet) : 'まだ卵もない。' }
  })

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    // サブエージェントのターンは飼い主との会話ではないので数えない。
    if (!pet || e.agentId !== undefined || e.usage === undefined) return result

    const before = stageOf(pet)
    const usage = await $.session.usage()
    pet = feed(pet, e.usage.input_tokens, e.usage.output_tokens, usage.context.percent ?? 0)
    const after = stageOf(pet)
    turns += 1

    // 食べた分を器に降らせる。ウンチは時計が 1 つずつ出す。
    if (columns > 0) sprinkle(scene, worldOf(), e.usage.input_tokens)

    if (pet.name === null && after !== 'egg' && after !== 'baby') {
      pet = { ...pet, name: await nameIt($, pet) }
    }
    if (turns % TALK_EVERY === 0) {
      const word = await talk($, pet, after, e.answer)
      pet = { ...pet, word }
      sayUntil = word === null ? 0 : scene.step + SAY_FRAMES
    }

    await save($)
    await redraw($)
    await $.ui.invalidate('ui.render')
    return result
  })

  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== PANE && e.requestId !== PLAZA_PANE) return next(e)

    // ひろばは絵を動かさず、これまでの子を一覧にする。
    if (e.requestId === PLAZA_PANE) {
      const { Box, Text } = await $.ui.resolve(e)
      if (plaza.length === 0) return Box({ children: [Text({ children: 'まだ誰もいない。' })] })
      return Box({
        flexDirection: 'column',
        children: [
          Text({ children: `ひろば  ${plaza.length} 匹` }),
          ...[...plaza].reverse().map((p) =>
            Text({
              children: `${p.name ?? 'なまえなし'}  ${STAGE_LABEL[stageOf(p)]}  健康 ${p.health}  ${p.cwd.split('/').pop() ?? ''}`,
            }),
          ),
        ],
      })
    }

    if (pet === null) return next(e)
    if (e.surface !== 'terminal') {
      const { Box, Text } = await $.ui.resolve(e)
      return Box({ flexDirection: 'column', children: [Text({ children: statusLine(pet) })] })
    }
    const { Box, Text, Button, Raster } = await $.ui.resolve(e)

    requestId = e.requestId
    const nextColumns = Math.max(MIN_COLUMNS, Math.min(e.props.bodyColumns, MAX_COLUMNS))
    if (nextColumns !== columns || cells === '') {
      columns = nextColumns
      cells = render(columns, PANE_ROWS, pet, scene, worldOf())
    }

    const width = petWidth(columns, PANE_ROWS, pet)
    const above =
      pet.word !== null && sayUntil > 0
        ? bubble(pet.word).map((line) =>
            Box({ paddingLeft: labelPad(line, width), children: [Text({ children: line })] }),
          )
        : []
    if (pet.name !== null) {
      above.push(
        Box({ paddingLeft: labelPad(pet.name, width), children: [Text({ children: pet.name })] }),
      )
    }

    const dirty = poopCount(pet)
    return Box({
      flexDirection: 'column',
      children: [
        ...above,
        Raster({ key: SCREEN, columns, rows: PANE_ROWS, cells }),
        Text({ children: statusLine(pet) }),
        Box({
          flexDirection: 'row',
          gap: 2,
          children: [
            Button({
              key: 'flush',
              label: dirty > 0 ? `流す (${dirty})` : '流す',
              plain: true,
              onPress: () => {
                startFlush(scene)
              },
            }),
            Button({
              key: 'hiroba',
              label: 'ひろば',
              plain: true,
              onPress: async () => {
                await $.ui.open({ id: PLAZA_PANE, title: 'ひろば' })
              },
            }),
          ],
        }),
      ],
    })
  })

  // 次のターンで会話が流れても面は閉じない。Claudeっちは常駐する。
}
