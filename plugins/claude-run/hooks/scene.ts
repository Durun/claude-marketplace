// 3D の場面を距離関数で持ち、1 ピクセルあたり 2x2 で光線を飛ばして平均する。
// 平均した色は限られた色数へ寄せるので、なめらかな陰影のままドットの粒が立つ。

export type Obstacle = { x: number; scale: number }

export type Frame = {
  /** 走者の腰の高さ。跳ぶと上がる。 */
  runnerY: number
  /** 走者の回転。走っている間ずっと回る。 */
  spin: number
  /** 地面の縞の位相。進んだぶんだけ流れる。 */
  ground: number
  /** カメラの周回角。0 で真横から見る。 */
  orbit: number
  obstacles: readonly Obstacle[]
}

const RUNNER_CENTER = 1.15
const CAMERA_RADIUS = 6.4
const CAMERA_HEIGHT = 2.1
const TARGET_Y = 1.1
const FOV = 0.92

const MAX_STEPS = 56
const MAX_DIST = 46
const SURFACE = 0.004

// 走者は放射状の棘の集まり。棘は 10 本で、長short を交互にして z へ振り分ける。
const LIMBS = Array.from({ length: 10 }, (_, i) => {
  const a = (i * Math.PI * 2) / 10
  const long = i % 2 === 0
  const tilt = (i % 4 < 2 ? 1 : -1) * 0.34
  const len = long ? 1.1 : 0.72
  const s = 1 / Math.sqrt(1 + tilt * tilt)
  return { x: Math.cos(a) * len * s, y: Math.sin(a) * len * s, z: tilt * len * s }
})

const MAT_GROUND = 0
const MAT_RUNNER = 1
const MAT_CACTUS = 2

let hitMaterial = MAT_GROUND

const capsule = (px: number, py: number, pz: number, bx: number, by: number, bz: number, r: number) => {
  const bb = bx * bx + by * by + bz * bz
  let h = (px * bx + py * by + pz * bz) / bb
  h = h < 0 ? 0 : h > 1 ? 1 : h
  const dx = px - bx * h
  const dy = py - by * h
  const dz = pz - bz * h
  return Math.sqrt(dx * dx + dy * dy + dz * dz) - r
}

const roundBox = (px: number, py: number, pz: number, hx: number, hy: number, hz: number, r: number) => {
  const qx = Math.abs(px) - hx
  const qy = Math.abs(py) - hy
  const qz = Math.abs(pz) - hz
  const ox = qx > 0 ? qx : 0
  const oy = qy > 0 ? qy : 0
  const oz = qz > 0 ? qz : 0
  const outside = Math.sqrt(ox * ox + oy * oy + oz * oz)
  const inside = Math.min(Math.max(qx, Math.max(qy, qz)), 0)
  return outside + inside - r
}

/** 走者。境界球の外では球までの距離を返すので、遠い光線は棘を数えずに進む。 */
const runner = (px: number, py: number, pz: number, f: Frame) => {
  const cy = py - RUNNER_CENTER - f.runnerY
  const bound = Math.sqrt(px * px + cy * cy + pz * pz) - 1.25
  if (bound > 0.15) return bound
  // 走者を回すかわりに、光線の側を逆へ回す。
  const c = Math.cos(-f.spin)
  const s = Math.sin(-f.spin)
  const lx = px * c - cy * s
  const ly = px * s + cy * c
  let d = 1e9
  for (const limb of LIMBS) {
    const t = capsule(lx, ly, pz, limb.x, limb.y, limb.z, 0.17)
    if (t < d) d = t
  }
  const core = Math.sqrt(lx * lx + ly * ly + pz * pz) - 0.34
  return Math.min(d, core)
}

