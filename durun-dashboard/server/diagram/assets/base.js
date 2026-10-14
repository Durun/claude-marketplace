// 全文書で走るスクリプト。画面右端の目次と、用語・参照番号のホバーを持つ。
// どちらも対象が無ければ何もしないので、文書ごとに読み分けなくてよい。
//
// ---- 画面右端の目次 ----
// 本文の h2・h3 を正本に、画面右端の目次を組み立てる。
// 目次を手書きすると見出しの増減ごとに二重管理になるため、読み込み時に本文から作る。
// 展開と収納は CSS の :hover / :focus-within が持ち、ここでは組み立てと現在位置の追従だけを行う。
// 対になる見た目は base.css にある。
(() => {
  // 用語集は文書末の付録なので目次には出さない
  const headings = [...document.querySelectorAll('h2, h3')].filter((h) => !h.closest('#glossary'));
  // 数行の目次は本文を追う助けにならないので出さない
  if (headings.length < 3) return;

  const outline = document.createElement('nav');
  outline.className = 'outline';
  outline.setAttribute('aria-label', '目次');

  const strip = document.createElement('div');
  strip.className = 'outline-strip';
  strip.setAttribute('aria-hidden', 'true');

  const panel = document.createElement('div');
  panel.className = 'outline-panel';

  const marks = headings.map((heading, index) => {
    // 既にある id は本文からのリンク先になっている可能性があるため残す
    heading.id ||= `h-${index + 1}`;
    const level = heading.tagName === 'H3' ? '3' : '2';

    const mark = document.createElement('span');
    mark.dataset.level = level;
    strip.append(mark);

    const link = document.createElement('a');
    link.href = `#${heading.id}`;
    link.dataset.level = level;
    link.textContent = heading.textContent.trim();
    panel.append(link);

    return { heading, mark, link };
  });

  outline.append(strip, panel);
  document.body.append(outline);

  // 画面上端から少し下がった位置を読んでいる場所とみなす。見出しが上端に来た瞬間に切り替わると早すぎる
  const READ_LINE_PX = 120;
  const update = () => {
    let current = marks[0];
    for (const mark of marks) {
      if (mark.heading.getBoundingClientRect().top > READ_LINE_PX) break;
      current = mark;
    }
    for (const mark of marks) {
      mark.mark.classList.toggle('current', mark === current);
      mark.link.classList.toggle('current', mark === current);
    }
  };

  update();
  document.addEventListener('scroll', update, { passive: true });
})();

