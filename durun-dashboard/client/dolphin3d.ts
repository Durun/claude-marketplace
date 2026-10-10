// three.js で描く、Claude の色のイルカ。体を尾に向かって波打たせて泳ぐ。
import {
  BufferAttribute, type BufferGeometry, CatmullRomCurve3, Color, DirectionalLight, ExtrudeGeometry, Group, HemisphereLight, LatheGeometry, MathUtils, Mesh,
  MeshBasicMaterial, MeshPhysicalMaterial, MeshStandardMaterial, PerspectiveCamera, Scene, Shape, SphereGeometry, SplineCurve, TubeGeometry, Vector2, Vector3,
  WebGLRenderer,
} from 'three';

const ORANGE = 0xd97757; // Claude の色
const BACK = 0xc15f3c; // 背中とひれを少し濃くして、丸みを見せる
const BELLY = 0xf7e9dc;

// 尾（-1）から口先までの体の半径。頭を大きめにして、かわいく見せる。
const PROFILE: [number, number][] = [
  [-1.0, 0.0], [-0.96, 0.07], [-0.8, 0.12], [-0.55, 0.21], [-0.25, 0.32], [0.05, 0.38], [0.3, 0.39],
  [0.5, 0.35], [0.66, 0.27], [0.78, 0.16], [0.92, 0.11], [1.02, 0.08], [1.07, 0.0],
];
const FLATTEN = 0.88; // 横から見て丸く、上から見て細い体にする
const TAIL_X = -0.98;
const DORSAL_X = 0.02;

// 波は頭では 0 で、尾に向かって大きくなる。体の長さに対して 1 回弱うねるくらいにする。
const AMPLITUDE = 0.09;
const WAVE_NUMBER = 2.6;
const STROKES_PER_SECOND = 1.1;
const wave = (x: number, phase: number): number => {
  const e = Math.max(0, (0.3 - x) / 1.3);
  return AMPLITUDE * e * e * Math.sin(phase + WAVE_NUMBER * x);
};

/** yaw は 0 で右向き、-π で左向き、-π/2 で正面向き。pitch は口先を上げる回転、roll は体の軸まわりの回転。stroke は尾を打つ速さ（0 で止まる）。 */
export type Pose = { yaw: number; pitch: number; roll: number; stroke: number };

const bodyGeometry = (): BufferGeometry => {
  const outline = new SplineCurve(PROFILE.map(([x, r]) => new Vector2(r, x))).getPoints(64).map((p) => new Vector2(Math.max(0, p.x), p.y));
  // 回転体の軸（y）を、体の前後（x）に向ける。
  const g = new LatheGeometry(outline, 40).rotateZ(-Math.PI / 2).scale(1, 1, FLATTEN);
  const normal = g.getAttribute('normal');
  const colors = new Float32Array(normal.count * 3);
  const c = new Color();
  for (let i = 0; i < normal.count; i++) {
    const up = normal.getY(i);
    c.set(BELLY).lerp(new Color(ORANGE), MathUtils.smoothstep(up, -0.6, -0.2)).lerp(new Color(BACK), MathUtils.smoothstep(up, 0.5, 0.95));
    c.toArray(colors, i * 3);
  }
  g.setAttribute('color', new BufferAttribute(colors, 3));
  return g;
};

/** points は始点に続けて、二次曲線の制御点と終点を交互に並べる。厚みの中心を z = 0 にそろえる。 */
const finMesh = (points: [number, number][], depth: number, material: MeshPhysicalMaterial): Mesh => {
  const shape = new Shape();
  shape.moveTo(...(points[0] as [number, number]));
  for (let i = 1; i + 1 < points.length; i += 2) shape.quadraticCurveTo(...(points[i] as [number, number]), ...(points[i + 1] as [number, number]));
  const g = new ExtrudeGeometry(shape, { depth, bevelEnabled: true, bevelThickness: depth / 2, bevelSize: depth / 2, bevelSegments: 3, curveSegments: 16 });
  return new Mesh(g.translate(0, 0, -depth / 2), material);
};

const sphere = (radius: number, material: MeshStandardMaterial | MeshBasicMaterial, x: number, y: number, z: number): Mesh => {
  const m = new Mesh(new SphereGeometry(radius, 24, 16), material);
  m.position.set(x, y, z);
  return m;
};

/** 体の表面の点。angle は真上から測った、体の周りの角度。 */
const surface = (x: number, angle: number): Vector3 => {
  const i = PROFILE.findIndex(([px]) => px >= x);
  const [x0, r0] = PROFILE[Math.max(0, i - 1)] as [number, number];
  const [x1, r1] = PROFILE[Math.max(0, i)] as [number, number];
  const r = x1 === x0 ? r0 : r0 + ((r1 - r0) * (x - x0)) / (x1 - x0);
  return new Vector3(x, r * Math.cos(angle), r * Math.sin(angle) * FLATTEN);
};

