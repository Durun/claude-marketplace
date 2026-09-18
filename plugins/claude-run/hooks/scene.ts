// 3D の場面を距離関数で持ち、1 ピクセルあたり 2x2 で光線を飛ばして平均する。
// 平均した色は限られた色数へ寄せるので、なめらかな陰影のままドットの粒が立つ。

export type Obstacle = { x: number; scale: number }

export type Frame = {
  /** 走者の腰の高さ。跳ぶと上がる。 */
  runnerY: number
  /** 走りの位相。足はこの位相で前後に振れる。 */
  stride: number
  /** 地面の縞の位相。進んだぶんだけ流れる。 */
  ground: number
  /** カメラの周回角。0 で真横から見る。 */
  orbit: number
  /** 奥行きの強さ。0 で望遠の真横、1 で寄った画角。走者の大きさは変わらない。 */
  depth: number
  obstacles: readonly Obstacle[]
}

/**
 * カメラの距離。遠いほど遠近が消え、平らな絵に見える。
 * 近づけると同時に画角を広げるので、走者の大きさは変わらないまま奥行きだけが出る。
 */
const FAR_RADIUS = 46
const NEAR_RADIUS = 7.2

const FAR_HEIGHT = 1.15
const NEAR_HEIGHT = 1.9

/**
 * 画面に入れる縦の世界の高さ。これを保ったまま距離を変えるので、走者の大きさが動かない。
 * 行数の少ない端末では詰める。そうしないと走者が数画素になって、目も足も潰れる。
 * 横に見える範囲は縦から比で決まるが、細長い面では十分に広いので、柱を見る余裕は残る。
 */
const VIEW_HEIGHT_FULL = 5.6
const VIEW_HEIGHT_MIN = 3

const viewHeight = (height: number) =>
  Math.max(VIEW_HEIGHT_MIN, Math.min(VIEW_HEIGHT_FULL, (VIEW_HEIGHT_FULL * height) / 52))
const TARGET_Y = 0.85

/**
 * 注視点を走者の横へずらす量。画面の横幅に対する割合で持つので、端末の幅が変わっても
 * 走者は同じ位置に立つ。
 *
 * ずらす向きは柱が来る側の逆に取る。真横から見ている間は走者が左に立ち、
 * 周回して柱が正面から来るころには中央へ寄り、反対側まで回ると右に立つ。
 * こうすると、どの画角でも柱を見てから跳ぶまでの間合いが残る。
 */
const TARGET_SHIFT_RATIO = 0.6

const MAX_STEPS = 56
const MAX_DIST = 90
/**
 * 当たりとみなす距離を、その光線が受け持つ画素の太さから決める。
 * 一定の割合にすると、望遠で遠くから見たときに輪郭が膨らむ。
 *
 * 上限も置く。行数の少ない端末では 1 画素が受け持つ幅が広くなり、
 * 甘いままだと物を囲む球の手前で当たったことになって、輪郭の外に輪が出る。
 */
const SURFACE_RATIO = 0.3
const SURFACE_MAX = 0.05

const MAT_GROUND = 0
const MAT_RUNNER = 1
const MAT_CACTUS = 2
const MAT_EYE = 3

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

/** なめらかな和。胴と頭のつなぎ目を丸める。 */
const smin = (a: number, b: number, k: number) => {
  const h = Math.max(k - Math.abs(a - b), 0) / k
  return Math.min(a, b) - h * h * k * 0.25
}

const sphere = (px: number, py: number, pz: number, r: number) => Math.sqrt(px * px + py * py + pz * pz) - r

const ellipsoid = (px: number, py: number, pz: number, rx: number, ry: number, rz: number) => {
  const k0 = Math.sqrt((px / rx) ** 2 + (py / ry) ** 2 + (pz / rz) ** 2)
  if (k0 === 0) return -Math.min(rx, Math.min(ry, rz))
  const k1 = Math.sqrt((px / (rx * rx)) ** 2 + (py / (ry * ry)) ** 2 + (pz / (rz * rz)) ** 2)
  return (k0 * (k0 - 1)) / k1
}