/** 障害物。サボテンの胴と両腕。 */
const cactus = (px: number, py: number, pz: number, o: Obstacle) => {
  const k = o.scale
  const lx = (px - o.x) / k
  const ly = py / k
  const lz = pz / k
  const bound = Math.sqrt(lx * lx + (ly - 0.5) * (ly - 0.5) + lz * lz) - 1.1
  if (bound > 0.15) return bound * k
  // 胴の高さは 0.9。跳んで越えられるかどうかの判定がこの値に合わせてある。
  let d = roundBox(lx, ly - 0.41, lz, 0.19, 0.41, 0.19, 0.09)
  d = Math.min(d, capsule(lx, ly - 0.52, lz, 0.42, 0, 0, 0.13))
  d = Math.min(d, capsule(lx - 0.42, ly - 0.52, lz, 0, 0.3, 0, 0.13))
  d = Math.min(d, capsule(lx, ly - 0.34, lz, -0.38, 0, 0, 0.13))
  d = Math.min(d, capsule(lx + 0.38, ly - 0.34, lz, 0, 0.34, 0, 0.13))
  return d * k
}

const scene = (px: number, py: number, pz: number, f: Frame) => {
  let d = py
  let m = MAT_GROUND
  const r = runner(px, py, pz, f)
  if (r < d) {
    d = r
    m = MAT_RUNNER
  }
  for (const o of f.obstacles) {
    // 画面から外れた柱は距離関数から外す。
    if (Math.abs(px - o.x) > 3 && Math.abs(px - o.x) - 3 > d) continue
    const c = cactus(px, py, pz, o)
    if (c < d) {
      d = c
      m = MAT_CACTUS
    }
  }
  hitMaterial = m
  return d
}

// 法線と光線の色は、返り値を作らずにここへ置く。1 コマで数万回呼ぶので確保を避ける。
let nx = 0
let ny = 0
let nz = 0
let outR = 0
let outG = 0
let outB = 0

const normal = (px: number, py: number, pz: number, f: Frame) => {
  const e = 0.0025
  const a = scene(px + e, py - e, pz - e, f)
  const b = scene(px - e, py - e, pz + e, f)
  const c = scene(px - e, py + e, pz - e, f)
  const d = scene(px + e, py + e, pz + e, f)
  const gx = a - b - c + d
  const gy = -a - b + c + d
  const gz = -a + b - c + d
  const l = Math.sqrt(gx * gx + gy * gy + gz * gz) || 1
  nx = gx / l
  ny = gy / l
  nz = gz / l
}

const LIGHT = (() => {
  const l = Math.sqrt(0.55 * 0.55 + 0.78 * 0.78 + 0.32 * 0.32)
  return { x: 0.55 / l, y: 0.78 / l, z: -0.32 / l }
})()

const GROUND_COLOR: readonly [number, number, number] = [0.78, 0.63, 0.42]
const RUNNER_COLOR: readonly [number, number, number] = [0.85, 0.47, 0.34]
const CACTUS_COLOR: readonly [number, number, number] = [0.25, 0.49, 0.38]

const colorOf = (material: number) =>
  material === MAT_RUNNER ? RUNNER_COLOR : material === MAT_CACTUS ? CACTUS_COLOR : GROUND_COLOR

const SKY_TOP: readonly [number, number, number] = [0.11, 0.14, 0.22]
const SKY_LOW: readonly [number, number, number] = [0.29, 0.31, 0.41]

