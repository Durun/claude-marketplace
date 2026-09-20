// 面の上で動いているものの状態。Claudeっちの居場所と、餌・ウンチ・トイレの進み具合を持つ。
// 保存しない。セッションを開き直せば、Claudeっちは器の前から歩き直す。

/** 餌の粒 1 つぶんの入力トークン。 */
export const TOKENS_PER_GRAIN = 400

/** 一度に降らせる粒の上限。多すぎると器から溢れて見える。 */
const MAX_FALLING = 24

/** 器に残せる粒。 */
const BOWL_CAPACITY = 40

/** 1 粒を food から減らすまでのコマ数。 */
const CHEW_FRAMES = 3

/**
 * 歩く速さ。1 コマあたりの画素。
 * 1 セルは 2 画素なので、2 ずつ動かして位置をセルの境目に揃える。
 * 奇数だと目のような細かい模様がセルをまたいで揺れて見える。
 */
const WALK_SPEED = 2

/** 家で過ごすコマ数。これを過ぎるとひろばへ遊びに行く。 */
const HOME_FRAMES = 600

/** ひろばで遊んでいるコマ数。 */
const VISIT_FRAMES = 250

export type Mode =
  | 'idle'
  | 'walk'
  | 'eat'
  | 'poop'
  | 'leave'
  | 'arrive'
  | 'study'
  | 'doze'
  | 'sleep'
  | 'wake'

/**
 * 飼い主の側で何が起きているか。Claudeっちの手持ち無沙汰の埋め方がこれで変わる。
 * think は Claude が考えている間、wait は道具の返事を待っている間。
 */
export type Activity = 'free' | 'think' | 'wait'

/** 待ち始めてからウトウトするまでのコマ数。短い道具では眠らない。 */
const DOZE_AFTER = 45

/** ウトウトから寝入るまでのコマ数。 */
const SLEEP_AFTER = 120

/** ハッと起きてから普段に戻るまでのコマ数。 */
const WAKE_FRAMES = 14

export type Grain = { x: number; y: number }

export type World = {
  /** 画素での面の幅と地面の高さ。 */
  width: number
  ground: number
}

export type Scene = {
  step: number
  /** Claudeっちの左端の画素位置。 */
  x: number
  facing: 1 | -1
  mode: Mode
  /** 今のふるまいが終わるコマ。 */
  until: number
  /** 歩いていく先の画素位置。 */
  target: number
  /** 器に溜まった餌の粒。 */
  food: number
  /** 降っている途中の粒。 */
  falling: Grain[]
  /** 落ちているウンチの画素位置。 */
  poops: number[]
  /** トイレへ流している最中か。 */
  flushing: boolean
  /** ひろばへ遊びに行っているか。家とひろばのどちらかにしか居ない。 */
  away: boolean
  /** 次に出かける、または帰るコマ。 */
  tripAt: number
  /** 飼い主の側の様子。 */
  activity: Activity
  /** その様子になったコマ。ウトウトと寝入りの頃合いをここから数える。 */
  activitySince: number
}

export const newScene = (): Scene => ({
  step: 0,
  x: 0,
  facing: 1,
  mode: 'idle',
  until: 0,
  target: 0,
  food: 0,
  falling: [],
  poops: [],
  flushing: false,
  away: false,
  tripAt: HOME_FRAMES,
  activity: 'free',
  activitySince: 0,
})

/** 眠っているか。眠っている間は餌も独り言も止める。 */
const asleep = (scene: Scene) => scene.mode === 'doze' || scene.mode === 'sleep'

/**
 * 飼い主の側の様子を伝える。寝ている最中に動きが戻ったら、ハッと目を覚ます。
 */
export const setActivity = (scene: Scene, activity: Activity) => {
  if (scene.activity === activity) return
  const wasAsleep = asleep(scene)
  scene.activity = activity
  scene.activitySince = scene.step
  if (!wasAsleep) return
  scene.mode = 'wake'
  scene.until = scene.step + WAKE_FRAMES
}

/** Claudeっちが立てる左端。トイレは右端に据え置く。 */
export const PET_MIN_X = 2
export const toiletX = (world: World) => world.width - 7

/** 歩く位置をセルの境目に揃える。 */
const even = (v: number) => Math.round(v / 2) * 2

/** 器とトイレの間。ここを歩き、器に重なって食べる。 */
const range = (world: World, petWidth: number) => {
  const min = even(PET_MIN_X)
  const max = even(toiletX(world) - 7 - petWidth)
  return max <= min ? { min: Math.max(0, max), max: Math.max(0, max) } : { min, max }
}

