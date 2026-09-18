// Claudeっちの姿を画素で描き、Raster の cells にする。
// 1 セルに四分ブロックを置き、縦横それぞれ 2 倍の画素を持つ。基準の姿を
// そのまま画素に書き起こしてあるので、拡大率と飾りだけで見た目を派生させる。

import { poopCount, stageOf, STAGE_LABEL, traitsOf, type Pet, type Stage } from './pet.ts'
import { BOWL_X, toiletX, type Scene, type World } from './scene.ts'

/** 端末の既定色。bit 24 だけを立てた値。 */
const DEFAULT = 0x01000000

/** 四分ブロック。添字は 左上 8 / 右上 4 / 左下 2 / 右下 1 の和。 */
const QUADRANT = [
  0x0020, 0x2597, 0x2596, 0x2584, 0x259d, 0x2590, 0x259e, 0x259f, 0x2598, 0x259a, 0x258c, 0x2599,
  0x2580, 0x259c, 0x259b, 0x2588,
]

/**
 * ノーマルタイプの大人。1 文字が 2x2 の画素にあたる元の絵をそのまま展開したもの。
 *
 *   ▐▛███▛█
 *  ▝▜██████▀
 *    ▝▝ ▝▝
 */
export const ART = [
  '...#############......',
  '...##.#######.##......',
  '.#################....',
  '...#############......',
  '.....#.#...#.#........',
] as const

const ART_WIDTH = ART[0].length
const ART_HEIGHT = ART.length

/** 目は元の絵の上の切れ込みに重ねる。 */
const EYE_COL = [5, 13] as const
const EYE_ROW = 1
const MOUTH_COL = 9
const MOUTH_ROW = 3

/** 基準の姿を四分ブロックの文字に戻す。元の絵と突き合わせるために使う。 */
export const artLines = () => {
  const lines: string[] = []
  for (let row = 0; row < ART_HEIGHT; row += 2) {
    let line = ''
    for (let col = 0; col < ART_WIDTH; col += 2) {
      const bits =
        (ART[row]?.[col] === '#' ? 8 : 0) |
        (ART[row]?.[col + 1] === '#' ? 4 : 0) |
        (ART[row + 1]?.[col] === '#' ? 2 : 0) |
        (ART[row + 1]?.[col + 1] === '#' ? 1 : 0)
      line += String.fromCodePoint(QUADRANT[bits] ?? 0x20)
    }
    lines.push(line.trimEnd())
  }
  return lines
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

const base64 = (bytes: Uint8Array) => {
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const n0 = ((bytes[i] ?? 0) << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0)
    out += B64[(n0 >> 18) & 63]
    out += B64[(n0 >> 12) & 63]
    out += i + 1 < bytes.length ? B64[(n0 >> 6) & 63] : '='
    out += i + 2 < bytes.length ? B64[n0 & 63] : '='
  }
  return out
}

type Canvas = { width: number; height: number; px: Int32Array }

const canvas = (width: number, height: number): Canvas => ({
  width,
  height,
  px: new Int32Array(width * height).fill(DEFAULT),
})

const put = (c: Canvas, x: number, y: number, color: number) => {
  const xi = Math.round(x)
  const yi = Math.round(y)
  if (xi < 0 || yi < 0 || xi >= c.width || yi >= c.height) return
  c.px[yi * c.width + xi] = color
}

const rect = (c: Canvas, x: number, y: number, w: number, h: number, color: number) => {
  for (let dy = 0; dy < h; dy += 1) for (let dx = 0; dx < w; dx += 1) put(c, x + dx, y + dy, color)
}

const ellipse = (c: Canvas, cx: number, cy: number, rx: number, ry: number, color: number) => {
  for (let y = Math.ceil(cy - ry); y <= cy + ry; y += 1) {
    for (let x = Math.ceil(cx - rx); x <= cx + rx; x += 1) {
      const dx = (x - cx) / rx
      const dy = (y - cy) / ry
      if (dx * dx + dy * dy <= 1) put(c, x, y, color)
    }
  }
}

/**
 * 1 セルの 4 画素を四分ブロック 1 文字にたたむ。
 * 1 セルに置ける色は前景と背景の 2 つなので、多数を占める色を前景に取る。
 */
