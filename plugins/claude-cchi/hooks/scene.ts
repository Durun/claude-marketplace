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

/** 歩く速さ。1 コマあたりの画素。 */
const WALK_SPEED = 1

export type Mode = 'idle' | 'walk' | 'eat' | 'poop'

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
})

/** 器は左端、トイレは右端に据え置く。 */
export const BOWL_X = 7
export const toiletX = (world: World) => world.width - 7

/** 器とトイレの間。ここを歩き、器の右隣で食べる。 */
const range = (world: World, petWidth: number) => {
  const min = BOWL_X + 7
  const max = toiletX(world) - 7 - petWidth
  return max <= min ? { min: Math.max(0, max), max: Math.max(0, max) } : { min, max }
}

/** 入力トークンを粒にして器の上へ降らせる。 */
export const sprinkle = (scene: Scene, world: World, tokens: number) => {
  const grains = Math.min(MAX_FALLING, Math.floor(tokens / TOKENS_PER_GRAIN))
  for (let i = 0; i < grains; i += 1) {
    scene.falling.push({ x: BOWL_X + (i % 7) - 3, y: -((i * 3) % 18) })
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

  const { min: minX, max: maxX } = range(world, petWidth)
  switch (scene.mode) {
    case 'poop':
      if (scene.step >= scene.until) scene.mode = 'idle'
      break
    case 'eat':
      if (scene.step % CHEW_FRAMES === 0) scene.food = Math.max(0, scene.food - 1)
      if (scene.food === 0 || scene.step >= scene.until) scene.mode = 'idle'
      break
    case 'walk': {
      const gap = scene.target - scene.x
      scene.facing = gap >= 0 ? 1 : -1
      scene.x = Math.max(minX, Math.min(maxX, scene.x + Math.sign(gap) * WALK_SPEED))
      if (Math.abs(gap) <= WALK_SPEED) {
        scene.mode = scene.food > 0 && scene.x <= minX + WALK_SPEED ? 'eat' : 'idle'
        scene.until = scene.step + 40
      }
      break
    }
    default:
      // 餌があれば器へ。無ければ気まぐれに歩き回る。
      if (scene.food > 0) {
        scene.mode = 'walk'
        scene.target = minX
      } else if (scene.step % 40 === 0) {
        scene.mode = 'walk'
        scene.target = minX + Math.round(Math.random() * (maxX - minX))
      }
  }
  return false
}
