// Claudeっちの姿を画素で描き、Raster の cells にする。
// 1 セルに半ブロック `▀` を置き、前景を上の画素、背景を下の画素として縦を 2 倍に使う。

import { poopCount, stageOf, STAGE_LABEL, traitsOf, type Pet, type Stage } from './pet.ts'

/** 端末の既定色。bit 24 だけを立てた値。 */
const DEFAULT = 0x01000000
const HALF_BLOCK = 0x2580

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

const ellipse = (c: Canvas, cx: number, cy: number, rx: number, ry: number, color: number) => {
  for (let y = Math.ceil(cy - ry); y <= cy + ry; y += 1) {
    for (let x = Math.ceil(cx - rx); x <= cx + rx; x += 1) {
      const dx = (x - cx) / rx
      const dy = (y - cy) / ry
      if (dx * dx + dy * dy <= 1) put(c, x, y, color)
    }
  }
}

const encode = (c: Canvas, columns: number, rows: number) => {
  const words = new Uint32Array(columns * rows * 3)
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < columns; col += 1) {
      const at = (row * columns + col) * 3
      words[at] = HALF_BLOCK
      words[at + 1] = c.px[row * 2 * c.width + col] ?? DEFAULT
      words[at + 2] = c.px[(row * 2 + 1) * c.width + col] ?? DEFAULT
    }
  }
  return base64(new Uint8Array(words.buffer))
}

/** 段階ごとの体の大きさ。おじさんから先は太らず、色と飾りだけが変わる。 */
const SIZE: Record<Stage, { rx: number; ry: number }> = {
  egg: { rx: 5, ry: 6 },
  baby: { rx: 5, ry: 4 },
  child: { rx: 7, ry: 6 },
  adult: { rx: 9, ry: 8 },
  ojisan: { rx: 9, ry: 8 },
  ojiisan: { rx: 9, ry: 8 },
}

const EYE_WHITE = 0xf8f8f8
const INK = 0x101010
const POOP = 0x6b4423
const POOP_LIGHT = 0x8a5c30
const SHELL = 0xf2e6cf
const GROUND = 0x3a3f4b

/** 白ひげ。おじいさんだけ。 */
const HAIR = 0xe8e8e8

const drawEyes = (c: Canvas, cx: number, cy: number, rx: number, eye: number, blink: boolean) => {
  const gap = Math.max(2, Math.round(rx * 0.45))
  for (const side of [-1, 1]) {
    const ex = cx + side * gap
    if (blink || eye === 3) {
      for (let d = -1; d <= 1; d += 1) put(c, ex + d, cy, INK)
      continue
    }
    ellipse(c, ex, cy, 1.6, 1.6, EYE_WHITE)
    // 瞳の位置で表情を変える。0 まる 1 たれ 2 つり。
    const pupilY = eye === 1 ? cy + 0.6 : eye === 2 ? cy - 0.6 : cy
    put(c, ex, pupilY, INK)
    put(c, ex + side * 0.4, pupilY, INK)
  }
}

export const render = (columns: number, rows: number, pet: Pet, frame: number) => {
  const c = canvas(columns, rows * 2)
  const stage = stageOf(pet)
  const traits = traitsOf(pet)
  const size = SIZE[stage]
  const rx = stage === 'egg' ? size.rx : size.rx * (traits.body === 2 ? 1.25 : 1)
  const ry = stage === 'egg' ? size.ry : size.ry * (traits.body === 1 ? 1.25 : 1)

  const groundY = c.height - 2
  for (let x = 0; x < c.width; x += 1) put(c, x, groundY, GROUND)

  const cx = Math.round(c.width * 0.33)
  // 呼吸で 1 画素だけ上下させる。止まって見えないための最小の動き。
  const bob = stage === 'egg' ? Math.round(Math.sin(frame / 8)) : Math.round(Math.sin(frame / 6))
  const cy = groundY - ry - 1 + bob

  if (stage === 'egg') {
    ellipse(c, cx, cy, rx, ry, SHELL)
    for (let i = 0; i < 5; i += 1) {
      const a = (i / 5) * Math.PI * 2
      put(c, cx + Math.cos(a) * rx * 0.5, cy + Math.sin(a) * ry * 0.5, traits.accent)
    }
  } else {
    ellipse(c, cx, cy, rx, ry, traits.color)
    // 頬。体の色が薄いうちは見分けが付かないが、食べるほど出てくる。
    put(c, cx - rx * 0.75, cy + ry * 0.2, traits.accent)
    put(c, cx + rx * 0.75, cy + ry * 0.2, traits.accent)
    drawEyes(c, cx, cy - ry * 0.15, rx, traits.eye, frame % 90 >= 87)
    // 口。
    for (let d = -1; d <= 1; d += 1) put(c, cx + d, cy + ry * 0.5, INK)
    if (stage === 'ojisan' || stage === 'ojiisan') {
      const hair = stage === 'ojiisan' ? HAIR : INK
      for (let d = -2; d <= 2; d += 1) put(c, cx + d, cy + ry * 0.75, hair)
      for (let d = -1; d <= 1; d += 1) put(c, cx + d, cy + ry * 0.9, hair)
    }
    if (stage === 'ojiisan') {
      for (const side of [-1, 1]) {
        for (let d = -1; d <= 1; d += 1) put(c, cx + side * rx * 0.45 + d, cy - ry * 0.6, HAIR)
      }
    }
  }

  // ウンチは右下に積む。溜まるほど横に並ぶ。
  const poops = Math.min(poopCount(pet), 8)
  for (let i = 0; i < poops; i += 1) {
    const px = c.width - 4 - i * 4
    if (px < cx + rx + 2) break
    ellipse(c, px, groundY - 1, 1.8, 1.2, POOP)
    ellipse(c, px, groundY - 3, 1.2, 1, POOP_LIGHT)
    put(c, px, groundY - 4, POOP_LIGHT)
  }

  return encode(c, columns, rows)
}

export const statusLine = (pet: Pet) => {
  const stage = STAGE_LABEL[stageOf(pet)]
  const who = pet.name ?? 'なまえなし'
  return `${who}  ${stage}  健康 ${pet.health}  餌 ${Math.round(pet.input / 1000)}k  ウンチ ${poopCount(pet)}`
}
