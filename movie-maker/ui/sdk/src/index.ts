/** 操作の呼び出しの結果。成功なら value、失敗なら error を持つ。 */
export type InvokeResult<T = unknown> = { ok: true; value: T } | { ok: false; error: string };

/** 操作の呼び出し。command は mm-ops の操作の名前。 */
export type Invoke = (command: string, args?: Record<string, unknown>) => Promise<InvokeResult>;

/**
 * 操作を呼ぶ関数の形。
 * mm-ops の JSON Schema から型を生成するまでの仮の中身で、呼ばれた名前を記録して失敗を返す。
 */
export const invoke: Invoke = async (command, args) => {
  console.debug("[mm-sdk] invoke", command, args ?? {});
  return { ok: false, error: `sdk is not connected to mm-ops yet: ${command}` };
};