/** 1 本の光線の色。当たらなければ空。 */
const trace = (
  ox: number,
  oy: number,
  oz: number,
  dx: number,
  dy: number,
  dz: number,
  f: Frame,
) => {
  let t = 0.05
  let hit = false
  let material = MAT_GROUND
  for (let i = 0; i < MAX_STEPS; i += 1) {
    const d = scene(ox + dx * t, oy + dy * t, oz + dz * t, f)
    if (d < SURFACE * t + 0.0015) {
      hit = true
      material = hitMaterial
      break
    }
    t += d
    if (t > MAX_DIST) break
  }
  const skyMix = dy * 0.5 + 0.5
  const skyR = SKY_TOP[0] * skyMix + SKY_LOW[0] * (1 - skyMix)
  const skyG = SKY_TOP[1] * skyMix + SKY_LOW[1] * (1 - skyMix)
  const skyB = SKY_TOP[2] * skyMix + SKY_LOW[2] * (1 - skyMix)
  if (!hit) {
    outR = skyR
    outG = skyG
    outB = skyB
    return
  }
  const px = ox + dx * t
  const py = oy + dy * t
  const pz = oz + dz * t
  normal(px, py, pz, f)
  const base = colorOf(material)
  let r = base[0]
  let g = base[1]
  let b = base[2]
  if (material === MAT_GROUND) {
    // 進行方向に沿った縞。位相を送るだけで地面が流れて見える。
    const stripe = Math.sin(px * 1.6 + f.ground) > 0 ? 1 : 0.86
    // 走路。カメラが回っても、どこを走っているのかが分かるように帯を敷く。
    const onTrack = Math.abs(pz) < 2.4
    const edge = Math.abs(Math.abs(pz) - 2.4) < 0.12
    const lane = onTrack ? 0.82 : 1
    r *= stripe * lane
    g *= stripe * lane
    b *= stripe * lane
    if (edge) {
      r = 0.95
      g = 0.85
      b = 0.62
    }
    // 走者と柱の真下を落とす。影の光線を飛ばさずに接地だけ見せる。
    const under = Math.exp(-(px * px + pz * pz) * 0.35) * Math.exp(-(f.runnerY * f.runnerY) * 1.2)
    let shade = under
    for (const o of f.obstacles) {
      const ddx = px - o.x
      shade += Math.exp(-(ddx * ddx + pz * pz) * 0.5) * 0.8
    }
    const k = 1 - Math.min(shade, 0.85) * 0.6
    r *= k
    g *= k
    b *= k
  }
  const lambert = Math.max(nx * LIGHT.x + ny * LIGHT.y + nz * LIGHT.z, 0)
  // カメラ側からの補助光。主光が当たらない手前の面が黒く潰れるのを防ぐ。
  const fill = Math.max(-(nx * dx + ny * dy + nz * dz), 0) * 0.34
  const rim = Math.pow(1 - Math.max(-(nx * dx + ny * dy + nz * dz), 0), 3) * 0.25
  const lit = 0.46 + lambert * 0.8 + fill
  r = r * lit + rim * 0.5
  g = g * lit + rim * 0.55
  b = b * lit + rim * 0.7
  // 遠景は空へ溶かす。柱が遠くから現れる感じが出る。
  const fog = 1 - Math.exp(-t * 0.028)
  outR = r * (1 - fog) + skyR * fog
  outG = g * (1 - fog) + skyG * fog
  outB = b * (1 - fog) + skyB * fog
}

// 表示に使う色。素材ごとの明度段と空の段だけを持ち、ここへ丸めることで粒を揃える。
const PALETTE: number[] = (() => {
  const out: number[] = []
  const push = (r: number, g: number, b: number) => {
    const to = (v: number) => Math.max(0, Math.min(255, Math.round(v * 255)))
    out.push((to(r) << 16) | (to(g) << 8) | to(b))
  }
  const ramp = (c: readonly [number, number, number]) => {
    for (const k of [0.22, 0.38, 0.55, 0.72, 0.88, 1.05, 1.22]) push(c[0] * k, c[1] * k, c[2] * k)
  }
  ramp(GROUND_COLOR)
  ramp(RUNNER_COLOR)
  ramp(CACTUS_COLOR)
  for (let i = 0; i <= 5; i += 1) {
    const k = i / 5
    push(SKY_LOW[0] * (1 - k) + SKY_TOP[0] * k, SKY_LOW[1] * (1 - k) + SKY_TOP[1] * k, SKY_LOW[2] * (1 - k) + SKY_TOP[2] * k)
  }
  push(1, 1, 1)
  return out
})()

