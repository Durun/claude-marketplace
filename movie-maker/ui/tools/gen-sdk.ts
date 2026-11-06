import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { generateOpsTs, type OpsSchema } from "../sdk/src/codegen.ts";

const OPS_JSON = fileURLToPath(new URL("../../crates/mm-ops/schema/ops.json", import.meta.url));
const OPS_TS = fileURLToPath(new URL("../sdk/src/generated/ops.ts", import.meta.url));

// ops.json から generated/ops.ts を書き直す。build の前に走る。
const schema = JSON.parse(await readFile(OPS_JSON, "utf8")) as OpsSchema;
await writeFile(OPS_TS, generateOpsTs(schema));
