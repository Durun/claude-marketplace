import type { Frame, Obstacle } from './scene.ts'

/** 1 コマの長さ。物理も間隔もこの刻みで数える。 */
export const FRAME_MS = 70

export type Game = Frame & {
  velocity: number
  speed: number
  score: number
  best: number
  over: boolean
  ticks: number
}

const GRAVITY = 0.075
const JUMP = 0.62
const START_SPEED = 0.29

/** カメラが回り始めるコマ。最初はしばらく真横から見せる。 */
const ORBIT_START = 90

/** カメラが 1 コマで回る角。周回に約 1 分かかる。 */
const ORBIT_RATE = 0.0105

export const initial = (best = 0): Game => ({
  runnerY: 0,
  velocity: 0,
  spin: 0,
  ground: 0,
  orbit: 0,
  speed: START_SPEED,
  obstacles: [{ x: 26, scale: 1 }],
  score: 0,
  best,
  over: false,
  ticks: 0,
})

/** 柱の最小間隔。跳んでいる間に進む距離より広く取り、必ず一度着地できるようにする。 */
const MIN_GAP = 15

/** 柱の高さ。距離関数の胴と同じ値を持つので、見た目と当たり判定がずれない。 */
const CACTUS_TOP = 0.9

/** 接地しているときの走者の下端。中心 1.15 から棘の長さ 1.1 を引いた値。 */
const RUNNER_BOTTOM = 0.05

const cleared = (runnerY: number, o: Obstacle) => runnerY + RUNNER_BOTTOM > CACTUS_TOP * o.scale

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

  const speed = Math.min(START_SPEED + game.ticks * 0.00035, 0.62)
  const obstacles = game.obstacles
    .map((o) => ({ ...o, x: o.x - speed }))
    .filter((o) => o.x > -8)

  const last = obstacles.reduce((max, o) => Math.max(max, o.x), Number.NEGATIVE_INFINITY)
  if (last < 20) {
    obstacles.push({ x: Math.max(26, last + MIN_GAP + Math.random() * 12), scale: Math.random() < 0.3 ? 1.35 : 1 })
  }

  const ticks = game.ticks + 1
  const orbit = ticks > ORBIT_START ? game.orbit + ORBIT_RATE * Math.min((ticks - ORBIT_START) / 40, 1) : 0

  return {
    ...game,
    runnerY,
    velocity,
    speed,
    obstacles,
    ticks,
    orbit,
    // 走者は転がりながら進む。地面の縞も同じ速さで流す。
    spin: game.spin - speed * 0.85,
    ground: game.ground + speed * 0.8,
    score: game.score + 1,
    over: obstacles.some((o) => hits(runnerY, o)),
  }
}