const quantize = (r: number, g: number, b: number) => {
  const ri = Math.max(0, Math.min(255, r * 255))
  const gi = Math.max(0, Math.min(255, g * 255))
  const bi = Math.max(0, Math.min(255, b * 255))
  let best = 0
  let bestD = Number.POSITIVE_INFINITY
  for (const c of PALETTE) {
    const dr = ((c >> 16) & 255) - ri
    const dg = ((c >> 8) & 255) - gi
    const db = (c & 255) - bi
    const d = dr * dr + dg * dg + db * db
    if (d < bestD) {
      bestD = d
      best = c
    }
  }
  return best
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

const base64 = (bytes: Uint8Array) => {
  let out = ''
  for (let i = 0; i < bytes.length; i += 3) {
    const a = bytes[i] ?? 0
    const b = bytes[i + 1] ?? 0
    const c = bytes[i + 2] ?? 0
    const n1 = (a << 16) | (b << 8) | c
    out += B64[(n1 >> 18) & 63]
    out += B64[(n1 >> 12) & 63]
    out += i + 1 < bytes.length ? B64[(n1 >> 6) & 63] : '='
    out += i + 2 < bytes.length ? B64[n1 & 63] : '='
  }
  return out
}

const HALF_BLOCK = 0x2580 // ▀ 上半分。前景が上のピクセル、背景が下のピクセル。

/** 画素を並べた配列を返す。高さはセル数の 2 倍で、上下 1 組が 1 セルになる。 */
export const renderPixels = (columns: number, rows: number, f: Frame, superSample: number) => {
  const width = columns
  const height = rows * 2
  const angle = f.orbit
  const ex = Math.sin(angle) * CAMERA_RADIUS
  const ez = Math.cos(angle) * CAMERA_RADIUS
  const ey = CAMERA_HEIGHT
  let fx = -ex
  let fy = TARGET_Y - ey
  let fz = -ez
  const fl = Math.sqrt(fx * fx + fy * fy + fz * fz)
  fx /= fl
  fy /= fl
  fz /= fl
  // 右方向は前方と真上の外積。上方向はそこから作り直す。
  let rx = fy * 0 - fz * 1
  let ry = fz * 0 - fx * 0
  let rz = fx * 1 - fy * 0
  const rl = Math.sqrt(rx * rx + ry * ry + rz * rz)
  rx /= rl
  ry /= rl
  rz /= rl
  const ux = ry * fz - rz * fy
  const uy = rz * fx - rx * fz
  const uz = rx * fy - ry * fx
  const aspect = width / (height * 1.0)
  const scale = Math.tan(FOV / 2)
  const pixels = new Uint32Array(width * height)
  const step = 1 / superSample
  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      let ar = 0
      let ag = 0
      let ab = 0
      for (let sy = 0; sy < superSample; sy += 1) {
        for (let sx = 0; sx < superSample; sx += 1) {
          const px = ((x + (sx + 0.5) * step) / width) * 2 - 1
          const py = 1 - ((y + (sy + 0.5) * step) / height) * 2
          const cx = px * scale * aspect
          const cy = py * scale
          let dx = fx + rx * cx + ux * cy
          let dy = fy + ry * cx + uy * cy
          let dz = fz + rz * cx + uz * cy
          const dl = Math.sqrt(dx * dx + dy * dy + dz * dz)
          dx /= dl
          dy /= dl
          dz /= dl
          trace(ex, ey, ez, dx, dy, dz, f)
          ar += outR
          ag += outG
          ab += outB
        }
      }
      const k = 1 / (superSample * superSample)
      pixels[y * width + x] = quantize(ar * k, ag * k, ab * k)
    }
  }
  return pixels
}

/** Raster の cells。1 セルに上下 2 画素を半ブロックで詰める。 */
export const encodeCells = (columns: number, rows: number, pixels: Uint32Array) => {
  const words = new Uint32Array(columns * rows * 3)
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < columns; col += 1) {
      const top = pixels[row * 2 * columns + col] ?? 0
      const bottom = pixels[(row * 2 + 1) * columns + col] ?? 0
      const at = (row * columns + col) * 3
      words[at] = HALF_BLOCK
      words[at + 1] = top
      words[at + 2] = bottom
    }
  }
  return base64(new Uint8Array(words.buffer))
}

export const render = (columns: number, rows: number, f: Frame, superSample = 2) =>
  encodeCells(columns, rows, renderPixels(columns, rows, f, superSample))