/** 口先から目の下へ、端を上げた線を引いて笑顔にする。 */
const smile = (): Group => {
  const ink = new MeshBasicMaterial({ color: 0x5a2a1a });
  const g = new Group();
  for (const side of [1, -1]) {
    const points = ([[1.0, 100], [0.9, 104], [0.8, 101], [0.7, 86]] as const).map(([x, deg]) => surface(x, (side * deg * Math.PI) / 180));
    g.add(new Mesh(new TubeGeometry(new CatmullRomCurve3(points), 24, 0.016, 6), ink));
  }
  return g;
};

/** 白目、黒目、光を、頭の左右に置く。 */
const eyes = (): Group => {
  const white = new MeshStandardMaterial({ color: 0xffffff, roughness: 0.25 });
  const pupil = new MeshStandardMaterial({ color: 0x241612, roughness: 0.2 });
  const glint = new MeshBasicMaterial({ color: 0xffffff });
  const g = new Group();
  for (const side of [1, -1]) {
    g.add(sphere(0.105, white, 0.6, 0.11, 0.2 * side), sphere(0.062, pupil, 0.64, 0.13, 0.268 * side), sphere(0.02, glint, 0.665, 0.17, 0.315 * side));
  }
  return g;
};

/** canvas にイルカを置く。返す draw を毎フレーム呼んで泳がせる。 */
export const createDolphin = (canvas: HTMLCanvasElement, width: number, height: number) => {
  const renderer = new WebGLRenderer({ canvas, alpha: true, antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  renderer.setSize(width, height, false);
  const camera = new PerspectiveCamera(30, width / height, 0.1, 20);
  // 少し上から見下ろして、横向きでも尾びれの形が見えるようにする。
  camera.position.set(0, 1.5, 5.1);
  camera.lookAt(0, -0.05, 0);
  const key = new DirectionalLight(0xffffff, 2.2);
  key.position.set(2, 3, 4);
  const rim = new DirectionalLight(0xffe2cc, 1.0);
  rim.position.set(-3, 2, -3);
  const scene = new Scene().add(new HemisphereLight(0xffffff, 0x8a6f60, 1.6), key, rim);

  const skin = new MeshPhysicalMaterial({ vertexColors: true, roughness: 0.42, clearcoat: 0.6, clearcoatRoughness: 0.3 });
  const finSkin = new MeshPhysicalMaterial({ color: BACK, roughness: 0.45, clearcoat: 0.5, clearcoatRoughness: 0.3 });
  const body = new Mesh(bodyGeometry(), skin);
  const rest = Float32Array.from(body.geometry.getAttribute('position').array);
  const dorsal = finMesh([[0.12, 0], [0.04, 0.22], [-0.2, 0.31], [-0.12, 0.14], [-0.22, 0]], 0.05, finSkin);
  dorsal.position.set(DORSAL_X, 0.3, 0);
  const fluke = new Group().add(finMesh([[0.05, 0], [-0.06, 0.13], [-0.24, 0.32], [-0.16, 0.1], [-0.25, 0], [-0.16, -0.1], [-0.24, -0.32], [-0.06, -0.13], [0.05, 0]], 0.04, finSkin).rotateX(Math.PI / 2));
  fluke.position.x = TAIL_X;
  const dolphin = new Group().add(body, dorsal, fluke, eyes(), smile());
  for (const side of [1, -1]) {
    const flipper = finMesh([[0.08, 0], [0.02, 0.15], [-0.14, 0.22], [-0.06, 0.08], [-0.1, 0]], 0.04, finSkin);
    flipper.position.set(0.32, -0.17, 0.24 * side);
    flipper.rotation.set(side * (Math.PI / 2 + 0.5), 0, -0.35);
    dolphin.add(flipper);
  }
  scene.add(dolphin);

  let phase = 0;
  /** 体の頂点を波に合わせてずらし、尾びれを尾の傾きに合わせる。dt は前回からの秒数。 */
  const draw = (dt: number, pose: Pose): void => {
    phase += dt * 2 * Math.PI * STROKES_PER_SECOND * pose.stroke;
    const position = body.geometry.getAttribute('position') as BufferAttribute;
    // 揺れは小さいので、法線は作り直さずに元の形のものを使う。
    for (let i = 0; i < position.count; i++) position.setY(i, (rest[i * 3 + 1] as number) + wave(rest[i * 3] as number, phase));
    position.needsUpdate = true;
    dorsal.position.y = 0.3 + wave(DORSAL_X, phase);
    fluke.position.y = wave(TAIL_X, phase);
    fluke.rotation.z = Math.atan((wave(TAIL_X, phase) - wave(TAIL_X + 0.05, phase)) / -0.05);
        // 体の軸まわりに回し、口先を上げ下げしてから左右へ向ける順にする。
    dolphin.rotation.set(pose.roll, pose.yaw, pose.pitch, 'YZX');
    renderer.render(scene, camera);
  };
  return { draw, dispose: () => renderer.dispose() };
};