/** 足の付け根。前後 2 対で、対角の 2 本が同じ位相で動く。振れ幅が大きいので真横からは 4 つに見える。 */
const LEGS = [
  { x: 0.54, z: 0.24, phase: 0 },
  { x: 0.28, z: -0.24, phase: Math.PI },
  { x: -0.28, z: 0.24, phase: Math.PI },
  { x: -0.54, z: -0.24, phase: 0 },
] as const

/** 目の位置。胴の横に前後 2 つ並べるので、真横から見ても 2 つに見える。 */
const EYES = [
  { x: 0.55, y: 0.62 },
  { x: -0.55, y: 0.62 },
] as const

/** 目は胴の横面へ少しだけ出す。胴の半奥行きは BODY_HZ と丸めの和。 */
const EYE_Z = 0.56
/** 目は縦に長い。 */
const EYE_RX = 0.075
const EYE_RY = 0.11
const EYE_RZ = 0.075

/** 胴。角ばった箱で、横幅に対して背が低い。 */
const BODY_Y = 0.54
const BODY_HX = 0.78
const BODY_HY = 0.23
const BODY_HZ = 0.42
const BODY_ROUND = 0.15

/** 手。胴の前後の端から横へ張り出す。 */
const HAND_X = 0.97
const HAND_Y = 0.44

const RUNNER_SCALE = 1.5

/**
 * 走者。境界球の外では球までの距離を返すので、遠い光線は体を数えずに進む。
 * 足は走りの位相で前後に振れ、跳んでいる間は畳む。
 */
const runner = (px0: number, py0: number, pz0: number, f: Frame) => {
  const px = px0 / RUNNER_SCALE
  const py = (py0 - f.runnerY) / RUNNER_SCALE
  const pz = pz0 / RUNNER_SCALE
  const ly = py
  const bound = Math.sqrt(px * px + (ly - BODY_Y) * (ly - BODY_Y) + pz * pz) - 1.2
  if (bound > 0.15) return bound

  let d = roundBox(px, ly - BODY_Y, pz, BODY_HX, BODY_HY, BODY_HZ, BODY_ROUND)
  d = Math.min(d, roundBox(px - HAND_X, ly - HAND_Y, pz, 0.11, 0.05, 0.22, 0.04))
  d = Math.min(d, roundBox(px + HAND_X, ly - HAND_Y, pz, 0.11, 0.05, 0.22, 0.04))

  const airborne = Math.min(f.runnerY, 0.6) / 0.6
  for (const leg of LEGS) {
    const phase = f.stride + leg.phase
    const swing = Math.sin(phase) * 0.08 * (1 - airborne)
    // 前へ振り出した足だけ地面から浮かせる。跳んでいる間は 4 本とも畳む。
    const lift = Math.max(Math.cos(phase), 0) * 0.05 * (1 - airborne) + airborne * 0.1
    d = smin(d, capsule(px - leg.x, ly - 0.28, pz - leg.z, swing, -0.28 + lift, 0, 0.09), 0.05)
  }
  return d * RUNNER_SCALE
}

