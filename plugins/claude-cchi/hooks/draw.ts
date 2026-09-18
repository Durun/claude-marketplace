// Claudeっちの姿を画素で描き、Raster の cells にする。
// 1 セルに四分ブロックを置き、縦横それぞれ 2 倍の画素を持つ。基準の姿を
// そのまま画素に書き起こしてあるので、拡大率と飾りだけで見た目を派生させる。

import { poopCount, stageOf, STAGE_LABEL, traitsOf, type Pet, type Stage } from './pet.ts'

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

/** 体の伸ばし方。基準からの差なので、赤ちゃんのうちは 3 つとも同じ形になる。 */
const stretch = (scale: number, body: number) =>
  body === 1
    ? { sx: Math.max(1, scale - 1), sy: scale }
    : body === 2
      ? { sx: scale, sy: Math.max(1, scale - 1) }
      : { sx: scale, sy: scale }

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

/** 腕を伸ばす向き。上下左右と斜め。 */
const SPARK_ARMS = [
  [0, -1], [0, 1], [-1, 0], [1, 0],
  [-1, -1], [1, -1], [-1, 1], [1, 1],
] as const

/**
 * 卵は、Claude が考えている間に出る印と同じ。中心から 8 方向に腕が伸び縮みする。
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

export const render = (columns: number, rows: number, pet: Pet, frame: number) => {
  const c = canvas(columns * 2, rows * 2)
  const stage = stageOf(pet)
  const traits = traitsOf(pet)
  // 狭い面では基準の絵が横にはみ出すので、入る大きさまで落とす。
  const fit = Math.max(1, Math.floor((columns - 2) / Math.ceil(ART_WIDTH / 2)))
  const { sx, sy } = stretch(Math.min(SCALE[stage], fit), traits.body)

  const groundY = c.height - 3
  for (let x = 0; x < c.width; x += 1) put(c, x, groundY, GROUND)

  // 呼吸で 1 画素だけ上下させる。止まって見えないための最小の動き。
  const bob = Math.round(Math.sin(frame / (stage === 'egg' ? 8 : 6)))
  const left = Math.max(1, Math.round(c.width * 0.4 - (ART_WIDTH * sx) / 2))
  const top = groundY - ART_HEIGHT * sy - 1 + bob

  if (stage === 'egg') {
    // 卵は地面に立たず、面の真ん中に浮かぶ。
    drawSpark(c, Math.round(c.width * 0.4), Math.round(c.height / 2) + bob, Math.min(SCALE.egg, fit), frame)
    return encode(c, columns, rows)
  }

  for (let row = 0; row < ART_HEIGHT; row += 1) {
    for (let col = 0; col < ART_WIDTH; col += 1) {
      if (ART[row]?.[col] !== '#') continue
      rect(c, left + col * sx, top + row * sy, sx, sy, traits.color)
    }
  }

  // 頬。体の色が薄いうちは見分けが付かないが、食べるほど出てくる。
  rect(c, left + 2 * sx, top + 2 * sy, sx, sy, traits.accent)
  rect(c, left + 18 * sx, top + 2 * sy, sx, sy, traits.accent)

  for (const col of EYE_COL) {
    drawEye(c, left + col * sx, top + EYE_ROW * sy, sx, sy, traits.eye, frame % 90 >= 87)
  }
  const thin = Math.max(1, Math.floor(sy / 2))
  const mouthY = top + MOUTH_ROW * sy
  rect(c, left + MOUTH_COL * sx, mouthY, sx * 2, thin, INK)

  // 口ひげは口の真上。おじいさんは白くなり、眉も生える。
  if (stage === 'ojisan' || stage === 'ojiisan') {
    rect(c, left + 8 * sx, mouthY - thin, sx * 4, thin, stage === 'ojiisan' ? HAIR : INK)
  }
  if (stage === 'ojiisan') {
    for (const col of EYE_COL) {
      rect(c, left + col * sx, top + EYE_ROW * sy - thin, sx * 2, thin, HAIR)
    }
  }

  // ウンチは右下に積む。溜まるほど左へ並ぶ。
  const poops = Math.min(poopCount(pet), 8)
  for (let i = 0; i < poops; i += 1) {
    const px = c.width - 5 - i * 6
    if (px < left + ART_WIDTH * sx + 3) break
    ellipse(c, px, groundY - 2, 3, 2, POOP)
    ellipse(c, px, groundY - 4, 2, 1.4, POOP_LIGHT)
  }

  return encode(c, columns, rows)
}

export const statusLine = (pet: Pet) => {
  const stage = STAGE_LABEL[stageOf(pet)]
  const who = pet.name ?? 'なまえなし'
  return `${who}  ${stage}  健康 ${pet.health}  餌 ${Math.round(pet.input / 1000)}k  ウンチ ${poopCount(pet)}`
}
