import type { EngineInterface, Register, Timer } from 'claude-code'
import { bowlX, CROWD_LIMIT, petWidth, render, renderCrowd, statusLine } from './draw.ts'
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
  hexColor,
  learnWords,
  sayText,
  traitsOf,
  wordFor,
  type Memory,
  type Say,
  type Pet,
  type Stage,
} from './pet.ts'
import {
  advance,
  excrete,
  newScene,
  setActivity,
  sprinkle,
  startFlush,
  travel,
  type Scene,
  type World,
} from './scene.ts'

const PANE = 'claude-cchi'
const GRAVE_PANE = 'claude-cchi-ohaka'
const CROWD = 'crowd'

/** ひろばの絵の高さ。 */
const CROWD_ROWS = 11
const SCREEN = 'screen'
const PLAZA_KEY = 'plaza'

/** ひろばに流れた一言の置き場。どのセッションからも読み書きする。 */
const TALK_KEY = 'plaza:talk'

/** 歩きと咀嚼が滑らかに見える速さ。 */
const FRAME_MS = 120

/** 面の高さ。Claudeっちの頭が上端に近いほど、頭上の名前が近くに出る。 */
const PANE_ROWS = 11
const MAX_COLUMNS = 72
const MIN_COLUMNS = 24

/** ひろばに残す数。古い順に落とす。 */
const PLAZA_LIMIT = 40

/** ひろばに残す一言の数。古い順に落とす。 */
const TALK_LIMIT = 20

/** ひろばへ耳を澄ます間隔。他の子の一言はこの間隔で届く。 */
const LISTEN_EVERY = 25

/** ひろばで口を開く間隔。セッションごとにコマ数がずれるので、話す順は自然に散る。 */
const TALK_IN_PLAZA_EVERY = 300

/** 一言を吹き出しに出しておく時間。他のセッションとはコマ数を共有できないので実時間で測る。 */
const BUBBLE_MS = 6000

/** 心拍を打つ間隔。これが途切れるとひろばで待つ扱いになる。 */
const HEARTBEAT_FRAMES = 100

/** ひとことを言う間隔。毎ターン喋ると会話の邪魔になる。 */
const TALK_EVERY = 3

/** 吹き出しを出しておくコマ数。 */
const SAY_FRAMES = 60

/** ウンチが 1 つ出るまでのコマ数。溜まった分を少しずつ出す。 */
const POOP_INTERVAL = 12

/** 独り言の間隔。貯めたことばから選ぶだけなので、モデルは呼ばない。 */
const MUTTER_EVERY = 240

/** 走っている道具の数。1 つでも待っていれば、Claudeっちは手持ち無沙汰になる。 */
let running = 0

let pet: Pet | null = null
let scene: Scene = newScene()
let ticker: Timer | null = null
let requestId = ''
let columns = 0
let cells = ''
let turns = 0
let sayUntil = 0
let plaza: Pet[] = []
let talk: Utterance[] = []
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

/**
 * ひろばに流れた一言。その場に居合わせた全員が聞き、それぞれの記憶に残る。
 * 聞いた側には言葉そのものと、誰の言葉かを示す名前と色だけが残る。
 */
export type Utterance = {
  /** 話した子の id。自分の声を聞き直さないために持つ。 */
  petId: string
  name: string
  color: string
  say: Say
  at: number
}

const pick = <T>(items: readonly T[]) => items[Math.floor(Math.random() * items.length)]

/** 前の版で保存された子は words を持たないか、色の無い文字列で持っている。読み出しはここを通す。 */
const wordsOf = (p: Pet): readonly Say[] =>
  (p.words ?? []).map((say) =>
    typeof say === 'string' ? [{ text: say, color: null }] : say,
  ) as readonly Say[]

/**
 * 前の版の記憶は「主語 1 語 + 述語 1 語」しかない。今の記憶は作業をそのまま要約した文なので、
 * 戻す元が無い。5 歳児の言い方としてはそのまま使えるので、言えることへ移して記憶は貯め直す。
 */
export const migrate = (p: Pet): Pet => {
  const old = (p.knowledge ?? []) as readonly (Memory & { subject?: string; predicate?: string })[]
  if (old.every((m) => typeof m.text === 'string'))
    return { ...p, heardAt: p.heardAt ?? 0, words: [...wordsOf(p)] }
  const said = old
    .filter((m) => typeof m.text !== 'string' && m.subject !== undefined)
    .map((m): Say => [{ text: `${m.subject} ${m.predicate ?? ''}`.trim(), color: null }])
  return learnWords(
    {
      ...p,
      heardAt: p.heardAt ?? 0,
      knowledge: old.filter((m) => typeof m.text === 'string'),
      words: [...wordsOf(p)],
    },
    said,
  )
}