/** 目。体より手前に置くので、体の距離関数とは別に持つ。 */
const eyes = (px0: number, py0: number, pz0: number, f: Frame) => {
  const px = px0 / RUNNER_SCALE
  const ly = (py0 - f.runnerY) / RUNNER_SCALE
  const pz = pz0 / RUNNER_SCALE
  let d = Number.POSITIVE_INFINITY
  for (const eye of EYES) {
    const a = ellipsoid(px - eye.x, ly - eye.y, pz - EYE_Z, EYE_RX, EYE_RY, EYE_RZ)
    if (a < d) d = a
    const b = ellipsoid(px - eye.x, ly - eye.y, pz + EYE_Z, EYE_RX, EYE_RY, EYE_RZ)
    if (b < d) d = b
  }
  return d * RUNNER_SCALE
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

/** 地面を除いた場面。地面は平面なので、光線との交点を直接解く。 */
const scene = (px: number, py: number, pz: number, f: Frame) => {
  let d = Number.POSITIVE_INFINITY
  let m = MAT_RUNNER
  const r = runner(px, py, pz, f)
  if (r < d) {
    d = r
    m = MAT_RUNNER
  }
  const e = eyes(px, py, pz, f)
  if (e < d) {
    d = e
    m = MAT_EYE
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

const EYE_COLOR: readonly [number, number, number] = [0.06, 0.05, 0.05]

const colorOf = (material: number) =>
  material === MAT_RUNNER
    ? RUNNER_COLOR
    : material === MAT_CACTUS
      ? CACTUS_COLOR
      : material === MAT_EYE
        ? EYE_COLOR
        : GROUND_COLOR

const SKY_TOP: readonly [number, number, number] = [0.11, 0.14, 0.22]
const SKY_LOW: readonly [number, number, number] = [0.29, 0.31, 0.41]

/** 影を探す距離。光は斜めなので、物からこれだけ離れた地面までは影が伸びうる。 */
const SHADOW_RANGE = 6

/**
 * 地面の一点が影に入っているか。光へ向かって距離関数を進め、遮られたら影とする。
 * 丸い暗がりを描く代わりに実際の遮りを見るので、カメラが回っても形が破綻しない。
 *
 * 半影は作らない。1 画素が世界の数センチを受け持つ粗さなので、濃淡を付けると
 * 縁が斑に散る。影の内と外だけを返し、縁の均しは画素ごとの多数決に任せる。
 */
const shadow = (px: number, pz: number, f: Frame) => {
  let near = Math.sqrt(px * px + pz * pz)
  for (const o of f.obstacles) {
    const dx = px - o.x
    const d = Math.sqrt(dx * dx + pz * pz)
    if (d < near) near = d
  }
  if (near > SHADOW_RANGE) return 1

  let t = 0.05
  for (let i = 0; i < 32; i += 1) {
    const d = scene(px + LIGHT.x * t, LIGHT.y * t, pz + LIGHT.z * t, f)
    if (d < 0.01) return 0.62
    t += d
    if (t > SHADOW_RANGE) break
  }
  return 1
}

/** 1 本の光線の色。地面にも物にも当たらなければ空。 */
const trace = (
  ox: number,
  oy: number,
  oz: number,
  dx: number,
  dy: number,
  dz: number,
  f: Frame,
  fogStart: number,
  tStart: number,
  pixelAngle: number,
) => {
  // 地面は平面なので、行進させずに交点を解く。地平線まで途切れない。
  const tGround = dy < -1e-6 ? -oy / dy : Number.POSITIVE_INFINITY
  const limit = Math.min(tGround, MAX_DIST)
  let t = tStart
  let hit = false
  let material = MAT_GROUND
  for (let i = 0; i < MAX_STEPS; i += 1) {
    const d = scene(ox + dx * t, oy + dy * t, oz + dz * t, f)
    if (d < Math.min(Math.max(t * pixelAngle * SURFACE_RATIO, 0.0015), SURFACE_MAX)) {
      hit = true
      material = hitMaterial
      break
    }
    t += d
    if (t > limit) break
  }
  if (!hit && tGround < MAX_DIST) {
    hit = true
    t = tGround
    material = MAT_GROUND
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
  const base = colorOf(material)
  let r = base[0]
  let g = base[1]
  let b = base[2]
  if (material === MAT_GROUND) {
    // 平面なので法線は真上で決まる。
    nx = 0
    ny = 1
    nz = 0
    // 遠くの縞は 1 画素に何本も入るので、距離とともに薄くして目のちらつきを抑える。
    const sharp = Math.exp(-Math.max(t - fogStart, 0) * 0.06)
    const stripe = 1 - (Math.sin(px * 1.6 + f.ground) > 0 ? 0 : 0.14) * sharp
    // 走路。カメラが回っても、どこを走っているのかが分かるように帯を敷く。
    const onTrack = Math.abs(pz) < 2.4
    const lane = onTrack ? 0.82 : 1
    r *= stripe * lane
    g *= stripe * lane
    b *= stripe * lane
    const k = shadow(px, pz, f)
    r *= k
    g *= k
    b *= k
  } else {
    normal(px, py, pz, f)
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
  // 望遠のときはカメラが遠いので、走者より奥へ入った距離だけを数える。
  const fog = 1 - Math.exp(-Math.max(t - fogStart, 0) * 0.013)
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
    // 段の幅を細かく取る。粗いと、なだらかな陰影に段の輪が出る。
    for (let i = 0; i < 16; i += 1) {
      const k = 0.2 + (i * (1.28 - 0.2)) / 15
      push(c[0] * k, c[1] * k, c[2] * k)
    }
  }
  ramp(GROUND_COLOR)
  ramp(RUNNER_COLOR)
  ramp(CACTUS_COLOR)
  ramp(EYE_COLOR)
  for (let i = 0; i <= 12; i += 1) {
    const k = i / 12
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

// 並べ替えディザの升目。-0.5 から 0.5 の範囲で画素ごとの偏りを持つ。
const DITHER = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5].map((v) => v / 16 - 0.47)

/**
 * ディザの強さ。色の段が見えるときに上げる。色の段を細かく取ってあるので既定は 0。
 * 0.02 あたりから効き始め、0.05 を超えると平らな面に網目が見える。
 */
const DITHER_DEPTH = 0

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
  const depth = Math.max(0, Math.min(f.depth, 1))
  const radius = FAR_RADIUS + (NEAR_RADIUS - FAR_RADIUS) * depth
  const view = viewHeight(height)
  const aspect = width / height
  const shift = (view / 2) * aspect * TARGET_SHIFT_RATIO * Math.cos(angle)
  // カメラの右方向は周回角だけで決まるので、注視点より先に求まる。
  const shiftX = Math.cos(angle) * shift
  const shiftZ = -Math.sin(angle) * shift
  const tx = shiftX
  const tz = shiftZ
  const ex = Math.sin(angle) * radius + tx
  const ez = Math.cos(angle) * radius + tz
  const ey = FAR_HEIGHT + (NEAR_HEIGHT - FAR_HEIGHT) * depth
  let fx = tx - ex
  let fy = TARGET_Y - ey
  let fz = tz - ez
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
  // 縦に見える高さを保つ画角。距離と一緒に動かすと、寄っても走者の背丈は変わらない。
  const scale = view / 2 / radius
  // 霧は走者のところから数え始める。手前は素の色で出る。
  const fogStart = radius
  // 場面は原点の周りにしかないので、望遠のときは手前の空間を飛ばして光線を始める。
  const tStart = Math.max(radius - 36, 0.05)
  // 1 画素が張る角度。当たりの判定に使う。
  const pixelAngle = view / radius / height
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
          trace(ex, ey, ez, dx, dy, dz, f, fogStart, tStart, pixelAngle)
          ar += outR
          ag += outG
          ab += outB
        }
      }
      const k = 1 / (superSample * superSample)
      // 色を丸めると、なだらかな陰影に段の輪が出る。画素ごとに決まった量だけ
      // ずらしてから丸めると、段が画素の粗さに散って輪が消える。
      const bias = (DITHER[(y & 3) * 4 + (x & 3)] ?? 0) * DITHER_DEPTH
      pixels[y * width + x] = quantize(ar * k + bias, ag * k + bias, ab * k + bias)
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

/**
 * 1 画素あたりに飛ばす光線の数の平方根。上げるほど輪郭がなめらかになり、
 * 1 コマの時間がその 2 乗で伸びる。2 で約 9 ms、3 で約 20 ms（120x26 セル）。
 */
export const SUPER_SAMPLE = 2

export const render = (columns: number, rows: number, f: Frame, superSample = SUPER_SAMPLE) =>
  encodeCells(columns, rows, renderPixels(columns, rows, f, superSample))
