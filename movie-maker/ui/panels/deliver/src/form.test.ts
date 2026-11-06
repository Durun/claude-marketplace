import { describe, expect, it } from "vitest";
import type { JsonSchema, RenderTarget } from "@mm/sdk";
import { draftOf, extraFields, fieldSpecs, parseRational, targetOf, withRequired, type FieldSpec } from "./form.ts";
import { defaultComposition, PRESETS, targetFromPreset, uniqueId, type Preset } from "./presets.ts";

const target: RenderTarget = {
  id: "yt",
  composition: "main",
  width: 1920,
  height: 1080,
  fps: { num: 30000, den: 1001 },
  codec: "h264",
  output_path: "out/yt.mp4",
};

describe("fieldSpecs", () => {
  const specs = fieldSpecs();

  it("定番の欄を決まった順で先頭に並べる", () => {
    expect(specs.map((s) => s.key).slice(0, 7)).toEqual(["id", "composition", "width", "height", "fps", "codec", "output_path"]);
  });

  it("欄の種類をスキーマの形から決める", () => {
    const kind = (key: string) => specs.find((s) => s.key === key)?.control.kind;
    expect(kind("width")).toBe("integer");
    expect(kind("fps")).toBe("rational");
    expect(kind("loudness_lufs")).toBe("number");
    expect(kind("composition")).toBe("composition");
    expect(kind("output_path")).toBe("output");
    expect(specs.find((s) => s.key === "loudness_lufs")?.required).toBe(false);
  });

  it("スキーマに足された欄は、直さなくても欄になる", () => {
    const schema: JsonSchema = {
      type: "object",
      required: ["id", "subtitles"],
      properties: {
        id: { type: "string" },
        subtitles: { type: "string", enum: ["none", "burn", "sidecar"] },
        chapters: { type: ["boolean", "null"] },
        range: { type: "object", properties: { start: { type: "number" }, mode: { type: "string", enum: ["a", "b"] } }, required: ["start"] },
        meta: { type: "object", additionalProperties: { type: "string" } },
      },
    };
    const added = fieldSpecs(schema);
    expect(added.map((s) => [s.key, s.control.kind, s.required])).toEqual([
      ["id", "text", true],
      ["subtitles", "choice", true],
      ["chapters", "bool", false],
      ["range.start", "number", false],
      ["range.mode", "choice", false],
      ["meta", "json", false],
    ]);
    expect(added[1]?.control).toEqual({ kind: "choice", options: ["none", "burn", "sidecar"] });
    expect(added[2]?.label).toBe("Chapters");
  });
});

describe("下書きとレンダーターゲットの往復", () => {
  const specs = fieldSpecs();

  it("fps は分数の文字になり、戻すと同じ値になる", () => {
    const draft = draftOf(target, specs);
    expect(draft.fps).toBe("30000/1001");
    const parsed = targetOf(draft, specs, target);
    expect(parsed).toEqual({ ok: true, value: target });
  });

  it("空にした省ける欄は取り除き、スキーマに無い欄は残す", () => {
    const base = { ...target, loudness_lufs: -14, future_field: 7 } as RenderTarget;
    const draft = { ...draftOf(base, specs), loudness_lufs: "" };
    const parsed = targetOf(draft, specs, base);
    expect(parsed.ok && "loudness_lufs" in parsed.value).toBe(false);
    expect(parsed.ok && (parsed.value as Record<string, unknown>).future_field).toBe(7);
  });

  it("誤りは欄ごとに返す", () => {
    const draft = { ...draftOf(target, specs), width: "19x", fps: "0", id: " ", loudness_lufs: "loud" };
    const parsed = targetOf(draft, specs, target);
    expect(parsed.ok).toBe(false);
    expect(!parsed.ok && Object.keys(parsed.errors).sort()).toEqual(["fps", "id", "loudness_lufs", "width"]);
  });
});

describe("parseRational", () => {
  it("整数、分数、小数を有理数にする", () => {
    expect(parseRational("30")).toEqual({ num: 30, den: 1 });
    expect(parseRational("30000/1001")).toEqual({ num: 30000, den: 1001 });
    expect(parseRational("29.97")).toEqual({ num: 2997, den: 100 });
    expect(parseRational("x")).toBeNull();
  });
});

describe("withRequired と extraFields", () => {
  const specs = fieldSpecs();

  it("増えた必須の欄をスキーマの既定値か型の初期値で埋める", () => {
    const withNew = [...specs, { key: "audio_bitrate", path: ["audio_bitrate"], label: "x", required: true, control: { kind: "integer" as const, min: 64 } }];
    const filled = withRequired({ id: "a" }, withNew) as unknown as Record<string, unknown>;
    expect(filled.audio_bitrate).toBe(64);
    expect(filled.id).toBe("a");
  });

  it("一覧の行に出す「その他」は、専用の形で見せる欄を除いた値のある欄", () => {
    expect(extraFields(target, specs)).toEqual([]);
    expect(extraFields({ ...target, loudness_lufs: -14 }, specs)).toEqual([{ label: "Loudness (LUFS)", text: "-14" }]);
  });
});

describe("プリセット", () => {
  const specs = fieldSpecs();

  it("YouTube 1080p、Shorts、ProRes のマスターを持つ", () => {
    expect(PRESETS.map((p) => [p.id, p.width, p.height])).toEqual([
      ["youtube-1080p", 1920, 1080],
      ["youtube-shorts", 1080, 1920],
      ["prores-422-hq", 1920, 1080],
    ]);
    expect(PRESETS.find((p) => p.id === "prores-422-hq")?.codec).toBe("prores_ks");
  });

  it("名前が重なると連番を付け、出力先は名前と拡張子から決める", () => {
    expect(uniqueId("a", ["a", "a-2"])).toBe("a-3");
    const preset = PRESETS[1] as Preset;
    const made = targetFromPreset(preset, { composition: "main", takenIds: ["youtube-shorts"] }, specs);
    expect(made.id).toBe("youtube-shorts-2");
    expect(made.output_path).toBe("exports/youtube-shorts-2.mp4");
    expect(targetOf(draftOf(made, specs), specs, made).ok).toBe(true);
  });
});

