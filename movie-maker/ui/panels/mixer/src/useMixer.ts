import { useCallback, useEffect, useMemo, useRef, useState } from "preact/hooks";
import type { MixerApi, MixerState, Op } from "@mm/sdk";
import { MeterEngine } from "./meterEngine.ts";

const message = (e: unknown): string => (e instanceof Error ? e.message : String(e));

export interface MixerHook {
  state: MixerState | null;
  error: string | null;
  /** 操作が拒否されるたびに増える。手元の値を持つ入力欄を作り直して、読み直した値に戻す合図。 */
  epoch: number;
  /** 操作を順に送り、終わるたびに state を読み直す。 */
  apply: (ops: Op[]) => void;
}

/** state の読み込みと操作の送信。操作は順番に送り、古い応答で新しい state を上書きしない。 */
export const useMixer = (api: MixerApi, refreshKey: number): MixerHook => {
  const [state, setState] = useState<MixerState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [epoch, setEpoch] = useState(0);
  const loads = useRef(0);
  const chain = useRef<Promise<void>>(Promise.resolve());

  const load = useCallback(async () => {
    const id = ++loads.current;
    try {
      const next = await api.state();
      if (id === loads.current) setState(next);
    } catch (e) {
      if (id === loads.current) setError(message(e));
    }
  }, [api]);

  useEffect(() => {
    void load();
  }, [load, refreshKey]);

  const apply = useCallback(
    (ops: Op[]) => {
      chain.current = chain.current.then(async () => {
        try {
          await api.applyOps(ops);
          setError(null);
        } catch (e) {
          setError(message(e));
          setEpoch((n) => n + 1);
        }
        await load();
      });
    },
    [api, load],
  );
  return { state, error, epoch, apply };
};

/**
 * メーターの購読と描画のループ。値は state に入れず MeterEngine が DOM を直接書く。
 * 購読の解除は非同期で返るので、先に外れたら返ってきた時点で解除する。
 */
export const useMeters = (api: MixerApi): MeterEngine => {
  const engine = useMemo(() => new MeterEngine(), []);
  useEffect(() => {
    let off: (() => void) | null = null;
    let alive = true;
    api.onMeters((m) => engine.push(m)).then(
      (f) => {
        if (alive) off = f;
        else f();
      },
      () => undefined,
    );
    let raf = 0;
    const tick = (now: number) => {
      engine.frame(now);
      raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => {
      alive = false;
      off?.();
      cancelAnimationFrame(raf);
    };
  }, [api, engine]);
  return engine;
};
