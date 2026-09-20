import type { EngineInterface, Register, Timer } from 'claude-code'
import { CROWD_LIMIT, petWidth, render, renderCrowd, statusLine } from './draw.ts'
import {
  adopt,
  feed,
  flush,
  inPlaza,
  isDead,
  isStopped,
  newPet,
  poopCount,
  rebirth,
  remember,
  stageOf,
  STAGE_LABEL,
  wordFor,
  type Fact,
  type Pet,
  type Stage,
  type Utterance,
} from './pet.ts'
import {
  advance,
  BOWL_X,
  excrete,
  newScene,
  sprinkle,
  startFlush,
  travel,
  type Scene,
  type World,
} from './scene.ts'

const PANE = 'claude-cchi'
const GRAVE_PANE = 'claude-cchi-ohaka'
const CROWD = 'crowd'

/** ひろばの絵の高さ。1 匹ずつは小さいので低くてよい。 */
const CROWD_ROWS = 7
const SCREEN = 'screen'
const PLAZA_KEY = 'plaza'
const BOARD_KEY = 'board'

/** 歩きと咀嚼が滑らかに見える速さ。 */
const FRAME_MS = 120

/** 面の高さ。Claudeっちの頭が上端に近いほど、頭上の名前が近くに出る。 */
const PANE_ROWS = 11
const MAX_COLUMNS = 72
const MIN_COLUMNS = 24

/** ひろばに残す数。古い順に落とす。 */
const PLAZA_LIMIT = 40

/** 掲示板に残す言葉の数。 */
const BOARD_LIMIT = 40

/** 1 度に聞き取れる数。5 歳児なので、たくさんは覚えられない。 */
const HEAR_AT_ONCE = 2

/** 心拍を打つ間隔。これが途切れるとひろばで待つ扱いになる。 */
const HEARTBEAT_FRAMES = 100

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
let board: Utterance[] = []
let sessionId = ''

/** 面の中でどちらを見ているか。ひろばに切り替えている間、家は描かない。 */
let tab: 'home' | 'plaza' = 'home'

const key = (id: string) => `pet:${id}`

const worldOf = (): World => ({ width: columns * 2, ground: PANE_ROWS * 2 - 3 })

const save = async ($: EngineInterface) => {
  if (!pet) return
  pet = { ...pet, seenAt: Date.now(), away: scene.away }
  await $.store.set(key(sessionId), pet)
  // 名前が付くまではひろばに出さない。名前のないまま死んだ子は、お墓のために残す。
  const keep = (p: Pet) => p.name !== null || isDead(p)
  plaza = [...plaza.filter((p) => p.id !== pet?.id && keep(p)), ...(keep(pet) ? [pet] : [])].slice(
    -PLAZA_LIMIT,
  )
  await $.store.set(PLAZA_KEY, plaza)
}

/** ひろばに居る Claudeっち。止まったセッションの子と、遊びに来ている子。 */
const crowdNow = () => plaza.filter((p) => inPlaza(p, Date.now())).slice(-CROWD_LIMIT)

/** 掲示板を読み直す。他のセッションの Claudeっちが書き足しているので、都度取り直す。 */
const loadBoard = async ($: EngineInterface) => {
  board = ((await $.store.get(BOARD_KEY)) as Utterance[] | undefined) ?? []
  return board
}

const post = async ($: EngineInterface, said: Utterance) => {
  board = [...(await loadBoard($)), said].slice(-BOARD_LIMIT)
  await $.store.set(BOARD_KEY, board)
}

const redraw = async ($: EngineInterface) => {
  if (!pet || requestId === '' || columns <= 0) return
  if (tab === 'home') {
    cells = render(columns, PANE_ROWS, pet, scene, worldOf())
    await $.ui.blit({ requestId, key: SCREEN, cells })
  }
  if (tab === 'plaza' || scene.away) {
    await $.ui.blit({
      requestId,
      key: CROWD,
      cells: renderCrowd(columns, CROWD_ROWS, crowdNow(), scene.step),
    })
  }
}