describe("実際のスキーマの delivery", () => {
  const specs = fieldSpecs();

  it("入れ子の delivery は子の欄になり、列挙は選択、真偽はチェックになる", () => {
    const kind = (key: string) => specs.find((s) => s.key === key)?.control;
    expect(kind("delivery.subtitles")).toEqual({ kind: "choice", options: ["none", "sidecar", "embedded"] });
    expect(kind("delivery.chapters")).toEqual({ kind: "bool" });
    expect(kind("delivery.true_peak_dbtp")?.kind).toBe("number");
    expect(specs.some((s) => s.key === "delivery")).toBe(false);
    expect(specs.find((s) => s.key === "delivery.subtitles")?.required).toBe(false);
  });

  it("子の値を入れ子のまま往復し、開いて保存しただけでは delivery を足さない", () => {
    expect(targetOf(draftOf(target, specs), specs, target)).toEqual({ ok: true, value: target });
    const withDelivery = { ...target, delivery: { subtitles: "sidecar", chapters: true, future: 1 } } as unknown as RenderTarget;
    const draft = draftOf(withDelivery, specs);
    expect(draft["delivery.subtitles"]).toBe("sidecar");
    expect(draft["delivery.chapters"]).toBe(true);
    expect(targetOf(draft, specs, withDelivery)).toEqual({ ok: true, value: withDelivery });
    const turnedOff = targetOf({ ...draft, "delivery.subtitles": "", "delivery.chapters": false }, specs, withDelivery);
    expect(turnedOff.ok && (turnedOff.value as unknown as { delivery: unknown }).delivery).toEqual({ chapters: false, future: 1 });
  });

  it("字幕を選ぶと delivery.subtitles を持つレンダーターゲットになる", () => {
    const parsed = targetOf({ ...draftOf(target, specs), "delivery.subtitles": "sidecar" }, specs, target);
    expect(parsed.ok && parsed.value).toEqual({ ...target, delivery: { subtitles: "sidecar" } });
  });
});

describe("入力の検査", () => {
  const specs = fieldSpecs();
  const errorsOf = (over: Record<string, string>) => {
    const parsed = targetOf({ ...draftOf(target, specs), ...over }, specs, target);
    return parsed.ok ? {} : parsed.errors;
  };

  it("幅と高さは 1 以上の偶数", () => {
    expect(errorsOf({ width: "0" }).width).toContain("Out of range");
    expect(errorsOf({ height: "1081" }).height).toContain("even");
    expect(errorsOf({ width: "1280", height: "720" })).toEqual({});
  });

  it("分母は分母の範囲で見る（分子の範囲を流用しない）", () => {
    const rate = (denMax: number): FieldSpec[] => [{ key: "fps", path: ["fps"], label: "Frame rate", required: true, control: { kind: "rational", min: 1, max: 100000, denMin: 1, denMax } }];
    const ok = targetOf({ fps: "30000/1001" }, rate(2000));
    expect(ok.ok && ok.value).toEqual({ fps: { num: 30000, den: 1001 } });
    expect(targetOf({ fps: "30000/1001" }, rate(1000)).ok).toBe(false);
  });

  it("ProRes と DNxHR は .mov の出力先だけ", () => {
    expect(errorsOf({ codec: "prores_ks" }).output_path).toContain(".mov");
    expect(errorsOf({ codec: "dnxhr_hq", output_path: "out/a.mxf" }).output_path).toContain(".mov");
    expect(errorsOf({ codec: "prores_ks", output_path: "out/a.MOV" })).toEqual({});
    expect(errorsOf({ codec: "h264_nvenc" })).toEqual({});
  });
});

describe("プリセットの中身", () => {
  const specs = fieldSpecs();
  const main = { id: "main", duration: 60, width: 1920, height: 1080 };
  const short = { id: "short", duration: 30, width: 1080, height: 1920 };

  it("YouTube と Shorts は -14 LUFS と NVENC、ProRes はラウドネスなし", () => {
    const made = (id: string) => targetFromPreset(PRESETS.find((p) => p.id === id) as Preset, { composition: "main", takenIds: [] }, specs);
    expect(made("youtube-1080p")).toMatchObject({ codec: "h264_nvenc", loudness_lufs: -14 });
    expect(made("youtube-shorts")).toMatchObject({ codec: "h264_nvenc", loudness_lufs: -14 });
    expect(made("prores-422-hq").loudness_lufs).toBeUndefined();
  });

  it("縦長のプリセットは縦長のコンポジションを既定にする", () => {
    const shorts = PRESETS.find((p) => p.id === "youtube-shorts") as Preset;
    const youtube = PRESETS[0] as Preset;
    const both = { compositions: [main, short], open_composition: "main" };
    expect(defaultComposition(shorts, both)).toBe("short");
    expect(defaultComposition(youtube, both)).toBe("main");
    expect(defaultComposition(shorts, { ...both, open_composition: "short" })).toBe("short");
    expect(defaultComposition(youtube, { ...both, open_composition: "short" })).toBe("short");
    expect(defaultComposition(shorts, { compositions: [main], open_composition: "main" })).toBe("main");
    expect(defaultComposition(shorts, { compositions: [], open_composition: null })).toBe("");
  });
});
