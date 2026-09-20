// Claudeっち 1 匹分の状態と、そこから決まる成長段階・個性。
// 描画とフックから切り離してあるので、check.ts でそのまま確かめられる。

export type Stage = 'egg' | 'baby' | 'child' | 'adult' | 'ojisan' | 'ojiisan'

/**
 * Claudeっちが覚えた 1 つのこと。5 歳児の語彙なので、主語と述語が 1 語ずつしかない。
 * ここが狭いおかげで、ひろばで伝わる情報も自然と削れていく。
 */
export type Fact = {
  subject: string
  predicate: string
  /** ひろばで聞いた相手の名前。自分のセッションから覚えたものは null。 */
  heardFrom: string | null
}

export type Pet = {
  /** ひろばでの一意の鍵。セッション ID と何代目かを繋いだもの。 */
  id: string
  /** 同じセッションで何代目か。生まれ変わるたびに増える。 */
  generation: number
  born: string
  /** 死んだ日時。生きている間は null。 */
  diedAt: string | null
  /** 累計の入力トークン。餌。 */
  input: number
  /** 累計の出力トークン。排泄物のもと。 */
  output: number
  /** 最後に流した時点の累計出力トークン。ここから溜まった分を数える。 */
  flushedOutput: number
  flushes: number
  health: number
  /** 直近のコンテキスト使用率 (0-100)。成長段階はこれで決まる。 */
  percent: number
  name: string | null
  /** 直近のひとこと。 */
  word: string | null
  /** 覚えていること。古いものから忘れる。 */
  knowledge: Fact[]
  /** 飼い主のセッションが最後に動いていた時刻。止まった判定に使う。 */
  seenAt: number
  /** いまひろばへ遊びに行っているか。 */
  away: boolean
  cwd: string
}

/** ウンチ 1 個あたりの出力トークン。1 ターンで 1 個前後に収まる量に取る。 */
export const OUTPUT_PER_POOP = 4000

/** これを超えて溜めると健康が減る。飼い主が流しに来るまでの猶予を残す。 */
export const POOP_LIMIT = 4

export const newPet = (sessionId: string, cwd: string, now: Date, generation = 0): Pet => ({
  id: `${sessionId}#${generation}`,
  generation,
  born: now.toISOString(),
  diedAt: null,
  input: 0,
  output: 0,
  flushedOutput: 0,
  flushes: 0,
  health: 100,
  percent: 0,
  name: null,
  word: null,
  knowledge: [],
  seenAt: now.getTime(),
  away: false,
  cwd,
})

/** この間ぶん心拍が途切れたら、そのセッションは止まったとみなす。 */
export const STALE_MS = 3 * 60 * 1000

/**
 * ひろばに居るか。Claudeっちは家かひろばのどちらかにしか居ない。
 * 止まったセッションの子はずっとひろばで待っている。
 */
export const inPlaza = (pet: Pet, now: number) =>
  !isDead(pet) && (pet.away || now - pet.seenAt > STALE_MS)

/** 飼い主のセッションが止まっているか。引き継げるのはこの子だけ。 */
export const isStopped = (pet: Pet, now: number) => !isDead(pet) && now - pet.seenAt > STALE_MS

/** 覚えていられる数。これを超えると古いものから忘れる。 */
export const MAX_FACTS = 12

/** 同じ主語のことは覚え直す。5 歳児なので、たくさんは覚えていられない。 */
export const remember = (pet: Pet, fact: Fact): Pet => ({
  ...pet,
  knowledge: [...pet.knowledge.filter((f) => f.subject !== fact.subject), fact].slice(-MAX_FACTS),
})

/**
 * 段階ごとの話し方。子供は単語だけ、大人から先は「<主語> は <述語>」。
 * 赤ちゃんと卵はまだ話さない。
 */
export const wordFor = (stage: Stage, fact: Fact): string | null => {
  if (stage === 'egg' || stage === 'baby') return null
  if (stage === 'child') return fact.subject
  return `${fact.subject} は ${fact.predicate}`
}

export const poopCount = (pet: Pet) =>
  Math.floor((pet.output - pet.flushedOutput) / OUTPUT_PER_POOP)