// ---- 用語と参照番号のホバー ----
// 本文中の用語と参照番号をホバー可能な .term へ置き換える。
// 本文へ手作業でマークアップを入れると項目の増減ごとに全文を直すことになるため、実行時に一度だけ自動で付ける。
// 正本は 2 つある。用語は文書末の用語集テーブル、参照番号は U1・D1 のような id を持つ要素そのものである。
// 対になる見た目は base.css にある。
(() => {
  const table = document.querySelector('#glossary table');

  // 「共有リンク（リンク）」「payload / props」のような別名表記を個別の見出し語へ分解する
  const terms = [...(table?.tBodies[0].rows ?? [])].flatMap((row) => {
    const desc = row.cells[1].textContent.trim();
    return row.cells[0].textContent
      .split(/[／/、（(）)]/)
      .map((t) => t.trim())
      .filter(Boolean)
      .map((term) => ({ term, desc }));
  })
    // 長い語を先に当てて「共有リンク」が「リンク」に食われないようにする
    .sort((a, b) => b.term.length - a.term.length);

  // 参照番号（U1=未決事項、D1=意思決定 など）の正本は、その番号を id に持つ項目そのものである。
  // 別表を作ると本体と説明が二重管理になるため、説明は id を持つ要素の文章から取る。
  const REF_ID = /^[A-Z]{1,2}\d+$/;
  const REF_TOKEN = /[A-Z]{1,2}\d+/g;
  // 意思決定は決まったことなので変更点の青、未決事項は判断待ちなので警告の赤。他の接頭辞は無彩色のバッジ
  const REF_TONE = { D: 'new', U: 'ng' };
  // 読み上げと aria-label 用のテキスト。判断材料を削ると定義箇所まで往復させることになるため要約しない
  const summarize = (el) => {
    const text = el.textContent.replace(/\s+/g, ' ').trim();
    // 番号そのもの（「U1」「U1.」）は説明にならないので落とす。表の識別子セルやカードの見出しが該当する
    return text.replace(new RegExp(`^${el.id}\\s*[.:）)]?\\s*`), '');
  };
  const refs = new Map(
    [...document.querySelectorAll('[id]')]
      .filter((el) => REF_ID.test(el.id))
      .map((el) => [el.id, summarize(el)])
  );

  // 識別子や既にリンク・見出しになっている箇所は二重装飾になるので触らない
  const SKIP = new Set(['CODE', 'PRE', 'A', 'SCRIPT', 'STYLE', 'SUMMARY', 'TEXTAREA', 'H1', 'H2', 'H3', 'svg']);
  const isAscii = (s) => /^[\w.-]+$/.test(s);
  const seen = new Set();

  const wrap = (node, hit) => {
    const rest = node.splitText(hit.index);
    const after = rest.splitText(hit.label.length);
    // 参照番号は定義箇所へ飛べるようにリンクにし、定義箇所の識別子バッジと同じ見た目にする
    const el = document.createElement(hit.href ? 'a' : 'span');
    el.className = hit.href ? `term ref badge uid ${REF_TONE[hit.label[0]] ?? ''}` : 'term';
    if (hit.href) el.href = hit.href;
    else {
      el.tabIndex = 0;
      el.setAttribute('role', 'note');
    }
    el.dataset.desc = hit.desc;
    // 参照番号は吹き出しに定義箇所そのものを縮小して出すため、表示のたびに引ける識別子を持たせる
    if (hit.href) el.dataset.ref = hit.label;
    el.setAttribute('aria-label', `${hit.label}: ${hit.desc}`);
    rest.replaceWith(el);
    el.append(rest);
    return after;
  };

  // 用語は節ごとの初出だけに付ける。参照番号は読み手が出会った場所で必ず引けるよう、出現するたびに付ける
  const findFirst = (text, owner) => {
    let best = null;
    const keep = (hit) => {
      if (!best || hit.index < best.index) best = hit;
    };
    for (const t of terms) {
      if (seen.has(t.term)) continue;
      const i = text.indexOf(t.term);
      if (i < 0) continue;
      // 英数字の語は単語境界を見る。props が properties に当たるのを避ける
      if (isAscii(t.term)) {
        const around = /[\w.-]/;
        if (around.test(text[i - 1] ?? '') || around.test(text[i + t.term.length] ?? '')) continue;
      }
      keep({ label: t.term, desc: t.desc, index: i, term: t.term });
    }
    for (const m of text.matchAll(REF_TOKEN)) {
      // 定義箇所の見出し語は自分自身へのリンクになるだけなので飛ばす
      if (m[0] === owner || !refs.has(m[0])) continue;
      const around = /[\w.-]/;
      if (around.test(text[m.index - 1] ?? '') || around.test(text[m.index + m[0].length] ?? '')) continue;
      keep({ label: m[0], desc: refs.get(m[0]), index: m.index, href: `#${m[0]}` });
    }
    return best;
  };

  const visit = (node, owner) => {
    for (let child = node.firstChild; child; ) {
      const next = child.nextSibling;
      if (child.nodeType === Node.TEXT_NODE) {
        let cursor = child;
        for (let hit = findFirst(cursor.data, owner); hit; hit = findFirst(cursor.data, owner)) {
          if (hit.term) seen.add(hit.term);
          cursor = wrap(cursor, hit);
        }
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        // h2 ごとに初出を数え直す。長い文書で節をまたいで語を追わずに済む
        if (child.nodeName === 'H2') seen.clear();
        if (!SKIP.has(child.nodeName) && child.id !== 'glossary') {
          visit(child, REF_ID.test(child.id) ? child.id : owner);
        }
      }
      child = next;
    }
  };

  visit(document.body, null);

  // 用語も参照番号も無い文書では吹き出しを作らない
  if (!document.querySelector('.term')) return;

  // 吹き出しの表示。位置は表示のたびに計算し、画面の左右上下に収める
  const tip = document.createElement('div');
  tip.id = 'tip';
  tip.setAttribute('role', 'tooltip');
  tip.hidden = true;
  document.body.append(tip);

  const MARGIN = 8;

  // 参照番号の吹き出しは、定義箇所の見た目をそのまま縮小して見せる。
  // 全文をテキストで出すとカードのバッジや表の列見出しが落ち、読み手が意味を組み直すことになるため。
  const RICH_ZOOM = 0.8;

  // 表の行は列見出しと組にしないと、どのセルが論点でどれが確認先なのか読めない
  const rowWithHead = (row) => {
    const source = row.closest('table');
    const table = document.createElement('table');
    table.className = source?.className ?? '';
    if (source?.tHead) table.append(source.tHead.cloneNode(true));
    const body = document.createElement('tbody');
    body.append(row.cloneNode(true));
    table.append(body);
    return table;
  };

  const richClone = (source) => {
    const clone = source.tagName === 'TR' ? rowWithHead(source) : source.cloneNode(true);
    // 文書に同じ id が2つあると本文からのリンクの飛び先が定まらない
    clone.removeAttribute('id');
    for (const el of clone.querySelectorAll('[id]')) el.removeAttribute('id');
    // 吹き出しはポインタを通さないが、フォーカスは通ってしまう
    for (const el of clone.querySelectorAll('[tabindex]')) el.removeAttribute('tabindex');
    // 定義箇所が持つ外側の余白は吹き出しの中では意味を持たない
    clone.style.margin = '0';

    const box = document.createElement('div');
    box.className = 'tip-clone';
    // 元の幅を与えてから縮小する。吹き出しの幅に任せると折り返しが変わって縮小版にならない
    box.style.width = `${source.getBoundingClientRect().width}px`;
    box.style.zoom = RICH_ZOOM;
    box.append(clone);
    return box;
  };

  const show = (el, event) => {
    const source = el.dataset.ref ? document.getElementById(el.dataset.ref) : null;
    tip.classList.toggle('rich', Boolean(source));
    if (source) tip.replaceChildren(richClone(source));
    else tip.textContent = el.dataset.desc;
    tip.hidden = false;
    // 幅を測る前に左上へ寄せる。狭い場所に置いたまま測ると折り返して高さが変わる
    tip.style.left = `${MARGIN}px`;
    tip.style.top = '0px';
    // 折り返した語は複数の矩形を持つ。ポインタが乗っている行の矩形を基準にする
    const rects = [...el.getClientRects()];
    const base = rects.find((r) => event?.clientY >= r.top && event.clientY <= r.bottom) ?? rects[0];
    const box = tip.getBoundingClientRect();
    const left = Math.min(Math.max(base.left, MARGIN), window.innerWidth - box.width - MARGIN);
    const above = base.top - box.height - 6;
    tip.style.left = `${left}px`;
    tip.style.top = `${above < MARGIN ? base.bottom + 6 : above}px`;
  };
  const hide = () => { tip.hidden = true; };

  document.addEventListener('pointerover', (e) => {
    const el = e.target.closest?.('.term');
    if (el) show(el, e); else hide();
  });
  document.addEventListener('focusin', (e) => {
    if (e.target.classList?.contains('term')) show(e.target); else hide();
  });
  // スクロールすると基準の行がずれるため、追従させずに閉じる
  document.addEventListener('scroll', hide, { passive: true });
})();