/** 聞こえた一言を記憶の形にする。相手の頭の中までは分からないので、聞こえた言葉がその子の色のまま残る。 */
const heardOf = (u: Utterance): Memory => ({
  text: sayText(u.say),
  heardFrom: u.name,
  color: u.color,
})

/** ひろばへ一言を流す。足す前に読み直すので、その間に流れた他の子の一言を消さない。 */
const append = async ($: EngineInterface, u: Utterance) => {
  const current = ((await $.store.get(TALK_KEY)) as Utterance[] | undefined) ?? []
  talk = [...current, u].slice(-TALK_LIMIT)
  await $.store.set(TALK_KEY, talk)
}

/** まだ聞いていない一言。自分の声と、一度覚えたものは除く。 */
export const freshTalk = (talk: readonly Utterance[], me: Pet) =>
  talk.filter((u) => u.at > me.heardAt && u.petId !== me.id)

/** ひろばに流れた一言を聞き取って覚える。居合わせた子の声は全部その場で耳に入る。 */
const listen = async ($: EngineInterface) => {
  const me = pet
  if (me === null) return
  talk = ((await $.store.get(TALK_KEY)) as Utterance[] | undefined) ?? []
  const last = talk.at(-1)
  if (last === undefined) return
  const fresh = freshTalk(talk, me)
  if (fresh.length === 0) return
  pet = { ...fresh.map(heardOf).reduce(remember, me), heardAt: last.at }
  await save($)
}

/**
 * ひろばで口を開く。直前に誰かの一言が流れていれば、自分の記憶とそれを繋ぎ直して新しい言い方を作る。
 * まだ誰も話していなければ、すでに言えることから 1 つ出す。口火を切るのにモデルは呼ばない。
 */
