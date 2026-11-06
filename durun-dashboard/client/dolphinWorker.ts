// イルカの 3D を、画面のスレッドの外で描く。WebGL の準備とシェーダーの組み立ては、混んだマシンで 0.5 秒ほど画面を固めるため。
import { createDolphin, type Pose } from './dolphin3d.ts';

/** 画面から届く知らせ。最初に canvas を渡し、その後は毎フレームの姿勢を送る。 */
export type DolphinMessage = { canvas: OffscreenCanvas; width: number; height: number; pixelRatio: number } | { dt: number; pose: Pose };

let view: ReturnType<typeof createDolphin> | undefined;
// 準備の間や描くのが遅れた間に届いた姿勢は、最後のものだけ描き、経った時間は足し合わせる。
let pending: { dt: number; pose: Pose } | undefined;

const loop = () => {
  if (view && pending) view.draw(pending.dt, pending.pose);
  pending = undefined;
  requestAnimationFrame(loop);
};

addEventListener('message', (e: MessageEvent<DolphinMessage>) => {
  const m = e.data;
  if ('pose' in m) {
    pending = { dt: (pending?.dt ?? 0) + m.dt, pose: m.pose };
    return;
  }
  try {
    view = createDolphin(m.canvas, m.width, m.height, m.pixelRatio);
  } catch {
    // GPU が使えないなどで WebGL を使えないときは、描けないことを画面に知らせる。
    postMessage('failed');
    return;
  }
  requestAnimationFrame(loop);
});
