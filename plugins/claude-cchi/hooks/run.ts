// [ あそぶ ] で始まる走りの進み方。柱を跳び越えるだけの単純な遊びで、走った分だけ健康が戻る。
// 姿は Claudeっち本人から取るので、目も体つきも色も家に居るときと同じ子が走る。

import { stageOf, traitsOf, type Pet, type Stage } from './pet.ts'
import { GLOOM_BELOW } from './draw.ts'
import type { Frame, Look, Obstacle } from './course.ts'

/** 1 コマの長さ。物理も間隔もこの刻みで数える。 */
export const FRAME_MS = 33

/**
 * 健康が 1 戻るまでに走るコマ数。
 * 溜まったウンチ 1 個ぶんの減りを、10 秒ほど走れば取り戻せるところに取る。
 */
export const TICKS_PER_HEALTH = 45

export type Game = Frame & {
  velocity: number
  speed: number
  score: number
  best: number
  over: boolean
  ticks: number
  /** この回の走りで戻した健康。 */
  healed: number
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

/** まばたきの間隔と、目を閉じているコマ数。家の面と同じ速さで瞬く。 */
const BLINK_EVERY = 300
const BLINK_FRAMES = 9

/** 具合が悪いか。家の面と同じしきい値で、口がへの字になる。 */
export const gloomyOf = (pet: Pet) => pet.health < GLOOM_BELOW

/** 走者の大きさ。育つほど大きく、赤ちゃんのうちは小さいまま走る。 */
const STAGE_SIZE: Record<Stage, number> = {
  egg: 1,
  baby: 1.05,
  child: 1.25,
  adult: 1.5,
  ojisan: 1.5,
  ojiisan: 1.45,
}

/** 家に居るときと同じ姿を、走者の距離関数へ渡す形に直す。 */
export const lookOf = (pet: Pet): Look => {
  const traits = traitsOf(pet)
  const stage = stageOf(pet)
  return {
    color: traits.color,
    accent: traits.accent,
    eye: traits.eye,
    body: traits.body,
    size: STAGE_SIZE[stage],
    mustache: stage === 'ojisan' || stage === 'ojiisan',
    white: stage === 'ojiisan',
    gloomy: gloomyOf(pet),
    blink: false,
  }
}

/**
 * 走っている間に健康が戻ると、口がへの字から直る。
 * 変わったときだけ作り直すので、毎コマ姿を組み直さない。
 */
export const withMood = (game: Game, pet: Pet): Game =>
  game.look.gloomy === gloomyOf(pet)
    ? game
    : { ...game, look: { ...game.look, gloomy: gloomyOf(pet) } }

export const initial = (look: Look, best = 0): Game => ({
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
  healed: 0,
  depth: 0,
  look,
})

/** 柱の最小間隔。跳んでいる間に進む距離より広く取り、必ず一度着地できるようにする。 */
const MIN_GAP = 15

/** 柱の高さ。距離関数の胴と同じ値を持つので、見た目と当たり判定がずれない。 */
const CACTUS_TOP = 0.9

const cleared = (runnerY: number, o: Obstacle) => runnerY > CACTUS_TOP * o.scale

const hits = (runnerY: number, o: Obstacle) => Math.abs(o.x) < 0.62 * o.scale && !cleared(runnerY, o)

/** カメラの動き。当たった後も続けるので、走りの計算とは分けて持つ。 */
const camera = (ticks: number, orbit: number) => {
  // 奥行きは端で速さが 0 になるように寄せる。切り替わりが唐突にならない。
  const t = Math.max(0, Math.min((ticks - DEPTH_START) / DEPTH_LENGTH, 1))
  return {
    depth: t * t * (3 - 2 * t),
    orbit: ticks > ORBIT_START ? orbit + ORBIT_RATE * Math.min((ticks - ORBIT_START) / 90, 1) : 0,
  }
}

export const step = (game: Game, jump: boolean): Game => {
  // 当たった後は場面を止めたまま、カメラだけ回り続ける。健康もそこで止まる。
  if (game.over) return { ...game, ticks: game.ticks + 1, ...camera(game.ticks + 1, game.orbit) }

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
  const obstacles = game.obstacles.map((o) => ({ ...o, x: o.x - speed })).filter((o) => o.x > -8)

  const last = obstacles.reduce((max, o) => Math.max(max, o.x), Number.NEGATIVE_INFINITY)
  if (last < 20) {
    obstacles.push({
      x: Math.max(26, last + MIN_GAP + Math.random() * 12),
      scale: Math.random() < 0.3 ? 1.35 : 1,
    })
  }

  const ticks = game.ticks + 1

  return {
    ...game,
    runnerY,
    velocity,
    speed,
    obstacles,
    ticks,
    ...camera(ticks, game.orbit),
    // 足は進んだ距離で振れる。地面の縞も同じ速さで流す。
    // まばたきは走りとは関わりなく、一定の間隔で来る。
    look:
      game.look.blink === (ticks % BLINK_EVERY >= BLINK_EVERY - BLINK_FRAMES)
        ? game.look
        : { ...game.look, blink: !game.look.blink },
    stride: game.stride + speed * 6.5,
    ground: game.ground + speed * 0.8,
    score: game.score + 1,
    healed: Math.floor(ticks / TICKS_PER_HEALTH),
    over: obstacles.some((o) => hits(runnerY, o)),
  }
}
