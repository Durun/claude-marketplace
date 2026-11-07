import type { AnyPanel } from "@mm/sdk";
import * as registered from "./panels.ts";

/** 登録されたパネル。列の中の並びは `order`、同じなら id の順。 */
export const PANELS: readonly AnyPanel[] = Object.values(registered).sort(
  (a, b) => a.order - b.order || a.id.localeCompare(b.id),
);

/** `createApi` を持つパネルの API を作る。キーはパネルの id。 */
export const createPanelApis = (panels: readonly AnyPanel[] = PANELS): Record<string, unknown> =>
  Object.fromEntries(panels.flatMap((p) => (p.createApi ? [[p.id, p.createApi()]] : [])));