/** 入力トークンを粒にして器の上へ降らせる。 */
export const sprinkle = (scene: Scene, world: World, tokens: number, bowlX: number) => {
  const grains = Math.min(MAX_FALLING, Math.floor(tokens / TOKENS_PER_GRAIN))
  for (let i = 0; i < grains; i += 1) {
    scene.falling.push({ x: bowlX + (i % 7) - 3, y: -((i * 3) % 18) })
  }
}

/** ウンチをする。しゃがんでいる間は歩かない。 */
export const excrete = (scene: Scene) => {
  scene.mode = 'poop'
  scene.until = scene.step + 10
}

export const startFlush = (scene: Scene) => {
  if (scene.poops.length > 0) scene.flushing = true
}

/**
 * 家とひろばを行き来する頃合いなら、歩いて出入りを始める。
 * 出るときはトイレの側から画面の外へ抜け、帰りは同じ側から入ってくる。
 */
export const travel = (scene: Scene, world: World, petWidth: number) => {
  if (scene.step < scene.tripAt) return
  if (scene.away) {
    scene.away = false
    scene.x = even(world.width + petWidth)
    scene.mode = 'arrive'
    scene.target = range(world, petWidth).max
    scene.tripAt = scene.step + HOME_FRAMES
    return
  }
  scene.mode = 'leave'
  scene.target = even(world.width + petWidth)
  scene.tripAt = scene.step + VISIT_FRAMES
}

/** 1 コマ進める。流し終えたら true を返し、呼び手が溜まりを 0 に戻す。 */
export const advance = (scene: Scene, world: World, petWidth: number) => {
  scene.step += 1

  const rest = world.ground - 2

  for (const grain of scene.falling) grain.y += 2
  const landed = scene.falling.filter((g) => g.y >= rest)
  scene.falling = scene.falling.filter((g) => g.y < rest)
  scene.food = Math.min(BOWL_CAPACITY, scene.food + landed.length)

  if (scene.flushing) {
    const toilet = toiletX(world)
    scene.poops = scene.poops
      .map((x) => x + Math.max(2, Math.round((toilet - x) / 6)))
      .filter((x) => x < toilet)
    if (scene.poops.length === 0) {
      scene.flushing = false
      return true
    }
    return false
  }

  // ひろばに居る間は家のことをしない。餌だけは器に降り続ける。
  if (scene.away) return false

  const { min: minX, max: maxX } = range(world, petWidth)
  switch (scene.mode) {
    case 'wake':
      if (scene.step >= scene.until) scene.mode = 'idle'
      break
    case 'study':
    case 'doze':
    case 'sleep':
      // 餌が降ったか、飼い主の手が空いたら中断して普段に戻る。
      if (scene.food > 0 || scene.activity === 'free') scene.mode = 'idle'
      // 待ちが長引くほど深く眠る。
      else if (scene.activity === 'wait' && scene.step - scene.activitySince > SLEEP_AFTER) {
        scene.mode = 'sleep'
      }
      break
    case 'poop':
      if (scene.step >= scene.until) scene.mode = 'idle'
      break
    case 'eat':
      if (scene.step % CHEW_FRAMES === 0) scene.food = Math.max(0, scene.food - 1)
      if (scene.food === 0 || scene.step >= scene.until) scene.mode = 'idle'
      break
    case 'leave':
    case 'arrive':
    case 'walk': {
      const gap = scene.target - scene.x
      scene.facing = gap >= 0 ? 1 : -1
      const next = scene.x + Math.sign(gap) * WALK_SPEED
      // 出入りの間は画面の外まで歩くので、家の範囲に閉じ込めない。
      scene.x = scene.mode === 'walk' ? Math.max(minX, Math.min(maxX, next)) : next
      if (Math.abs(gap) <= WALK_SPEED) {
        if (scene.mode === 'leave') {
          scene.away = true
          scene.x = minX
        }
        scene.mode = scene.mode === 'walk' && scene.food > 0 && scene.x <= minX + WALK_SPEED
          ? 'eat'
          : 'idle'
        scene.until = scene.step + 40
      }
      break
    }
    default:
      // 餌があれば器へ。無ければ気まぐれに歩き回る。
      if (scene.food > 0) {
        scene.mode = 'walk'
        scene.target = minX
      } else if (scene.activity === 'wait' && scene.step - scene.activitySince > DOZE_AFTER) {
        scene.mode = scene.step - scene.activitySince > SLEEP_AFTER ? 'sleep' : 'doze'
      } else if (scene.activity === 'think') {
        scene.mode = 'study'
      } else if (scene.step % 40 === 0) {
        scene.mode = 'walk'
        scene.target = even(minX + Math.random() * (maxX - minX))
      }
  }
  return false
}
