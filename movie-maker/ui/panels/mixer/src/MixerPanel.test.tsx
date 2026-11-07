// @vitest-environment jsdom
import { render } from "preact";
import { act } from "preact/test-utils";
import type { LayerAudio, Op } from "@mm/sdk";
import type { MixerApi, MixerMeters, MixerState, MixerTrack } from "./api.ts";
import { afterEach, describe, expect, it, vi } from "vitest";
import { MixerPanel } from "./MixerPanel.tsx";

const host = document.createElement("div");
document.body.append(host);
afterEach(() => render(null, host));

const num = (value: number) => ({ value: { type: "number" as const, value } });
const audio = (over: Partial<LayerAudio> = {}): LayerAudio => ({ role: "voice", gain_db: num(-3), pan: num(0), ...over });
const track = (layer: string, over: Partial<MixerTrack> = {}): MixerTrack => ({
  layer,
  kind: "footage",
  label: `Clip ${layer}`,
  audio: audio(),
  animated: false,
  ...over,
});
const stateOf = (over: Partial<MixerState> = {}): MixerState => ({
  composition: "main",
  mixer: null,
  tracks: [track("a"), track("b", { audio: null })],
  roles: ["voice", "bgm", "sfx"],
  ...over,
});

const settle = () => act(() => new Promise<void>((ok) => setTimeout(ok, 0)));

const mount = async (state: MixerState, over: Partial<MixerApi> = {}) => {
  let push: (m: MixerMeters) => void = () => undefined;
  const api: MixerApi = {
    state: vi.fn().mockResolvedValue(state),
    applyOps: vi.fn().mockResolvedValue(undefined),
    onMeters: vi.fn().mockImplementation(async (h) => {
      push = h;
      return () => {};
    }),
    ...over,
  };
  act(() => render(<MixerPanel api={api} refreshKey={0} />, host));
  await settle();
  return { api, push: (m: MixerMeters) => act(() => push(m)) };
};

const byLabel = <T extends Element>(label: string) => host.querySelector(`[aria-label="${label}"]`) as unknown as T;
const flush = (fn: () => void) =>
  act(async () => {
    fn();
    await new Promise<void>((ok) => setTimeout(ok, 0));
  });
const change = (el: HTMLInputElement | HTMLSelectElement, value: string) =>
  flush(() => {
    el.value = value;
    el.dispatchEvent(new Event("input", { bubbles: true }));
    el.dispatchEvent(new Event("change", { bubbles: true }));
  });
const click = (el: Element) => flush(() => (el as HTMLElement).click());
const fire = (el: Element, event: Event) => flush(() => void el.dispatchEvent(event));
const lastOps = (api: MixerApi) => vi.mocked(api.applyOps).mock.calls.at(-1)?.[0];