export const encode = (c: Canvas, columns: number, rows: number) => {
  const words = new Uint32Array(columns * rows * 3)
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < columns; col += 1) {
      const quad = [
        c.px[row * 2 * c.width + col * 2] ?? DEFAULT,
        c.px[row * 2 * c.width + col * 2 + 1] ?? DEFAULT,
        c.px[(row * 2 + 1) * c.width + col * 2] ?? DEFAULT,
        c.px[(row * 2 + 1) * c.width + col * 2 + 1] ?? DEFAULT,
      ]
      const count = (v: number) => quad.filter((q) => q === v).length
      const ink = quad.filter((q) => q !== DEFAULT)
      const fg = ink.length === 0 ? DEFAULT : ink.reduce((a, b) => (count(a) >= count(b) ? a : b))
      const rest = quad.filter((q) => q !== fg)
      const bg = rest.length === 0 ? DEFAULT : rest.reduce((a, b) => (count(a) >= count(b) ? a : b))
      const bits = quad.reduce((acc, q, i) => acc | (q === fg && fg !== DEFAULT ? 8 >> i : 0), 0)
      const at = (row * columns + col) * 3
      words[at] = QUADRANT[bits] ?? 0x0020
      words[at + 1] = fg
      words[at + 2] = bg
    }
  }
  return base64(new Uint8Array(words.buffer))
}

/** 拡大率。おじさんから先は大きくならず、飾りだけが変わる。 */
const SCALE: Record<Stage, number> = { egg: 2, baby: 1, child: 2, adult: 3, ojisan: 3, ojiisan: 3 }

const EYE_WHITE = 0xf8f8f8
const INK = 0x101010
const POOP = 0x6b4423
const POOP_LIGHT = 0x8a5c30
const GROUND = 0x3a3f4b
const HAIR = 0xe8e8e8

/**
 * 拡大率を縦横に分ける。ほそながいは縦、ずんぐりは横へ 1 段伸ばす。
 * 伸ばした先が面に入らないときは、面に入るところまで戻す。
 */
const stretch = (columns: number, rows: number, stage: Stage, body: number) => {
  const base = SCALE[stage]
  const roomX = Math.max(1, Math.floor(columns / ART_WIDTH))
  const roomY = Math.max(1, Math.floor((rows * 2 - 4) / ART_HEIGHT))
  return {
    sx: Math.min(base + (body === 2 ? 1 : 0), roomX),
    sy: Math.min(base + (body === 1 ? 1 : 0), roomY),
  }
}

const drawEye = (c: Canvas, x: number, y: number, sx: number, sy: number, eye: number, blink: boolean) => {
  // 赤ちゃんの体は白目と瞳を描き分けるには小さすぎるので、点の目にする。
  if (blink || eye === 3 || sx < 2 || sy < 2) {
    rect(c, x, y + sy, sx * 2, Math.max(1, Math.floor(sy / 2)), INK)
    return
  }
  const h = sy + Math.floor(sy / 2)
  rect(c, x, y, sx * 2, h, EYE_WHITE)
  // 瞳の位置で表情を変える。0 まる 1 たれ 2 つり。
  const py = eye === 1 ? y + h - sy : eye === 2 ? y : y + Math.floor((h - sy) / 2)
  rect(c, x + Math.floor(sx / 2), py, Math.max(1, sx), Math.max(1, sy), INK)
}


/** 考えている間の印の色。 */
const SPARK = 0xd97757

const TOKEN = 0xd9b45a
const BOWL = 0x8a8f9a
const PORCELAIN = 0xe8eaee
const WATER = 0x6fb7e0

/** 腕を伸ばす向き。上下左右と斜め。 */
const SPARK_ARMS = [
  [0, -1],
  [0, 1],
  [-1, 0],
  [1, 0],
  [-1, -1],
  [1, -1],
  [-1, 1],
  [1, 1],
] as const

/**
 * 卵は、Claude が考えている間に出る印と同じ。中心から 8 方向へ腕が伸び縮みする。
 * 斜めの腕を 1 つ短く取ると、伸びきったところで星の形に見える。
 */
const drawSpark = (c: Canvas, cx: number, cy: number, scale: number, frame: number) => {
  const beat = Math.floor(frame / 4) % 4
  const reach = 2 + (beat < 2 ? beat : 3 - beat)
  rect(c, cx - scale, cy - scale, scale * 2, scale * 2, SPARK)
  for (const [dx, dy] of SPARK_ARMS) {
    const len = dx !== 0 && dy !== 0 ? reach - 1 : reach
    for (let step = 1; step <= len; step += 1) {
      rect(c, cx - scale / 2 + dx * step * scale, cy - scale / 2 + dy * step * scale, scale, scale, SPARK)
    }
  }
}

/** 餌の器。縁だけを描き、溜まった粒を中に積む。 */
const drawBowl = (c: Canvas, x: number, ground: number, food: number) => {
  const left = x - 5
  for (let i = 0; i < 11; i += 1) put(c, left + i, ground - 1, BOWL)
  for (let i = 1; i <= 3; i += 1) {
    put(c, left, ground - 1 - i, BOWL)
    put(c, left + 10, ground - 1 - i, BOWL)
  }
  for (let i = 0; i < Math.min(food, 27); i += 1) {
    put(c, left + 1 + (i % 9), ground - 2 - Math.floor(i / 9), TOKEN)
  }
}

