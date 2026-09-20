// 走る場面を距離関数で持ち、1 画素 1 本の光線で色を決める。
// 走者は Claudeっち本人なので、体つき・目の形・色は家で描いている姿から受け取る。

export type Obstacle = { x: number; scale: number }

/** 走者の姿。家の絵と同じ個性を、距離関数が扱える形で持つ。 */
export type Look = {
  /** 体の色。0x00RRGGBB */
  color: number
  /** 頬の色。 */
  accent: number
  /** 目の形。0 まる 1 たれ 2 つり 3 てん */
  eye: number
  /** 体の形。0 まるい 1 縦長 2 横長 */
  body: number
  /** 成長段階ぶんの大きさ。 */
  size: number
  /** 口ひげが生えているか。 */
  mustache: boolean
  /** ひげが白いか。おじいさんは眉も白い。 */
  white: boolean
  /** 具合が悪いか。口がへの字になる。 */
  gloomy: boolean
  /** いま目を閉じているか。 */
  blink: boolean
}

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
  look: Look
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
 * 行数の少ない面では詰める。そうしないと走者が数画素になって、目も足も潰れる。
 */
const VIEW_HEIGHT_FULL = 5.6
const VIEW_HEIGHT_MIN = 3

const viewHeight = (height: number) =>
  Math.max(VIEW_HEIGHT_MIN, Math.min(VIEW_HEIGHT_FULL, (VIEW_HEIGHT_FULL * height) / 52))
const TARGET_Y = 0.85

/**
 * 注視点を走者の横へずらす量。画面の横幅に対する割合で持つので、面の幅が変わっても
 * 走者は同じ位置に立つ。
 *
 * ずらす向きは柱が来る側の逆に取る。真横から見ている間は走者が左に立ち、
 * 周回して柱が正面から来るころには中央へ寄り、反対側まで回ると右に立つ。
 */
const TARGET_SHIFT_RATIO = 0.6

const MAX_STEPS = 44
const MAX_DIST = 90

/**
 * 当たりとみなす距離を、その光線が受け持つ画素の太さから決める。
 * 一定の割合にすると、望遠で遠くから見たときに輪郭が膨らむ。
 */
const SURFACE_RATIO = 0.3
const SURFACE_MAX = 0.05

const MAT_GROUND = 0
const MAT_RUNNER = 1
const MAT_CACTUS = 2
const MAT_EYE = 3
const MAT_ACCENT = 4
const MAT_HAIR = 5
const MAT_WHITE = 6

let hitMaterial = MAT_GROUND

const capsule = (
  px: number,
  py: number,
  pz: number,
  bx: number,
  by: number,
  bz: number,
  r: number,
) => {
  const bb = bx * bx + by * by + bz * bz
  let h = (px * bx + py * by + pz * bz) / bb
  h = h < 0 ? 0 : h > 1 ? 1 : h
  const dx = px - bx * h
  const dy = py - by * h
  const dz = pz - bz * h
  return Math.sqrt(dx * dx + dy * dy + dz * dz) - r
}

