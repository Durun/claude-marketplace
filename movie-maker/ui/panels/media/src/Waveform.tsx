import { useEffect, useState } from "preact/hooks";
import type { MediaApi, WaveformView } from "./api.ts";
import { formatDuration } from "./format.ts";

/** 画面に出すピークの数。パネルの幅より細かくても見えない。 */
export const WAVEFORM_BUCKETS = 120;
/** 波形の縦の座標。中心が 1、上下に振れる。 */
const CENTER = 1;
const HEIGHT = 2;
const HALF = 0.5;

/** ピークの包絡線を 1 つの閉じた図形にする。 */
export const envelopePath = (peaks: number[]): string => {
  const top = peaks.map((p, i) => `L${i + HALF},${CENTER - Math.min(1, p)}`);
  const bottom = peaks.map((p, i) => `L${i + HALF},${CENTER + Math.min(1, p)}`).reverse();
  return `M0,${CENTER}${top.join("")}L${peaks.length},${CENTER}${bottom.join("")}Z`;
};

export interface WaveformProps {
  api: MediaApi;
  id: string;
}

/** 選んだ素材の波形。読めないときは理由を出す。 */
export const Waveform = ({ api, id }: WaveformProps) => {
  const [view, setView] = useState<WaveformView | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    setView(null);
    setError(null);
    api
      .waveform(id, WAVEFORM_BUCKETS)
      .then((v) => alive && setView(v))
      .catch((e: unknown) => alive && setError(String(e)));
    return () => {
      alive = false;
    };
  }, [api, id]);

  if (error) return <p class="mm-media-note">{error}</p>;
  if (!view) return <p class="mm-media-note">Loading waveform…</p>;
  return (
    <figure class="mm-media-wave">
      <svg
        class="mm-media-wave-svg"
        viewBox={`0 0 ${view.peaks.length} ${HEIGHT}`}
        preserveAspectRatio="none"
        role="img"
        aria-label="Waveform"
      >
        <path d={envelopePath(view.peaks)} />
      </svg>
      <figcaption class="mm-media-wave-caption">{formatDuration(view.duration)}</figcaption>
    </figure>
  );
};