/** トイレ。流している間だけ水面が回る。 */
const drawToilet = (c: Canvas, x: number, ground: number, swirl: number | null) => {
  const left = x - 5
  rect(c, left + 7, ground - 12, 4, 6, PORCELAIN)
  ellipse(c, left + 4, ground - 6, 5, 3.4, PORCELAIN)
  ellipse(c, left + 4, ground - 6, 3.4, 2, swirl === null ? WATER : DEFAULT)
  if (swirl !== null) {
    // 水が渦を巻いて見えるよう、水面の点を回す。
    for (let i = 0; i < 3; i += 1) {
      const a = (swirl / 3 + (i * Math.PI * 2) / 3) % (Math.PI * 2)
      put(c, left + 4 + Math.cos(a) * 2.6, ground - 6 + Math.sin(a) * 1.4, WATER)
    }
  }
  rect(c, left + 2, ground - 2, 6, 1, PORCELAIN)
}

const drawPoop = (c: Canvas, x: number, ground: number) => {
  ellipse(c, x, ground - 2, 3, 2, POOP)
  ellipse(c, x, ground - 4, 2, 1.4, POOP_LIGHT)
}

export const render = (columns: number, rows: number, pet: Pet, scene: Scene, world: World) => {
  const c = canvas(columns * 2, rows * 2)
  const stage = stageOf(pet)
  const traits = traitsOf(pet)
  const { sx, sy } = stretch(columns, rows, stage, traits.body)
  const groundY = world.ground

  for (let x = 0; x < c.width; x += 1) put(c, x, groundY, GROUND)
  drawBowl(c, BOWL_X, groundY, scene.food)
  drawToilet(c, toiletX(world), groundY, scene.flushing ? scene.step : null)
  for (const grain of scene.falling) put(c, grain.x, grain.y, TOKEN)
  for (const x of scene.poops) drawPoop(c, x, groundY)

  if (stage === 'egg') {
    // 卵は地面に立たず、面の真ん中に浮かぶ。
    drawSpark(c, Math.round(c.width * 0.4), Math.round(c.height / 2), Math.min(sx, sy), scene.step)
    return encode(c, columns, rows)
  }

  // 歩いている間は足を交互に出し、ウンチの間はしゃがむ。
  const thin = Math.max(1, Math.floor(sy / 2))
  const swing = scene.mode === 'walk' && Math.floor(scene.step / 3) % 2 === 0 ? sx : 0
  const left = Math.round(scene.x)
  const top = groundY - ART_HEIGHT * sy - 1 + (scene.mode === 'poop' ? thin : 0)

  for (let row = 0; row < ART_HEIGHT; row += 1) {
    for (let col = 0; col < ART_WIDTH; col += 1) {
      if (ART[row]?.[col] !== '#') continue
      const shift = row === ART_HEIGHT - 1 ? (col < ART_WIDTH / 2 ? swing : -swing) : 0
      rect(c, left + col * sx + shift, top + row * sy, sx, sy, traits.color)
    }
  }

  rect(c, left + 2 * sx, top + 2 * sy, sx, sy, traits.accent)
  rect(c, left + 18 * sx, top + 2 * sy, sx, sy, traits.accent)

  for (const col of EYE_COL) {
    drawEye(c, left + col * sx, top + EYE_ROW * sy, sx, sy, traits.eye, scene.step % 90 >= 87)
  }

  // 食べている間は口を開け閉めする。
  const chewing = scene.mode === 'eat' && Math.floor(scene.step / 2) % 2 === 0
  const mouthY = top + MOUTH_ROW * sy
  rect(c, left + MOUTH_COL * sx, mouthY, sx * 2, chewing ? sy : thin, INK)

  // 口ひげは口の真上。おじいさんは白くなり、眉も生える。
  if (stage === 'ojisan' || stage === 'ojiisan') {
    rect(c, left + 8 * sx, mouthY - thin, sx * 4, thin, stage === 'ojiisan' ? HAIR : INK)
  }
  if (stage === 'ojiisan') {
    for (const col of EYE_COL) {
      rect(c, left + col * sx, top + EYE_ROW * sy - thin, sx * 2, thin, HAIR)
    }
  }

  return encode(c, columns, rows)
}

/** Claudeっちの横幅。歩ける範囲と、頭上の札の位置を決めるのに使う。 */
export const petWidth = (columns: number, rows: number, pet: Pet) =>
  ART_WIDTH * stretch(columns, rows, stageOf(pet), traitsOf(pet).body).sx

export const statusLine = (pet: Pet) => {
  const stage = STAGE_LABEL[stageOf(pet)]
  const who = pet.name ?? 'なまえなし'
  return `${who}  ${stage}  健康 ${pet.health}  餌 ${Math.round(pet.input / 1000)}k  ウンチ ${poopCount(pet)}`
}
