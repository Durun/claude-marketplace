import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { generateOpsTs, type OpsSchema } from "../sdk/src/codegen.ts";

const OPS_JSON = fileURLToPath(new URL("../../crates/mm-ops/schema/ops.json", import.meta.url));
const OPS_TS = fileURLToPath(new URL("../sdk/src/generated/ops.ts", import.meta.url));

// ops.json から generated/ops.ts を書き直す。`--check` では書かず、コミット済みの内容と違えば失敗する（CI 用）。
const schema = JSON.parse(await readFile(OPS_JSON, "utf8")) as OpsSchema;
const generated = generateOpsTs(schema);
if (process.argv.includes("--check")) {
  const committed = await readFile(OPS_TS, "utf8").catch(() => "");
  if (committed !== generated) {
    console.error("sdk/src/generated/ops.ts が ops.json と合わない。pnpm gen で書き直してコミットする");
    process.exit(1);
  }
} else {
  await writeFile(OPS_TS, generated);
}