const start = ($: EngineInterface) => {
  ticker?.cancel()
  ticker = $.clock.every(FRAME_MS, async () => {
    // 死んだ子は動かない。遺影は 1 度描けば足りる。
    if (!pet || columns <= 0 || isDead(pet)) return
    const width = petWidth(columns, PANE_ROWS, pet)
    const flushed = advance(scene, worldOf(), width)
    if (travel(scene)) {
      // 出入りのたびに、ひろばの顔ぶれを取り直して家の下の区画を出し入れする。
      plaza = ((await $.store.get(PLAZA_KEY)) as Pet[] | undefined) ?? plaza
      await save($)
      await $.ui.invalidate('ui.render')
    }
    if (scene.step % HEARTBEAT_FRAMES === 0) await save($)
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
      '2〜4 文字。末尾に「っち」は付けない。説明や記号を付けず、名前だけを出力する。',
    prompt: `会話:\n${recent}\n\nこの子の名前:`,
    maxTokens: 24,
  })
  const name = text.trim().split(/\s|\n/)[0]?.replace(/[「」"'。、]/g, '') ?? ''
  if (name === '') return null
  // 名前は必ず「っち」で終わる。haiku が付けてきたときは重ねない。
  return `${name.replace(/っち$/, '').slice(0, 5)}っち`
}

/** ひろばで他の子が言ったことを聞き取る。5 歳児なので 1 度に 2 つまで。 */
const hear = async ($: EngineInterface, p: Pet): Promise<Pet> => {
  const fresh = (await loadBoard($))
    .filter((u) => u.petId !== p.id && u.at > p.heardAt)
    .slice(-HEAR_AT_ONCE)
  if (fresh.length === 0) return p
  const heard = fresh.reduce(
    (acc, u) => remember(acc, { subject: u.subject, predicate: u.predicate, heardFrom: u.name }),
    p,
  )
  return { ...heard, heardAt: Math.max(...fresh.map((u) => u.at)) }
}

/**
 * 話すことを 1 つ考える。自分のセッションで見聞きしたことと、ひろばで聞いたことを合わせる。
 * 主語と述語を 1 語ずつしか持てないので、渡した文脈のほとんどは落ちる。
 */
const think = async ($: EngineInterface, p: Pet, answer: string): Promise<Fact | null> => {
  const known = p.knowledge
    .map((f) => `${f.subject} は ${f.predicate}${f.heardFrom === null ? '' : `（${f.heardFrom}から）`}`)
    .join('\n')
  const text = await $.model.complete({
    model: 'haiku',
    system:
      'あなたは 5 歳児の語彙しか持たない生き物。いま見聞きしたことと、覚えていることから、' +
      '言いたいことを 1 つだけ選び「主語|述語」の形で答える。' +
      '主語も述語も 5 文字以内のやさしい日本語にする。' +
      '「トークン|おおい」「ひろば|たのしい」のように、縦棒 1 本で区切った 1 行だけを出力する。',
    prompt: `いま見聞きしたこと:\n${answer.slice(0, 600)}\n\n覚えていること:\n${known || '（まだ何も知らない）'}`,
    maxTokens: 32,
  })
  const [subject, predicate] = (text.trim().split('\n')[0] ?? '').split('|').map((w) => w.trim())
  if (subject === undefined || predicate === undefined) return null
  if (subject === '' || predicate === '') return null
  return { subject: subject.slice(0, 6), predicate: predicate.slice(0, 6), heardFrom: null }
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

/** ひろばの名前行。renderCrowd と同じ等分で、1 匹ずつの真上に名前を置く。 */
export const crowdNames = (columns: number, pets: readonly Pet[]) => {
  if (pets.length === 0) return ''
  const slot = Math.floor(columns / pets.length)
  let line = ''
  pets.forEach((p, i) => {
    const name = p.name ?? 'なまえなし'
    const at = i * slot + Math.max(0, Math.round((slot - displayWidth(name)) / 2))
    line += ' '.repeat(Math.max(0, at - displayWidth(line))) + name
  })
  return line
}

/** 遺影に添える一行。生まれてから死ぬまでと、どこまで育ったか。 */
const epitaph = (pet: Pet) => {
  const day = (iso: string) => iso.slice(0, 10).replace(/-/g, '/')
  const who = pet.name ?? 'なまえなし'
  const span = pet.diedAt === null ? day(pet.born) : `${day(pet.born)} - ${day(pet.diedAt)}`
  return `${who}  ${STAGE_LABEL[stageOf(pet)]}まで育った  ${span}`
}

/** 吹き出しの行数。出ていない間もこの高さを空けておく。 */
export const BUBBLE_ROWS = 3

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
        'Claudeっちを育てる。/claude-cchi で面を開き、hiroba で生きている子、ohaka で眠った子を見る。',
    })
    sessionId = await $.session.id()
    const stored = (await $.store.get(key(sessionId))) as Pet | undefined
    pet = stored ?? newPet(sessionId, e.cwd, new Date())
    scene = newScene()
    plaza = ((await $.store.get(PLAZA_KEY)) as Pet[] | undefined) ?? []
    await loadBoard($)
    if (e.isInteractive) {
      await save($)
      await $.ui.open({ id: PANE, title: 'Claudeっち' })
      start($)
    }
    return next(e)
  })

  on('command.run', { command: 'claude-cchi' }, async ($, e) => {
    const sub = e.args.trim()
    if (sub === 'hiroba' || sub === 'ohaka') {
      plaza = ((await $.store.get(PLAZA_KEY)) as Pet[] | undefined) ?? plaza
      await loadBoard($)
      const grave = sub === 'ohaka'
      const count = plaza.filter((p) => isDead(p) === grave).length
      if (grave) {
        await $.ui.open({ id: GRAVE_PANE, title: 'お墓' })
        return { text: `お墓には ${count} 匹が眠っている。` }
      }
      tab = 'plaza'
      await $.ui.open({ id: PANE, title: 'Claudeっち' })
      start($)
      return { text: `ひろばには ${count} 匹いる。` }
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
    // input_tokens は未キャッシュ分だけなので、キャッシュの読み書きも餌に数える。
    // 会話が続くほど大半はキャッシュ読みになり、これを外すと餌がほとんど降らない。
    const eaten =
      e.usage.input_tokens + e.usage.cache_read_input_tokens + e.usage.cache_creation_input_tokens
    pet = feed(pet, eaten, e.usage.output_tokens, usage.context.percent ?? 0, new Date())
    if (isDead(pet)) {
      await save($)
      await redraw($)
      await $.ui.invalidate('ui.render')
      return result
    }
    const after = stageOf(pet)
    turns += 1

    // 食べた分を器に降らせる。ウンチは時計が 1 つずつ出す。
    if (columns > 0) sprinkle(scene, worldOf(), eaten)

    if (pet.name === null && after !== 'egg' && after !== 'baby') {
      pet = { ...pet, name: await nameIt($, pet) }
    }
    if (turns % TALK_EVERY === 0) {
      pet = await hear($, pet)
      const fact = await think($, pet, e.answer)
      if (fact !== null) {
        pet = remember(pet, fact)
        const word = wordFor(after, fact)
        pet = { ...pet, word }
        sayUntil = word === null ? 0 : scene.step + SAY_FRAMES
        // 言ったことはひろばに残り、他の Claudeっちが聞く。
        if (word !== null && pet.name !== null) {
          await post($, {
            petId: pet.id,
            name: pet.name,
            subject: fact.subject,
            predicate: fact.predicate,
            at: Date.now(),
          })
        }
      }
    }

    await save($)
    await redraw($)
    await $.ui.invalidate('ui.render')
    return result
  })

  on('ui.render', { component: 'Pane' }, async ($, e, next) => {
    if (e.requestId !== PANE && e.requestId !== GRAVE_PANE) return next(e)

    // お墓は面の中に置かず、呼ばれたときだけ別に開く。
    if (e.requestId === GRAVE_PANE) {
      const { Box, Text } = await $.ui.resolve(e)
      const here = plaza.filter(isDead)
      if (here.length === 0) {
        return Box({ children: [Text({ children: 'まだ誰も眠っていない。' })] })
      }
      return Box({
        flexDirection: 'column',
        children: [
          Text({ children: `お墓  ${here.length} 匹` }),
          ...[...here].reverse().map((p) => Text({ children: epitaph(p) })),
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

    if (isDead(pet)) {
      return Box({
        flexDirection: 'column',
        children: [
          Raster({ key: SCREEN, columns, rows: PANE_ROWS, cells }),
          Text({ children: epitaph(pet) }),
          Button({
            key: 'rebirth',
            label: '生まれ変わる',
            plain: true,
            onPress: async () => {
              if (pet === null) return
              pet = rebirth(pet, sessionId, new Date())
              scene = newScene()
              turns = 0
              sayUntil = 0
              await save($)
              start($)
              await $.ui.invalidate('ui.render')
            },
          }),
        ],
      })
    }

    const width = petWidth(columns, PANE_ROWS, pet)
    // ひろばへ行っている間、頭上の札は家に居ないので出さない。
    const speech =
      scene.away || pet.word === null || sayUntil === 0
        ? []
        : bubble(pet.word).map((line) =>
            Box({ paddingLeft: labelPad(line, width), children: [Text({ children: line })] }),
          )
    // 吹き出しの出入りで面の高さが変わると、下の区画ごと描き直しになる。空でも同じ行数を占める。
    const above = [Box({ height: BUBBLE_ROWS - speech.length }), ...speech]
    if (pet.name !== null) {
      above.push(
        Box({ paddingLeft: labelPad(pet.name, width), children: [Text({ children: pet.name })] }),
      )
    }

    const rule = () => Text({ children: '\u2500'.repeat(Math.max(4, columns - 2)) })
    const crowd = crowdNow()
    // 卵のうちだけ、生まれるのをやめてひろばの子を引き取れる。
    // 相手は飼い主のセッションが止まった子に限る。遊びに来ているだけの子は元の家へ帰る。
    const home = pet
    const adoptable =
      stageOf(home) === 'egg'
        ? crowd.filter((p) => p.id !== home.id && isStopped(p, Date.now()))
        : []
    const dirty = poopCount(pet)
    const tabButton = (id: 'home' | 'plaza', label: string) =>
      Button({
        key: `tab:${id}`,
        label: tab === id ? `[${label}]` : ` ${label} `,
        plain: true,
        onPress: async () => {
          tab = id
          cells = ''
          await $.ui.invalidate('ui.render')
        },
      })

    // 上から順に、切り替え・ステータス・操作・家・ひろば。行数は切り替えでしか変わらない。
    return Box({
      flexDirection: 'column',
      children: [
        Box({
          flexDirection: 'row',
          gap: 1,
          children: [
            tabButton('home', 'おうち'),
            Text({ children: '|' }),
            tabButton('plaza', 'ひろば'),
          ],
        }),
        Text({ children: statusLine(pet) }),
        Button({
          key: 'flush',
          label: dirty > 0 ? `流す (${dirty})` : '流す',
          plain: true,
          onPress: () => {
            startFlush(scene)
          },
        }),
        ...(tab === 'home'
          ? [rule(), ...above, Raster({ key: SCREEN, columns, rows: PANE_ROWS, cells })]
          : []),
        // おうちを見ていて本人も家に居るなら、ひろばは出さない。遊びに行った先は見える。
        ...(tab === 'home' && !scene.away
          ? []
          : [
              rule(),
              Text({ children: crowdNames(columns, crowd) }),
              Raster({
                key: CROWD,
                columns,
                rows: CROWD_ROWS,
                cells: renderCrowd(columns, CROWD_ROWS, crowd, scene.step),
              }),
              Text({ children: crowd.length === 0 ? 'ひろば  まだ誰もいない。' : 'ひろば' }),
              ...board
                .slice(-3)
                .reverse()
                .map((u) => Text({ children: `${u.name}: 「${u.subject} は ${u.predicate}」` })),
              // 卵のうちだけ、生まれるのをやめてひろばの子を引き取れる。
              // 相手は飼い主のセッションが止まった子に限る。遊びに来ているだけの子は元の家へ帰る。
              ...adoptable.map((p) =>
                Button({
                  key: `adopt:${p.id}`,
                  label: `${p.name ?? 'なまえなし'} を引き継ぐ`,
                  plain: true,
                  onPress: async () => {
                    pet = adopt(p, home.cwd, new Date())
                    scene = newScene()
                    turns = 0
                    sayUntil = 0
                    cells = ''
                    tab = 'home'
                    await save($)
                    start($)
                    await $.ui.invalidate('ui.render')
                  },
                }),
              ),
            ]),
        rule(),
      ],
    })
  })

  // 次のターンで会話が流れても面は閉じない。Claudeっちは常駐する。
}
