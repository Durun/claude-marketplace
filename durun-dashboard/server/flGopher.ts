// FL Connect の ② の道。FL 2026 の Gopher の画面に Chrome DevTools Protocol（CDP）でつなぎ、FL が画面に差し込む script_handler でツールを呼ぶ。
// FL は、環境変数 WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<ポート> を付けて起動し、人間が Gopher を開いたときにポートが開く。
// 呼び方は Image-Line の mcp_bridge.js と同じ。tools/call の JSON を script_handler.runJson に入れ、返事は flHelper.onRunJson に返る。
// 返事に要求の番号が無いので、一度に 1 つずつ呼ぶ。
// Gopher の画面の URL のクエリには利用者名とログインの合言葉が入るので、URL は記録にも画面にも出さない。

const GOPHER_ORIGIN = 'https://gopher-fls.image-line.com/';
/** Gopher も、FL の返事を 35 秒でとめる。 */
export const CALL_TIMEOUT_MS = 35_000;
const CONNECT_TIMEOUT_MS = 3000;

/**
 * Gopher の画面の中に置く橋。FL の返事は Gopher の flHelper のコールバックに返るので、FL Connect が呼んで待っている間だけ受け取り、Gopher へは渡さない。
 * 渡すと、Gopher が呼んでいる最中なら、FL Connect が読んだプロジェクトの中身が Gopher のサーバーへ送られる。
 * Gopher の呼び出し（fl_tool_call）と結果（fl_tool_result）の数を数え、Gopher が呼んでいる間は FL Connect から呼ばない。
 * 画面を読み直すと橋が消えるので、呼ぶたびに印を見て掛け直す。
 */
const BRIDGE = `(() => {
  const flc = (window.__flConnect ??= { waiting: false, resolve: undefined, waitingTools: false, resolveTools: undefined, gopherCalls: 0, listening: false });
  flc.handler = typeof script_handler === 'object' && script_handler ? script_handler : window.chrome?.webview?.hostObjects?.script_handler ?? null;
  const helper = (window.flHelper ??= {});
  if (!helper.onRunJson?.__flConnect) {
    const prev = helper.onRunJson;
    const wrapped = function (payload) {
      if (flc.waiting) {
        flc.waiting = false;
        const resolve = flc.resolve;
        flc.resolve = undefined;
        resolve?.(String(payload));
        return;
      }
      if (prev) return prev.apply(this, arguments);
    };
    wrapped.__flConnect = true;
    helper.onRunJson = wrapped;
  }
  if (!helper.onMCPTools?.__flConnect) {
    const prev = helper.onMCPTools;
    const wrapped = function (tools) {
      if (flc.waitingTools) {
        flc.waitingTools = false;
        const resolve = flc.resolveTools;
        flc.resolveTools = undefined;
        resolve?.(String(tools));
        return;
      }
      if (prev) return prev.apply(this, arguments);
    };
    wrapped.__flConnect = true;
    helper.onMCPTools = wrapped;
  }
  if (!flc.listening) {
    window.addEventListener('message', (event) => {
      const kind = event.data?.kind;
      if (kind === 'fl_tool_call') flc.gopherCalls += 1;
      else if (kind === 'fl_tool_result' && flc.gopherCalls > 0) flc.gopherCalls -= 1;
    });
    flc.listening = true;
  }
  return flc.handler ? 'ready' : 'no-handler';
})()`;

/** ページの中で、FL に 1 つ頼んで返事を待つ式。返すのは { payload } か、呼べなかった理由。 */
const callExpression = (envelope: string): string => `new Promise((resolve) => {
  const flc = window.__flConnect;
  if (flc.gopherCalls > 0) return resolve({ refused: 'gopher' });
  if (flc.waiting || flc.waitingTools) return resolve({ refused: 'own' });
  flc.waiting = true;
  const timer = setTimeout(() => { flc.waiting = false; flc.resolve = undefined; resolve({ timeout: true }); }, ${CALL_TIMEOUT_MS});
  flc.resolve = (payload) => { clearTimeout(timer); resolve({ payload }); };
  try {
    flc.handler.runJson = ${JSON.stringify(envelope)};
  } catch (e) {
    clearTimeout(timer);
    flc.waiting = false;
    flc.resolve = undefined;
    resolve({ error: String(e) });
  }
})`;

