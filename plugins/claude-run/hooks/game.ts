import type { Frame, Obstacle } from './scene.ts'

/** 1 コマの長さ。物理も間隔もこの刻みで数える。 */
export const FRAME_MS = 33

export type Game = Frame & {
  velocity: number
  speed: number
  score: number
  best: number
  over: boolean
  ticks: number
}

const GRAVITY = 0.0167
const JUMP = 0.232
const START_SPEED = 0.205

/** 奥行きが出始めるコマと、出切るまでのコマ数。それまでは望遠の真横で、平らな絵に見える。 */
const DEPTH_START = 150
const DEPTH_LENGTH = 210

/** カメラが回り始めるコマ。奥行きが出切ってから回す。 */
const ORBIT_START = DEPTH_START + DEPTH_LENGTH

/** カメラが 1 コマで回る角。周回に約 1 分かかる。 */
const ORBIT_RATE = 0.005

export const initial = (best = 0): Game => ({
  runnerY: 0,
  velocity: 0,
  stride: 0,
  ground: 0,
  orbit: 0,
  speed: START_SPEED,
  obstacles: [{ x: 26, scale: 1 }],
  score: 0,
  best,
  over: false,
  ticks: 0,
  depth: 0,
})

/** 柱の最小間隔。跳んでいる間に進む距離より広く取り、必ず一度着地できるようにする。 */
const MIN_GAP = 15

/** 柱の高さ。距離関数の胴と同じ値を持つので、見た目と当たり判定がずれない。 */
const CACTUS_TOP = 0.9

const cleared = (runnerY: number, o: Obstacle) => runnerY > CACTUS_TOP * o.scale

const hits = (runnerY: number, o: Obstacle) => Math.abs(o.x) < 0.62 * o.scale && !cleared(runnerY, o)

export const step = (game: Game, jump: boolean): Game => {
  if (game.over) return game

  let velocity = game.velocity
  let runnerY = game.runnerY
  if (jump && runnerY === 0) velocity = JUMP
  if (velocity !== 0 || runnerY > 0) {
    velocity -= GRAVITY
    runnerY += velocity
    if (runnerY <= 0) {
      runnerY = 0
      velocity = 0
    }
  }

  const speed = Math.min(START_SPEED + game.ticks * 0.000117, 0.45)
  const obstacles = game.obstacles
    .map((o) => ({ ...o, x: o.x - speed }))
    .filter((o) => o.x > -8)

  const last = obstacles.reduce((max, o) => Math.max(max, o.x), Number.NEGATIVE_INFINITY)
  if (last < 20) {
    obstacles.push({ x: Math.max(26, last + MIN_GAP + Math.random() * 12), scale: Math.random() < 0.3 ? 1.35 : 1 })
  }

  const ticks = game.ticks + 1
  // 奥行きは端で速さが 0 になるように寄せる。切り替わりが唐突にならない。
  const t = Math.max(0, Math.min((ticks - DEPTH_START) / DEPTH_LENGTH, 1))
  const depth = t * t * (3 - 2 * t)
  const orbit = ticks > ORBIT_START ? game.orbit + ORBIT_RATE * Math.min((ticks - ORBIT_START) / 90, 1) : 0

  return {
    ...game,
    runnerY,
    velocity,
    speed,
    obstacles,
    ticks,
    orbit,
    depth,
    // 足は進んだ距離で振れる。地面の縞も同じ速さで流す。
    stride: game.stride + speed * 6.5,
    ground: game.ground + speed * 0.8,
    score: game.score + 1,
    over: obstacles.some((o) => hits(runnerY, o)),
  }
}