describe("トラック", () => {
  it("フェーダーはドラッグ中は送らず、離したときに全体を 1 回だけ送る", async () => {
    const { api } = await mount(stateOf());
    const fader = byLabel<HTMLInputElement>("Volume of Clip a");
    act(() => {
      fader.value = "-10";
      fader.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(api.applyOps).not.toHaveBeenCalled();
    expect(host.textContent).toContain("-10.0 dB");
    await fire(fader, new Event("change", { bubbles: true }));
    expect(api.applyOps).toHaveBeenCalledTimes(1);
    expect(lastOps(api)).toEqual([{ op: "setLayerAudio", layer: "a", audio: audio({ gain_db: num(-10) }) }]);
  });

  it("音の設定が無いトラックは既定から作る", async () => {
    const { api } = await mount(stateOf());
    await click(byLabel("Mute Clip b"));
    expect(lastOps(api)).toEqual([
      { op: "setLayerAudio", layer: "b", audio: { role: "voice", gain_db: num(0), pan: num(0), mute: true } },
    ]);
  });

  it("ダブルクリックで音量は 0 dB、パンは中央に戻る", async () => {
    const { api } = await mount(stateOf({ tracks: [track("a", { audio: audio({ pan: num(0.5) }) })] }));
    await fire(byLabel("Volume of Clip a"), new MouseEvent("dblclick", { bubbles: true }));
    expect(lastOps(api)).toEqual([{ op: "setLayerAudio", layer: "a", audio: audio({ gain_db: num(0), pan: num(0.5) }) }]);
    await fire(byLabel("Pan of Clip a"), new MouseEvent("dblclick", { bubbles: true }));
    expect(lastOps(api)).toEqual([{ op: "setLayerAudio", layer: "a", audio: audio({ gain_db: num(-3), pan: num(0) }) }]);
  });

  it("ソロと送り先のバスを変える", async () => {
    const { api } = await mount(stateOf());
    await click(byLabel("Solo Clip a"));
    expect(lastOps(api)).toEqual([{ op: "setLayerAudio", layer: "a", audio: audio({ solo: true }) }]);
    await change(byLabel<HTMLSelectElement>("Bus for Clip a"), "bgm");
    expect(lastOps(api)).toEqual([{ op: "setLayerAudio", layer: "a", audio: audio({ role: "bgm" }) }]);
  });

  it("キーフレームのあるトラックは音量とパンを触らせず、ミュートは触れる", async () => {
    await mount(stateOf({ tracks: [track("a", { animated: true })] }));
    expect(byLabel<HTMLInputElement>("Volume of Clip a").disabled).toBe(true);
    expect(byLabel<HTMLInputElement>("Pan of Clip a").disabled).toBe(true);
    expect(byLabel<HTMLButtonElement>("Mute Clip a").disabled).toBe(false);
  });
});

describe("マスター、バス、ダッキング", () => {
  it("マスターとバスの音量は setMixer で全体を送る", async () => {
    const { api } = await mount(stateOf({ mixer: { master_db: -1, buses: { bgm: { gain_db: -6 } } } }));
    await change(byLabel("Master volume"), "-4");
    expect(lastOps(api)).toEqual([{ op: "setMixer", composition: "main", mixer: { master_db: -4, buses: { bgm: { gain_db: -6 } } } }]);
    await change(byLabel("sfx bus volume"), "-2");
    expect(lastOps(api)).toEqual([
      { op: "setMixer", composition: "main", mixer: { master_db: -1, buses: { bgm: { gain_db: -6 }, sfx: { gain_db: -2 } } } },
    ]);
  });

  it("ダッキングを有効にすると既定値で送り、無効にすると null を送る", async () => {
    const { api } = await mount(stateOf());
    const check = host.querySelector<HTMLInputElement>("[aria-label=Ducking] input[type=checkbox]") as HTMLInputElement;
    await click(check);
    expect(lastOps(api)).toEqual([
      {
        op: "setMixer",
        composition: "main",
        mixer: { ducking: { trigger: "voice", targets: ["bgm"], reduction_db: -12, threshold_db: -40, attack_ms: 20, release_ms: 300 } },
      },
    ]);
  });

  it("きっかけを変えると、対象から外し、量は範囲に収める", async () => {
    const ducking = { trigger: "voice", targets: ["bgm", "sfx"], reduction_db: -12, threshold_db: -40, attack_ms: 20, release_ms: 300 };
    const { api } = await mount(stateOf({ mixer: { ducking } }));
    await change(host.querySelector("#mm-mixer-trigger") as HTMLSelectElement, "bgm");
    expect(lastOps(api)).toEqual([{ op: "setMixer", composition: "main", mixer: { ducking: { ...ducking, trigger: "bgm", targets: ["sfx"] } } }]);
    await change(byLabel("Reduction (dB)"), "6");
    expect(vi.mocked(api.applyOps).mock.calls.at(-1)?.[0]).toMatchObject([{ mixer: { ducking: { reduction_db: 0 } } }]);
  });
});

describe("メーター", () => {
  const meters = (over: Partial<MixerMeters> = {}): MixerMeters => ({
    t: 1,
    playing: true,
    tracks: [{ layer: "a", peak_db: -12 }],
    master: { peak_db: [-6, -30], true_peak_db: -5.4, short_term_lufs: -16.2, integrated_lufs: null },
    ...over,
  });

  it("イベントでは読み出しの数字だけを書き換え、パネルを描き直さない", async () => {
    const { api, push } = await mount(stateOf());
    const row = host.querySelector(".mm-mixer-track");
    push(meters());
    expect(host.querySelector(".mm-mixer-readout b")?.textContent).toBe("-5.4");
    expect(host.querySelectorAll(".mm-mixer-readout b")[2]?.textContent).toBe("--");
    expect(host.querySelector(".mm-mixer-track")).toBe(row);
    expect(api.state).toHaveBeenCalledTimes(1);
  });
});

describe("拒否と読み直し", () => {
  it("操作が拒否されたら理由を出し、state を読み直して手元の値を戻す", async () => {
    const applyOps = vi.fn().mockRejectedValue(new Error("layer a is locked"));
    const { api } = await mount(stateOf(), { applyOps });
    await change(byLabel("Volume of Clip a"), "-20");
    await settle();
    expect(host.querySelector("[role=alert]")?.textContent).toBe("layer a is locked");
    expect(api.state).toHaveBeenCalledTimes(2);
    expect(byLabel<HTMLInputElement>("Volume of Clip a").value).toBe("-3");
  });

  it("応答を待つ間に続けて押した変更は、前の変更を読み直した state から作り、前の変更を消さない", async () => {
    let current = stateOf();
    let release: () => void = () => undefined;
    const applyOps = vi.fn().mockImplementationOnce(
      (ops: Op[]) =>
        new Promise<void>((ok) => {
          release = () => {
            const set = ops[0] as Extract<Op, { op: "setLayerAudio" }>;
            current = stateOf({ tracks: [track("a", { audio: set.audio }), track("b", { audio: null })] });
            ok();
          };
        }),
    );
    applyOps.mockResolvedValue(undefined);
    const { api } = await mount(current, { applyOps, state: vi.fn(async () => current) });
    await click(byLabel("Mute Clip a"));
    await click(byLabel("Solo Clip a"));
    expect(api.applyOps).toHaveBeenCalledTimes(1);
    await flush(() => release());
    await settle();
    expect(lastOps(api)).toEqual([{ op: "setLayerAudio", layer: "a", audio: audio({ mute: true, solo: true }) }]);
  });

  it("成功したら state を読み直し、エラーを消す", async () => {
    const { api } = await mount(stateOf());
    await change(byLabel("Master volume"), "-4");
    await settle();
    expect(api.state).toHaveBeenCalledTimes(2);
    expect(host.querySelector("[role=alert]")).toBeNull();
  });
});