const TOOLS_EXPRESSION = `new Promise((resolve) => {
  const flc = window.__flConnect;
  if (flc.waiting || flc.waitingTools) return resolve({ refused: 'own' });
  flc.waitingTools = true;
  const timer = setTimeout(() => { flc.waitingTools = false; flc.resolveTools = undefined; resolve({ timeout: true }); }, ${CALL_TIMEOUT_MS});
  flc.resolveTools = (payload) => { clearTimeout(timer); resolve({ payload }); };
  try {
    flc.handler.MCPTools = '1';
  } catch (e) {
    clearTimeout(timer);
    flc.waitingTools = false;
    flc.resolveTools = undefined;
    resolve({ error: String(e) });
  }
})`;

type PageReply = { payload?: string; refused?: 'gopher' | 'own'; timeout?: boolean; error?: string };

export type GopherTool = { name: string; description: string; inputSchema?: { properties?: Record<string, { type?: string; description?: string }>; required?: string[] } };

/**
 * FL の返事から、ツールの結果の文を取り出す。返事は JSON-RPC の形をしているが、FL は中の文の " をエスケープしないので、JSON として読めないことがある。
 * 読めなければ、"text": " から "isError" の手前までを文として切り出す。それも無ければ、返事をそのまま文にする。
 */
export const gopherText = (payload: string): { text: string; isError: boolean } => {
  try {
    const parsed = JSON.parse(payload) as { result?: { content?: { text?: string }[]; isError?: boolean }; error?: { message?: string } };
    if (parsed && typeof parsed === 'object') {
      if (parsed.error) return { text: parsed.error.message ?? JSON.stringify(parsed.error), isError: true };
      if (parsed.result?.content) return { text: parsed.result.content.map((c) => c.text ?? '').join('\n'), isError: parsed.result.isError === true };
    }
    return { text: typeof parsed === 'string' ? parsed : payload, isError: false };
  } catch {
    const m = /"text"\s*:\s*"([\s\S]*?)"\s*}\s*]\s*,\s*"isError"\s*:\s*(true|false)/.exec(payload);
    if (m) return { text: (m[1] ?? '').replace(/\\n/g, '\n'), isError: m[2] === 'true' };
    return { text: payload, isError: false };
  }
};

export type GopherState = 'no-port' | 'no-page' | 'no-handler' | 'connected';

export class GopherError extends Error {
  readonly state: GopherState | 'busy' | 'timeout' | 'failed';

  constructor(message: string, state: GopherError['state']) {
    super(message);
    this.state = state;
  }
}

type Cdp = { evaluate: (expression: string) => Promise<unknown>; close: () => void; closed: () => boolean };

/** CDP の WebSocket でページにつなぐ。Node は fetch と WebSocket を持つので、新しいモジュールは要らない。 */
const connectPage = async (url: string): Promise<Cdp> => {
  const ws = new WebSocket(url);
  await new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new Error('Gopher の画面につなげません')), CONNECT_TIMEOUT_MS);
    ws.onopen = () => {
      clearTimeout(timer);
      resolve();
    };
    ws.onerror = () => {
      clearTimeout(timer);
      reject(new Error('Gopher の画面につなげません'));
    };
  });
  let seq = 0;
  let open = true;
  type Reply = { id?: number; result?: { result?: { value?: unknown }; exceptionDetails?: unknown }; error?: { message?: string } };
  const replies = new Map<number, (message: Reply) => void>();
  ws.onmessage = (event) => {
    const message = JSON.parse(String(event.data)) as Reply;
    if (message.id !== undefined) replies.get(message.id)?.(message);
  };
  ws.onclose = () => {
    open = false;
    for (const reply of replies.values()) reply({ error: { message: 'Gopher の画面との接続が切れました' } });
    replies.clear();
  };
  const evaluate = (expression: string): Promise<unknown> =>
    new Promise((resolve, reject) => {
      if (!open) return reject(new Error('Gopher の画面との接続が切れました'));
      seq += 1;
      const id = seq;
      replies.set(id, (message) => {
        replies.delete(id);
        const failure = message.error?.message ?? (message.result?.exceptionDetails ? 'Gopher の画面の中で例外が起きました' : undefined);
        if (failure) reject(new Error(failure));
        else resolve(message.result?.result?.value);
      });
      ws.send(JSON.stringify({ id, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }));
    });
  return { evaluate, close: () => ws.close(), closed: () => !open };
};