const speak = async ($: EngineInterface) => {
  const me = pet
  if (me === null || me.name === null || isDead(me)) return
  const last = talk.at(-1)
  // 自分の声が最後なら黙っている。返すのは聞いた側の番。
  if (last?.petId === me.id) return
  const mine = pick(me.knowledge)
  const said =
    last !== undefined && mine !== undefined
      ? pick(await babble($, mine, heardOf(last)))
      : pick(wordsOf(me))
  if (said === undefined) return
  const word = wordFor(stageOf(me), said)
  if (word === null) return
  pet = learnWords(me, [said])
  await append($, {
    petId: me.id,
    name: me.name,
    color: hexColor(traitsOf(me).color),
    say: word,
    at: Date.now(),
  })
  await save($)
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
    const wasAway = scene.away
    const flushed = advance(scene, worldOf(), width)
    // 赤ちゃんのうちは家から出さない。ひろばへは育ってから行く。
    if (stageOf(pet) !== 'baby') travel(scene, worldOf(), width)
    if (scene.away !== wasAway) {
      // 出入りのたびに、ひろばの顔ぶれを取り直して区画を出し入れする。
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
      scene.poops.push(Math.max(bowlX(width) + 12, scene.x + behind - scene.facing * spread))
    }
    if (flushed) {
      pet = flush(pet)
      await save($)
      await $.ui.invalidate('ui.render')
    }
    // ひろばに居る間だけ、耳を澄まして口を開く。吹き出しは実時間で消えるので毎回描き直す。
    if (scene.away && scene.step % LISTEN_EVERY === 0) {
      await listen($)
      await $.ui.invalidate('ui.render')
    }
    if (scene.away && scene.step % TALK_IN_PLAZA_EVERY === 0) {
      await speak($)
      await $.ui.invalidate('ui.render')
    }
    // 貯めたことばから独り言を言う。寝ている間と、ひろばで立ち話をしている間は黙っている。
    if (
      sayUntil === 0 &&
      !scene.away &&
      scene.mode !== 'sleep' &&
      scene.mode !== 'doze' &&
      scene.step % MUTTER_EVERY === 0
    ) {
      const say = pick(wordsOf(pet))
      const word = say === undefined ? null : wordFor(stageOf(pet), say)
      if (word !== null) {
        pet = { ...pet, word }
        sayUntil = scene.step + SAY_FRAMES
        await $.ui.invalidate('ui.render')
      }
    }
    if (sayUntil > 0 && scene.step >= sayUntil) {
      sayUntil = 0
      pet = { ...pet, word: null }
      await $.ui.invalidate('ui.render')
    }
    await redraw($)
    // 頭上の札は Raster の外の行なので、歩いた分だけ木を組み直す。
    if (scene.mode !== 'idle' && scene.step % 2 === 0) await $.ui.invalidate('ui.render')
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

/**
 * やりとりから、時制を持たない背景・規範・性質を取り出して覚える。専門語はそのまま残す。
 * 返答だけを読むと、提案や確認待ちを確定した決まりごとと取り違える。依頼と対にして渡す。
 */
const recall = async ($: EngineInterface, p: Pet, answer: string): Promise<Memory | null> => {
  const known = p.knowledge.map((m) => m.text).join('\n')
  const messages = await $.session.messages()
  const ask =
    [...messages].reverse().find((m) => m.role === 'user' && m.text !== '')?.text.slice(0, 1000) ??
    ''
  const text = await $.model.complete({
    model: 'haiku',
    system:
      'あなたは知識の記録係。飼い主の依頼と、それへの返答を読み、そこから分かる背景・規範・性質だけを書く。' +
      'いつ読み返しても当てはまることを現在形で書く。できごと・経緯・これからやることは書かない。' +
      '「〜した」「〜する予定」のように時制を持つ文にしない。取り出せることが無ければ何も出力しない。' +
      '固有名詞・技術用語・数値・因果関係はそのまま残す。400 文字以内で、必要なだけ文を重ねてよい。' +
      '既に覚えていることと重なるなら、まだ書いていない側面を書く。前置きや箇条書きの記号を付けず、本文だけを出力する。',
    prompt: `飼い主の依頼:\n${ask}\n\n返答:\n${answer.slice(0, 4000)}\n\n既に覚えていること:\n${known || '（まだ何も知らない）'}`,
    maxTokens: 400,
  })
  const line = text.trim().replace(/\n+/g, ' ')
  return line === '' ? null : { text: line.slice(0, 400), heardFrom: null, color: null }
}

/**
 * 1 つの記憶から、いつでも当てはまる決まりごとを取り出して口に出せる形にする。
 * できごとの報告にすると、ひろばで他の子に渡せるものが残らない。
 * ひろばで誰かに教わった言葉があれば、それを混ぜた言い方も作る。
 */
const babble = async ($: EngineInterface, memory: Memory, heard?: Memory): Promise<Say[]> => {
  const borrowed = heard?.text ?? ''
  const text = await $.model.complete({
    model: 'haiku',
    system:
      'あなたは 5 歳児。渡された文から、覚えておきたいことを「A は B」の形で言う。' +
      'A には出てきた言葉をそのまま使ってよい。B にはふつうの名詞か形容詞を置く。' +
      '「めずらしい」「むずかしい」「たいへん」のような感想でもよい。' +
      'いま起きたことの報告は書かない。いつもそう言えることを現在形で書く。' +
      '例:「ねる は ウトウト」「はいく は めずらしい」「ほん は べんきょう」' +
      '数字は使わない。1 つ 16 文字以内の短い文にする。' +
      '違う言い方を 3 つ、1 行に 1 つ出力する。' +
      (borrowed === ''
        ? ''
        : `3 つのうち 1 つか 2 つは、友だちに教わった「${borrowed}」を混ぜて言う。` +
          '借りるのは教わった言葉の中の 1 語だけ。その 1 語を [ ] で囲む。文の全体を囲まない。'),
    prompt: `言い直す文:\n${memory.text}`,
    maxTokens: 96,
  })
  const color = heard?.color ?? null
  return text
    .trim()
    .split('\n')
    // 剥がすのは箇条書きの印と連番だけ。裸の数字を落とすと「5 歳児」が「歳児」になって残る。
    .map((line) => line.replace(/^[-・]\s*|^\d+[.、)]\s*/, '').replace(/[「」"']/g, '').trim())
    // 前置きと、長すぎる言い回しは捨てる。5 歳児の口から出る長さではない。
    // 「〜た」で終わる行も捨てる。できごとの報告は、他の子に渡しても使えない。
    .filter(
      (line) =>
        line !== '' && !/[:：]$/.test(line) && !/た[。！]?$/.test(line) && displayWidth(line) <= 32,
    )
    .slice(0, 3)
    .map((line) => markBorrowed(line, color))
    .filter((say) => sayText(say) !== '')
}

/** [ ] で囲まれたところに、教わった子の色を付ける。 */
export const markBorrowed = (line: string, color: string | null): Say =>
  line
    .split(/(\[[^\]]*\])/)
    .filter((part) => part !== '')
    .map((part) =>
      part.startsWith('[') && part.endsWith(']')
        ? { text: part.slice(1, -1), color }
        : { text: part, color: null },
    )
    .filter((part) => part.text !== '')

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

/**
 * ひろばの吹き出し。いま流れている一言を、話した子の真上に出す。
 * 出ていない間も同じ行数を空けておく。高さが変わると下の区画ごと描き直しになる。
 */
const chatBubble = (columns: number, crowd: readonly Pet[]): Say[] => {
  const blank: Say[] = Array.from({ length: BUBBLE_ROWS }, () => [])
  const last = talk.at(-1)
  if (last === undefined || crowd.length === 0 || Date.now() - last.at > BUBBLE_MS) return blank
  const index = crowd.findIndex((p) => p.id === last.petId)
  if (index < 0) return blank
  const lines = bubble(last.say)
  const slot = Math.floor(columns / crowd.length)
  const head = lines[0] === undefined ? '' : sayText(lines[0])
  const at = index * slot + Math.max(0, Math.round((slot - displayWidth(head)) / 2))
  return lines.map((line) => [{ text: ' '.repeat(at), color: null }, ...line])
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

/** 吹き出しの 3 行。中の行は色ごとに区切って返す。下辺の三角が Claudeっちを指す。 */
export const bubble = (say: Say): Say[] => {
  const inner = displayWidth(sayText(say)) + 2
  const tail = Math.floor(inner / 2)
  return [
    [{ text: `╭${'─'.repeat(inner)}╮`, color: null }],
    [{ text: '│ ', color: null }, ...say, { text: ' │', color: null }],
    [{ text: `╰${'─'.repeat(tail)}▽${'─'.repeat(inner - tail - 1)}╯`, color: null }],
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
    pet = stored === undefined ? newPet(sessionId, e.cwd, new Date()) : migrate(stored)
    scene = newScene()
    plaza = (((await $.store.get(PLAZA_KEY)) as Pet[] | undefined) ?? []).map(migrate)
    talk = ((await $.store.get(TALK_KEY)) as Utterance[] | undefined) ?? []
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

  // Claude が考えている間は勉強し、道具の返事を待つ間はウトウトして寝入る。
  on('turn.start', async ($, e, next) => {
    setActivity(scene, 'think')
    return next(e)
  })

  on('tool.call', async ($, e, next) => {
    running += 1
    setActivity(scene, 'wait')
    try {
      return await next(e)
    } finally {
      running -= 1
      if (running <= 0) setActivity(scene, 'think')
    }
  })

  on('turn.complete', async ($, e, next) => {
    const result = await next(e)
    if (e.agentId === undefined) setActivity(scene, 'free')
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
    if (columns > 0) sprinkle(scene, worldOf(), eaten, bowlX(petWidth(columns, PANE_ROWS, pet)))

    if (pet.name === null && after !== 'egg' && after !== 'baby') {
      pet = { ...pet, name: await nameIt($, pet) }
    }
    if (turns % TALK_EVERY === 0) {
      const memory = await recall($, pet, e.answer)
      if (memory !== null) {
        const heard = pick(pet.knowledge.filter((m) => m.heardFrom !== null))
        pet = learnWords(remember(pet, memory), await babble($, memory, heard))
        const say = pick(wordsOf(pet))
        const word = say === undefined ? null : wordFor(after, say)
        pet = { ...pet, word }
        sayUntil = word === null ? 0 : scene.step + SAY_FRAMES
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
    const sayRow = (line: Say, pad: number) =>
      Box({
        paddingLeft: pad,
        children: line.map((part) =>
          Text(part.color === null ? { children: part.text } : { children: part.text, color: part.color }),
        ),
      })
    const speech =
      scene.away || pet.word === null || sayUntil === 0
        ? []
        : bubble(pet.word).map((line) => sayRow(line, labelPad(sayText(line), width)))
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
              ...chatBubble(columns, crowd).map((line) => sayRow(line, 0)),
              Text({ children: crowdNames(columns, crowd) }),
              Raster({
                key: CROWD,
                columns,
                rows: CROWD_ROWS,
                cells: renderCrowd(columns, CROWD_ROWS, crowd, scene.step),
              }),
              Text({ children: crowd.length === 0 ? 'ひろば  まだ誰もいない。' : 'ひろば' }),
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
