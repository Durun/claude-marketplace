// 再生の操作列と時計の表示。ui/dist の shell の中央の列に入れる（アプリが initialization_script で読み込む）。
// Timeline の選択（Rust の Tauri イベント）は、shell の Inspector が受ける DOM のイベントにつなぐ。
// 再生位置は Rust の時計が持つ。ここでは受けた位置を、送られてから経った時間だけ進めて表示する。
(() => {
  const { invoke, Channel } = window.__TAURI__.core;
  const ADAPTER_WAIT_MS = 1500;
  const LAYER_SELECTED_EVENT = "mm:layer-selected";
  // キー操作を再生に渡さない入力部品。値を打つ・選ぶ操作が Space や矢印を使う。
  const TEXT_ENTRY = "input, select, textarea, [contenteditable]";
  const nowUs = () => Math.round((performance.timeOrigin + performance.now()) * 1000);

  const CSS = `
    #mm-transport { height: 100%; box-sizing: border-box; padding: var(--mm-space-md, 10px); overflow: auto;
      background: var(--mm-color-bg); color: var(--mm-color-text);
      font-family: var(--mm-font-family); font-size: var(--mm-font-size-base); }
    #mm-transport h2 { margin: 0 0 var(--mm-space-sm, 6px); font-size: var(--mm-font-size-lg); }
    #mm-transport .tc { font: 26px var(--mm-font-mono, monospace); }
    #mm-transport .dim { color: var(--mm-color-text-muted); font-size: var(--mm-font-size-base); }
    #mm-transport .bar { height: 14px; margin: var(--mm-space-sm, 6px) 0; cursor: pointer;
      background: var(--mm-color-surface); border: 1px solid var(--mm-color-border-strong);
      border-radius: var(--mm-radius-md, 4px); overflow: hidden; }
    #mm-transport .fill { height: 100%; width: 0; background: var(--mm-color-accent); }
    #mm-transport .buttons { display: flex; gap: var(--mm-space-xs, 4px); margin-bottom: var(--mm-space-md, 10px); }
    #mm-transport button { flex: 1; padding: 4px 6px; cursor: pointer; font: inherit;
      color: var(--mm-color-text); background: var(--mm-gradient-button, var(--mm-color-surface));
      border: 1px solid var(--mm-color-border-strong); border-radius: var(--mm-radius-md, 4px); }
    #mm-transport button.primary { background: var(--mm-color-accent); color: var(--mm-color-accent-text); }
    #mm-transport dl { display: grid; grid-template-columns: max-content 1fr; gap: 2px 10px; margin: 0 0 var(--mm-space-md, 10px); }
    #mm-transport dt { color: var(--mm-color-text-muted); }
    #mm-transport dd { margin: 0; word-break: break-all; }
    #mm-transport ul { margin: 0; padding: 0; list-style: none; font-family: var(--mm-font-mono, monospace); }
    #mm-transport li { padding: 1px 0; overflow-wrap: anywhere; }
  `;

  const el = (tag, attrs = {}, children = []) => {
    const n = document.createElement(tag);
    Object.assign(n, attrs);
    n.append(...children);
    return n;
  };

  const fmt = (secs) => {
    const s = Math.max(0, secs);
    const m = Math.floor(s / 60);
    return `${m}:${(s - m * 60).toFixed(3).padStart(6, "0")}`;
  };

  const ui = {};
  let info = null;
  let last = null;

  const shown = () => {
    if (!last) return 0;
    const age = last.playing ? Math.max(0, nowUs() - last.sentUs) / 1e6 : 0;
    const t = last.t + age;
    return info ? Math.min(t, info.duration) : t;
  };

  const draw = (t) => {
    if (!info) return;
    ui.tc.textContent = fmt(t);
    ui.fr.textContent = String(Math.floor(t * info.fps + 1e-6));
    ui.fill.style.width = `${Math.min(100, (t / info.duration) * 100)}%`;
    ui.play.textContent = last && last.playing ? "停止" : "再生";
    ui.lag.textContent = last ? `${last.lagMs.toFixed(1)} ms` : "-";
  };

  const act = (action, value) => invoke("transport", { action, value: value ?? null });

  const build = () => {
    const style = el("style", { textContent: CSS });
    ui.tc = el("span", { textContent: fmt(0) });
    ui.dur = el("span", { textContent: fmt(0) });
    ui.fr = el("span", { textContent: "0" });
    ui.frTotal = el("span", { textContent: "0" });
    ui.fill = el("div", { className: "fill" });
    ui.bar = el("div", { className: "bar" }, [ui.fill]);
    ui.play = el("button", { className: "primary", textContent: "再生" });
    ui.play.onclick = () => act("toggle");
    const btn = (label, fn) => el("button", { textContent: label, onclick: fn });
    const buttons = el("div", { className: "buttons" }, [
      btn("先頭", () => act("home")),
      btn("-1", () => act("step", -1)),
      ui.play,
      btn("+1", () => act("step", 1)),
      btn("末尾", () => act("end")),
    ]);
    ui.bar.onclick = (e) => {
      const r = ui.bar.getBoundingClientRect();
      act("seek", ((e.clientX - r.left) / r.width) * (info ? info.duration : 0));
    };
    ui.lag = el("dd", { textContent: "-" });
    ui.info = el("dl");
    ui.layers = el("ul");
    const root = el("section", { id: "mm-transport" }, [
      el("h2", { textContent: "プレビュー" }),
      el("div", { className: "tc" }, [ui.tc, el("span", { className: "dim" }, [" / ", ui.dur])]),
      el("div", { className: "dim" }, ["フレーム ", ui.fr, " / ", ui.frTotal]),
      ui.bar,
      buttons,
      ui.info,
      el("h2", { textContent: "レイヤー" }),
      ui.layers,
    ]);
    document.head.append(style);
    (document.querySelector(".mm-shell-center") ?? document.body).append(root);
  };

  const row = (k, v) => [el("dt", { textContent: k }), el("dd", { textContent: v })];

  const showInfo = () => {
    ui.dur.textContent = fmt(info.duration);
    ui.frTotal.textContent = String(Math.round(info.duration * info.fps));
    const backends = info.backends.map(([k, v]) => `${k}=${v}`).join(" ");
    ui.info.replaceChildren(
      ...row("プロジェクト", info.name),
      ...row("コンポジション", `${info.composition} ${info.width}x${info.height} ${info.fps} fps`),
      ...row("主の時計", info.clock === "audio" ? "音の出力" : "壁時計（音の出力なし）"),
      ...row("音の出力", info.audio_device ?? "なし"),
      ...row("GPU", info.adapter || "-"),
      ...row("実装", backends),
      el("dt", { textContent: "通知の遅れ" }),
      ui.lag,
    );
    ui.layers.replaceChildren(
      ...info.layers.map((l) =>
        el("li", { textContent: `${l.kind.padEnd(11)} ${l.id}  ${l.start.toFixed(1)}-${l.end.toFixed(1)}s` }),
      ),
    );
  };

  const onTick = (msg) => {
    const recv = nowUs();
    last = { t: msg.t, sentUs: msg.sent_us, playing: msg.playing, lagMs: msg.lag_ms };
    requestAnimationFrame(() => {
      const t = shown();
      draw(t);
      invoke("sync_ack", { seq: msg.seq, recvUs: recv, rafUs: nowUs(), shownT: t });
    });
  };

  const start = async () => {
    build();
    info = await invoke("project_info");
    showInfo();
    const channel = new Channel();
    channel.onmessage = onTick;
    await invoke("subscribe", { onTick: channel });
    window.addEventListener("keydown", (e) => {
      if (e.repeat || (e.target instanceof Element && e.target.closest(TEXT_ENTRY))) return;
      if (e.code === "Space") e.preventDefault();
      invoke("key_event", { source: "webview", key: e.code });
    });
    // 選択かプロジェクトのデータが変わったら、Inspector に中身を渡し、レイヤーの一覧を取り直す。
    window.__TAURI__.event.listen(LAYER_SELECTED_EVENT, async (e) => {
      window.dispatchEvent(new CustomEvent(LAYER_SELECTED_EVENT, { detail: e.payload }));
      info = await invoke("project_info");
      showInfo();
    });
    // 描画スレッドが GPU を選び終えた後に情報を取り直す。
    setTimeout(async () => {
      info = await invoke("project_info");
      showInfo();
    }, ADAPTER_WAIT_MS);
  };

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", start);
  else start();
})();