const roundBox = (
  px: number,
  py: number,
  pz: number,
  hx: number,
  hy: number,
  hz: number,
  r: number,
) => {
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

/** なめらかな和。胴と足のつなぎ目を丸める。 */
const smin = (a: number, b: number, k: number) => {
  const h = Math.max(k - Math.abs(a - b), 0) / k
  return Math.min(a, b) - h * h * k * 0.25
}

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

/**
 * 顔の造作の置き場。家の絵は 22x5 の升目に描いてあるので、その列と行をこちらの寸法へ写す。
 * 目は 5・13 列の 1 行目、頬は 2・18 列の 2 行目、口は 9 列の 3 行目、口ひげはその真上。
 * x は胴の中心から、y は BODY_Y からの隔たりで持つ。体つきで胴が伸びると造作も一緒に伸びる。
 */
const EYE_X = 0.38
const EYE_DY = 0.155
const CHEEK_X = 0.68
const CHEEK_DY = 0.005
const MOUTH_DY = -0.145
const MUSTACHE_DY = -0.08
const BROW_DY = 0.29

/** 顔の飾りを出す面の奥行き。胴の半奥行き BODY_HZ と丸めの和より、わずかに内へ入れる。 */
const FACE_Z = 0.56

/** 瞳は白目より前に出す。同じ面に置くと、白目に埋もれて見えない。 */
const PUPIL_Z = 0.015

/** 胴。角ばった箱で、横幅に対して背が低い。 */
const BODY_Y = 0.68
const BODY_HX = 0.78
const BODY_HY = 0.23
const BODY_HZ = 0.42
const BODY_ROUND = 0.15

/** 手。胴の前後の端から横へ張り出す。 */
const HAND_X = 0.97
const HAND_Y = 0.58

/** 体つきごとの胴の伸び。家の絵の ほそながい・ずんぐり と同じ向きに伸ばす。 */
const BODY_SHAPE = [
  { hx: 1, hy: 1 },
  { hx: 0.82, hy: 1.55 },
  { hx: 1.14, hy: 0.84 },
] as const

/** 白目の大きさ。家の絵の切れ込み 2 列 1.5 行ぶんにあたる。 */
const WHITE_RX = 0.085
const WHITE_RY = 0.115

/** 瞳。白目の中に収まる大きさで、白目より前に置く。 */
const PUPIL_RX = 0.045
const PUPIL_RY = 0.075

/** てん目の点。白目を持たず、この点だけが出る。 */
const DOT_RX = 0.05
const DOT_RY = 0.075

const EYE_RZ = 0.06

/**
 * 走者。境界球の外では球までの距離を返すので、遠い光線は体を数えずに進む。
 * 足は走りの位相で前後に振れ、跳んでいる間は畳む。
 */
const runner = (px0: number, py0: number, pz0: number, f: Frame) => {
  const scale = f.look.size
  const px = px0 / scale
  const py = (py0 - f.runnerY) / scale
  const pz = pz0 / scale
  const shape = BODY_SHAPE[f.look.body] ?? BODY_SHAPE[0]
  const bound = Math.sqrt(px * px + (py - BODY_Y) * (py - BODY_Y) + pz * pz) - 1.35
  if (bound > 0.15) return bound * scale

  let d = roundBox(px, py - BODY_Y, pz, BODY_HX * shape.hx, BODY_HY * shape.hy, BODY_HZ, BODY_ROUND)
  d = Math.min(d, roundBox(px - HAND_X, py - HAND_Y, pz, 0.11, 0.05, 0.22, 0.04))
  d = Math.min(d, roundBox(px + HAND_X, py - HAND_Y, pz, 0.11, 0.05, 0.22, 0.04))

  const airborne = Math.min(f.runnerY, 0.6) / 0.6
  for (const leg of LEGS) {
    const phase = f.stride + leg.phase
    const swing = Math.sin(phase) * 0.08 * (1 - airborne)
    // 前へ振り出した足だけ地面から浮かせる。跳んでいる間は 4 本とも畳む。
    const lift = Math.max(Math.cos(phase), 0) * 0.05 * (1 - airborne) + airborne * 0.1
    d = smin(d, capsule(px - leg.x, py - 0.42, pz - leg.z, swing, -0.42 + lift, 0, 0.09), 0.05)
  }
  return d * scale
}

/** 顔のどの造作に当たったか。face が返す距離と対で使う。 */
let faceMaterial = MAT_EYE

/**
 * 顔。白目と瞳・まぶた・頬・口・口ひげ・眉を、胴の前後の面から少しだけ出す。
 * 家の絵と同じ造作を同じ並びで置くので、走っていても同じ子の顔に見える。
 * 体とは色が違うので、体の距離関数とは分けて持つ。
 */
const face = (px0: number, py0: number, pz0: number, f: Frame) => {
  const scale = f.look.size
  const px = px0 / scale
  const py = (py0 - f.runnerY) / scale
  const pz = pz0 / scale
  faceMaterial = MAT_EYE
  // 造作は胴の面にしかない。そこから離れた光線は目も口も数えない。
  const bound = Math.sqrt(px * px + (py - BODY_Y) * (py - BODY_Y) + pz * pz) - 1.05
  if (bound > 0.12) return bound * scale

  const shape = BODY_SHAPE[f.look.body] ?? BODY_SHAPE[0]
  // 造作は胴と一緒に伸び縮みする。伸ばさないと、ほそながい体では顔から外れる。
  const fx = (x: number) => x * shape.hx
  const fy = (dy: number) => BODY_Y + dy * shape.hy
  const dot = f.look.eye === 3
  let d = Number.POSITIVE_INFINITY
  let m = MAT_EYE
  const nearer = (v: number, material: number) => {
    if (v < d) {
      d = v
      m = material
    }
  }
  for (const z of [FACE_Z, -FACE_Z]) {
    const front = z > 0 ? 1 : -1
    for (const side of [1, -1]) {
      const ex = fx(EYE_X * side)
      const ey = fy(EYE_DY)
      if (f.look.blink) {
        // まばたき。家の絵と同じく、閉じた目は下線 1 本だけになる。
        nearer(capsule(px - ex + WHITE_RX, py - ey + WHITE_RY, pz - z, WHITE_RX * 2, 0, 0, 0.028), MAT_EYE)
      } else if (dot) {
        // てん目は白目を持たない。点だけが顔に乗る。
        nearer(ellipsoid(px - ex, py - ey, pz - z, DOT_RX, DOT_RY, EYE_RZ), MAT_EYE)
      } else {
        nearer(ellipsoid(px - ex, py - ey, pz - z, WHITE_RX, WHITE_RY, EYE_RZ), MAT_WHITE)
        nearer(
          ellipsoid(px - ex, py - ey, pz - z - front * PUPIL_Z, PUPIL_RX, PUPIL_RY, EYE_RZ),
          MAT_EYE,
        )
      }
      // たれ目は目尻の下、つり目は目尻の上へまぶたを引く。まる目とてん目には引かない。
      if ((f.look.eye === 1 || f.look.eye === 2) && !f.look.blink) {
        const lidY = f.look.eye === 1 ? ey - WHITE_RY - 0.03 : ey + WHITE_RY + 0.03
        const tilt = f.look.eye === 1 ? -0.05 : 0.05
        nearer(capsule(px - ex - side * 0.02, py - lidY, pz - z, side * 0.13, tilt, 0, 0.03), MAT_EYE)
      }
      // 頬。家の絵と同じく、目の外側に色の違う点が付く。
      nearer(
        ellipsoid(px - fx(CHEEK_X * side), py - fy(CHEEK_DY), pz - z, 0.075, 0.05, 0.05),
        MAT_ACCENT,
      )
      // 眉。おじいさんにだけ、白いものが目の上に生える。
      if (f.look.white) {
        nearer(
          capsule(px - ex + WHITE_RX, py - fy(BROW_DY), pz - z, WHITE_RX * 2, 0, 0, 0.028),
          MAT_HAIR,
        )
      }
    }
    // 口。具合が悪いとへの字になる。家の絵と同じで、両端だけが持ち上がる。
    const my = fy(MOUTH_DY)
    if (f.look.gloomy) {
      nearer(capsule(px - 0.085, py - my, pz - z, 0.085, 0.045, 0, 0.026), MAT_EYE)
      nearer(capsule(px, py - my, pz - z, 0.085, -0.045, 0, 0.026), MAT_EYE)
    } else {
      nearer(capsule(px + 0.085, py - my, pz - z, 0.17, 0, 0, 0.026), MAT_EYE)
    }
    // 口ひげは口の真上。おじさんから生え、おじいさんになると白くなる。
    if (f.look.mustache) {
      nearer(capsule(px + 0.17, py - fy(MUSTACHE_DY), pz - z, 0.34, 0, 0, 0.036), MAT_HAIR)
    }
  }
  faceMaterial = m
  return d * scale
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
  let d = runner(px, py, pz, f)
  let m = MAT_RUNNER
  const e = face(px, py, pz, f)
  if (e < d) {
    d = e
    m = faceMaterial
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

type Rgb = readonly [number, number, number]

const GROUND_COLOR: Rgb = [0.78, 0.63, 0.42]
const CACTUS_COLOR: Rgb = [0.25, 0.49, 0.38]
const EYE_COLOR: Rgb = [0.06, 0.05, 0.05]
const HAIR_COLOR: Rgb = [0.95, 0.94, 0.92]

/** 白目。家の絵と同じ、わずかに灰色がかった白。 */
const WHITE_COLOR: Rgb = [0.97, 0.97, 0.97]

/** 0x00RRGGBB を 0..1 の 3 つ組に開く。姿の色は Claudeっち本人から来る。 */
const rgbOf = (color: number): Rgb => [
  ((color >> 16) & 255) / 255,
  ((color >> 8) & 255) / 255,
  (color & 255) / 255,
]

/** 素材ごとの色。走者と頬とひげはその子の姿で変わる。 */
const colorsOf = (look: Look): Rgb[] => [
  GROUND_COLOR,
  rgbOf(look.color),
  CACTUS_COLOR,
  EYE_COLOR,
  rgbOf(look.accent),
  look.white ? HAIR_COLOR : EYE_COLOR,
  WHITE_COLOR,
]

let colors: Rgb[] = []

const SKY_TOP: Rgb = [0.11, 0.14, 0.22]
const SKY_LOW: Rgb = [0.29, 0.31, 0.41]

/** 影を探す距離。光は斜めなので、物からこれだけ離れた地面までは影が伸びうる。 */
const SHADOW_RANGE = 6

/**
 * 地面の一点が影に入っているか。光へ向かって距離関数を進め、遮られたら影とする。
 * 丸い暗がりを描く代わりに実際の遮りを見るので、カメラが回っても形が破綻しない。
 *
 * 半影は作らない。1 画素が世界の数センチを受け持つ粗さなので、濃淡を付けると縁が斑に散る。
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
  const base = colors[material] ?? GROUND_COLOR
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
    const lane = Math.abs(pz) < 2.4 ? 0.82 : 1
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
// 走者の色はその子ごとに違うので、姿が変わったときだけ組み直す。
let palette: number[] = []
let paletteKey = ''

const buildPalette = (look: Look) => {
  const key = `${look.color}:${look.accent}:${look.white ? 1 : 0}`
  if (key === paletteKey) return
  paletteKey = key
  colors = colorsOf(look)
  const out: number[] = []
  const push = (r: number, g: number, b: number) => {
    const to = (v: number) => Math.max(0, Math.min(255, Math.round(v * 255)))
    out.push((to(r) << 16) | (to(g) << 8) | to(b))
  }
  // 段の幅を細かく取る。粗いと、なだらかな陰影に段の輪が出る。
  for (const c of colors) {
    for (let i = 0; i < 16; i += 1) {
      const k = 0.2 + (i * (1.28 - 0.2)) / 15
      push(c[0] * k, c[1] * k, c[2] * k)
    }
  }
  for (let i = 0; i <= 12; i += 1) {
    const k = i / 12
    push(
      SKY_LOW[0] * (1 - k) + SKY_TOP[0] * k,
      SKY_LOW[1] * (1 - k) + SKY_TOP[1] * k,
      SKY_LOW[2] * (1 - k) + SKY_TOP[2] * k,
    )
  }
  push(1, 1, 1)
  palette = out
}

const quantize = (r: number, g: number, b: number) => {
  const ri = Math.max(0, Math.min(255, r * 255))
  const gi = Math.max(0, Math.min(255, g * 255))
  const bi = Math.max(0, Math.min(255, b * 255))
  let best = 0
  let bestD = Number.POSITIVE_INFINITY
  for (const c of palette) {
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
    const n = (a << 16) | (b << 8) | c
    out += B64[(n >> 18) & 63]
    out += B64[(n >> 12) & 63]
    out += i + 1 < bytes.length ? B64[(n >> 6) & 63] : '='
    out += i + 2 < bytes.length ? B64[n & 63] : '='
  }
  return out
}

const HALF_BLOCK = 0x2580 // ▀ 上半分。前景が上のピクセル、背景が下のピクセル。

/** 画素を並べた配列を返す。高さはセル数の 2 倍で、上下 1 組が 1 セルになる。 */
export const renderPixels = (columns: number, rows: number, f: Frame) => {
  buildPalette(f.look)
  const width = columns
  const height = rows * 2
  const angle = f.orbit
  const depth = Math.max(0, Math.min(f.depth, 1))
  const radius = FAR_RADIUS + (NEAR_RADIUS - FAR_RADIUS) * depth
  const view = viewHeight(height)
  const aspect = width / height
  const shift = (view / 2) * aspect * TARGET_SHIFT_RATIO * Math.cos(angle)
  // 注視点は走者の横へずらす。ずらす向きは周回角だけで決まる。
  const tx = Math.cos(angle) * shift
  const tz = -Math.sin(angle) * shift
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
  let rx = -fz
  let ry = 0
  let rz = fx
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

  for (let y = 0; y < height; y += 1) {
    for (let x = 0; x < width; x += 1) {
      const sx = ((x + 0.5) / width) * 2 - 1
      const sy = 1 - ((y + 0.5) / height) * 2
      const cx = sx * scale * aspect
      const cy = sy * scale
      let dx = fx + rx * cx + ux * cy
      let dy = fy + ry * cx + uy * cy
      let dz = fz + rz * cx + uz * cy
      const dl = Math.sqrt(dx * dx + dy * dy + dz * dz)
      dx /= dl
      dy /= dl
      dz /= dl
      trace(ex, ey, ez, dx, dy, dz, f, fogStart, tStart, pixelAngle)
      pixels[y * width + x] = quantize(outR, outG, outB)
    }
  }
  return pixels
}

/** Raster の cells。1 セルに上下 2 画素を半ブロックで詰める。 */
export const encodeCells = (columns: number, rows: number, pixels: Uint32Array) => {
  const words = new Uint32Array(columns * rows * 3)
  for (let row = 0; row < rows; row += 1) {
    for (let col = 0; col < columns; col += 1) {
      const at = (row * columns + col) * 3
      words[at] = HALF_BLOCK
      words[at + 1] = pixels[row * 2 * columns + col] ?? 0
      words[at + 2] = pixels[(row * 2 + 1) * columns + col] ?? 0
    }
  }
  return base64(new Uint8Array(words.buffer))
}

export const render = (columns: number, rows: number, f: Frame) =>
  encodeCells(columns, rows, renderPixels(columns, rows, f))