/**
 * 卵からかえるのは最初の食事。以降は成長段階をコンテキスト使用率で決める。
 * 使用率は会話が進むほど上がるので、巻き戻らない限り段階も戻らない。
 */
export const stageOf = (pet: Pet): Stage => {
  if (pet.input <= 0) return 'egg'
  if (pet.percent >= 40) return 'ojiisan'
  if (pet.percent >= 30) return 'ojisan'
  if (pet.percent >= 20) return 'adult'
  if (pet.percent >= 10) return 'child'
  return 'baby'
}

export const STAGE_LABEL: Record<Stage, string> = {
  egg: '卵',
  baby: '赤ちゃん',
  child: '子供',
  adult: '大人',
  ojisan: 'おじさん',
  ojiisan: 'おじいさん',
}

/** 食べたぶんを足し、溜まったウンチのぶん健康を動かす。死んだ子は何も変わらない。 */
export const feed = (pet: Pet, input: number, output: number, percent: number, now: Date): Pet => {
  if (isDead(pet)) return pet
  const fed = { ...pet, input: pet.input + input, output: pet.output + output, percent }
  const over = poopCount(fed) - POOP_LIMIT
  const health = Math.max(0, Math.min(100, over > 0 ? fed.health - over * 4 : fed.health + 3))
  return { ...fed, health, diedAt: health <= 0 ? now.toISOString() : null }
}

/** 健康が尽きたら死ぬ。死んだ子は食べも歩きもしない。 */
export const isDead = (pet: Pet) => pet.health <= 0

/**
 * ひろばに居る子を自分のセッションへ引き取る。卵の代わりにこの子が家に来る。
 * id は変えない。ひろばの行は飼い主が移ったぶんだけ書き換わる。
 */
export const adopt = (pet: Pet, cwd: string, now: Date): Pet => ({
  ...pet,
  cwd,
  away: false,
  word: null,
  seenAt: now.getTime(),
})

/** 同じセッションの次の代として生まれ直す。前の代はひろばに残る。 */
export const rebirth = (pet: Pet, sessionId: string, now: Date): Pet =>
  newPet(sessionId, pet.cwd, now, pet.generation + 1)

export const flush = (pet: Pet): Pet => ({
  ...pet,
  flushedOutput: pet.output,
  flushes: pet.flushes + 1,
  health: Math.min(100, pet.health + 10),
})

export type Traits = {
  /** 目の形。0 まる 1 たれ 2 つり 3 てん */
  eye: number
  /** 体の形。0 まるい 1 縦長 2 横長 */
  body: number
  /** 体の色。0x00RRGGBB */
  color: number
  /** 頬の色。 */
  accent: number
}

const hash = (s: string) => {
  let h = 2166136261
  for (let i = 0; i < s.length; i += 1) {
    h ^= s.charCodeAt(i)
    h = Math.imul(h, 16777619)
  }
  return h >>> 0
}

const hsvToRgb = (h: number, s: number, v: number) => {
  const c = v * s
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1))
  const m = v - c
  const t = Math.floor(h / 60) % 6
  const [r, g, b] = (
    [
      [c, x, 0],
      [x, c, 0],
      [0, c, x],
      [0, x, c],
      [x, 0, c],
      [c, 0, x],
    ] as const
  )[t] ?? [c, x, 0]
  const to = (n: number) => Math.max(0, Math.min(255, Math.round((n + m) * 255)))
  return (to(r) << 16) | (to(g) << 8) | to(b)
}

/** その子の色の濃さ。かえった時点でこの濃さで出る。 */
const SATURATION = 0.72

/** おじいさんの褪せ方。 */
const FADED = 0.55

/** 形も色もかえった時点で決まる。歳を取ったときだけ色が少し褪せる。 */
export const traitsOf = (pet: Pet): Traits => {
  const seed = hash(`${pet.id}:${pet.born}`)
  const hue = seed % 360
  const fade = stageOf(pet) === 'ojiisan' ? FADED : 1
  return {
    eye: (seed >>> 9) % 4,
    body: (seed >>> 11) % 3,
    color: hsvToRgb(hue, SATURATION * fade, 0.96),
    accent: hsvToRgb((hue + 40) % 360, 0.85 * fade, 0.99),
  }
}