/** デバッグのポートに、Gopher の画面があるか。ページの一覧には Gopher の画面の URL が出るが、つなぐ先を選ぶのにだけ使い、外へ出さない。 */
const findPage = async (port: number): Promise<string> => {
  let pages: { type?: string; url?: string; webSocketDebuggerUrl?: string }[];
  try {
    pages = (await (await fetch(`http://127.0.0.1:${port}/json`, { signal: AbortSignal.timeout(CONNECT_TIMEOUT_MS) })).json()) as typeof pages;
  } catch {
    throw new GopherError(`デバッグのポート ${port} が開いていません。FL を FL Connect のウィンドウから起動し、Gopher を開いてください`, 'no-port');
  }
  const page = pages.find((p) => p.type === 'page' && p.url?.startsWith(GOPHER_ORIGIN) && p.webSocketDebuggerUrl);
  if (!page?.webSocketDebuggerUrl) throw new GopherError('Gopher の画面がありません。FL で Gopher を開いてください', 'no-page');
  return page.webSocketDebuggerUrl;
};

/** Gopher の画面への接続。つながらなくなったら、次に呼ぶときにつなぎ直す。 */
export class GopherLink {
  #port: number;
  #cdp: Cdp | undefined;
  #queue: Promise<unknown> = Promise.resolve();
  tools: GopherTool[] | undefined;

  constructor(port: number) {
    this.#port = port;
  }

  async #page(): Promise<Cdp> {
    if (!this.#cdp || this.#cdp.closed()) {
      this.#cdp = undefined;
      this.#cdp = await connectPage(await findPage(this.#port));
    }
    const ready = await this.#cdp.evaluate(BRIDGE).catch((e: Error) => {
      this.close();
      throw new GopherError(e.message, 'failed');
    });
    if (ready !== 'ready') throw new GopherError('Gopher の画面に script_handler がありません。FL の版が変わったかもしれません', 'no-handler');
    return this.#cdp;
  }

  /** 一度に 1 つずつ動かす。FL の返事には要求の番号が無いので、重ねると返事を取り違える。 */
  #serial<T>(task: () => Promise<T>): Promise<T> {
    const run = this.#queue.then(task, task);
    this.#queue = run.catch(() => undefined);
    return run;
  }

  /** つながるかを確かめ、ツールの一覧を読む。 */
  check(): Promise<GopherState> {
    return this.#serial(async (): Promise<GopherState> => {
      await this.#page();
      if (!this.tools) this.tools = await this.#readTools();
      return 'connected';
    });
  }

  async #readTools(): Promise<GopherTool[]> {
    const reply = (await (await this.#page()).evaluate(TOOLS_EXPRESSION)) as PageReply;
    if (reply.payload === undefined) throw new GopherError(reply.timeout ? 'FL がツールの一覧を返しません' : `ツールの一覧を読めません（${reply.error ?? reply.refused}）`, 'failed');
    const parsed = JSON.parse(reply.payload) as GopherTool[] | { tools?: GopherTool[] };
    return Array.isArray(parsed) ? parsed : (parsed.tools ?? []);
  }

  /** ツールを呼び、FL の返事の文を返す。Gopher が呼んでいる間は呼ばずに断る。 */
  call(name: string, args: Record<string, unknown>): Promise<{ text: string; isError: boolean; ms: number }> {
    return this.#serial(async () => {
      const page = await this.#page();
      const envelope = JSON.stringify({ jsonrpc: '2.0', id: Date.now(), method: 'tools/call', params: { name, arguments: args } });
      const started = Date.now();
      const reply = (await page.evaluate(callExpression(envelope))) as PageReply;
      if (reply.refused === 'gopher') throw new GopherError('人間が Gopher に頼んだ操作を FL が動かしています。返事を取り違えないよう、終わってから呼んでください', 'busy');
      if (reply.refused === 'own') throw new GopherError('前の呼び出しの返事を待っています', 'busy');
      if (reply.timeout) throw new GopherError(`${CALL_TIMEOUT_MS / 1000} 秒待っても FL が返事をしません`, 'timeout');
      if (reply.payload === undefined) throw new GopherError(`FL に頼めません（${reply.error ?? '理由が分かりません'}）`, 'failed');
      return { ...gopherText(reply.payload), ms: Date.now() - started };
    });
  }

  close(): void {
    this.#cdp?.close();
    this.#cdp = undefined;
  }
}
