import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { createClient, createIpcTransport, IPC_APPLY_OP } from "./client.ts";
import { generateOpsTs, opName, paramValueSchemas, tsType, type OpsSchema } from "./codegen.ts";
import { OP_NAMES, PARAM_VALUE_SCHEMAS } from "./generated/ops.ts";

const OPS_JSON = new URL("../../../crates/mm-ops/schema/ops.json", import.meta.url);
const GENERATED = new URL("./generated/ops.ts", import.meta.url);
const schema = JSON.parse(readFileSync(OPS_JSON, "utf8")) as OpsSchema;

describe("生成した型", () => {
  it("ops.json から作り直すと、置いてある generated/ops.ts と一致する", () => {
    expect(readFileSync(GENERATED, "utf8")).toBe(generateOpsTs(schema));
  });

  it("ops.json の操作の名前を全部持つ", () => {
    expect([...OP_NAMES]).toEqual((schema.oneOf ?? []).map(opName));
    expect(OP_NAMES).toContain("setParam");
  });

  it("ops.json が変わると型も変わる", () => {
    const extra = {
      type: "object",
      properties: { op: { type: "string", const: "newOp" }, n: { type: "integer" } },
      required: ["op", "n"],
    };
    const changed: OpsSchema = { ...schema, oneOf: [...(schema.oneOf ?? []), extra] };
    const ts = generateOpsTs(changed);
    expect(ts).toContain("export type NewOpOp");
    expect(ts).toContain('"newOp"');
    expect(ts).not.toBe(generateOpsTs(schema));
  });

  it("パラメータの値の種類ごとの value のスキーマを持つ", () => {
    expect(Object.keys(PARAM_VALUE_SCHEMAS).sort()).toEqual(Object.keys(paramValueSchemas(schema)).sort());
    expect(PARAM_VALUE_SCHEMAS.vec3).toMatchObject({ type: "array", minItems: 3, maxItems: 3 });
  });
});

describe("tsType", () => {
  it("null を許す型と省略できるフィールドを表す", () => {
    const t = tsType({
      type: "object",
      properties: { a: { type: ["string", "null"] }, b: { type: "integer" } },
      required: ["b"],
    });
    expect(t).toContain('"a"?: string | null;');
    expect(t).toContain('"b": number;');
  });
});

describe("IPC の裏", () => {
  it("apply_op に op を渡し、結果を value で返す", async () => {
    const call = vi.fn().mockResolvedValue({ rev: 3 });
    const client = createClient(createIpcTransport(call));
    const result = await client.send("lockLayer", { layer: "l1", lock: true });
    expect(call).toHaveBeenCalledWith(IPC_APPLY_OP, { op: { op: "lockLayer", layer: "l1", lock: true } });
    expect(result).toEqual({ ok: true, value: { rev: 3 } });
  });

  it("命令が拒否したら error にして返す", async () => {
    const call = vi.fn().mockRejectedValue("レイヤーが無い");
    const result = await createClient(createIpcTransport(call)).sendOp({ op: "removeLayer", layer: "x" });
    expect(result).toEqual({ ok: false, error: "レイヤーが無い" });
  });
});
