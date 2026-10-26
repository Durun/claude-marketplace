// 全文書で走るスクリプト。画面右端の目次と、用語のホバーを持つ。
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

// ---- 用語のホバー ----
// 本文中の用語をホバー可能な .term へ置き換える。
// 本文へ手作業でマークアップを入れると項目の増減ごとに全文を直すことになるため、実行時に一度だけ自動で付ける。
// 正本は文書末の用語集テーブルである。
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

  // 識別子や既にリンク・見出しになっている箇所は二重装飾になるので触らない
  const SKIP = new Set(['CODE', 'PRE', 'A', 'SCRIPT', 'STYLE', 'SUMMARY', 'TEXTAREA', 'H1', 'H2', 'H3', 'svg']);
  const isAscii = (s) => /^[\w.-]+$/.test(s);
  const seen = new Set();

  const wrap = (node, hit) => {
    const rest = node.splitText(hit.index);
    const after = rest.splitText(hit.term.length);
    const el = document.createElement('span');
    el.className = 'term';
    el.tabIndex = 0;
    el.setAttribute('role', 'note');
    el.dataset.desc = hit.desc;
    el.setAttribute('aria-label', `${hit.term}: ${hit.desc}`);
    rest.replaceWith(el);
    el.append(rest);
    return after;
  };

  // 用語は節ごとの初出だけに付ける
  const findFirst = (text) => {
    let best = null;
    for (const t of terms) {
      if (seen.has(t.term)) continue;
      const i = text.indexOf(t.term);
      if (i < 0) continue;
      // 英数字の語は単語境界を見る。props が properties に当たるのを避ける
      if (isAscii(t.term)) {
        const around = /[\w.-]/;
        if (around.test(text[i - 1] ?? '') || around.test(text[i + t.term.length] ?? '')) continue;
      }
      if (!best || i < best.index) best = { ...t, index: i };
    }
    return best;
  };

  const visit = (node) => {
    for (let child = node.firstChild; child; ) {
      const next = child.nextSibling;
      if (child.nodeType === Node.TEXT_NODE) {
        let cursor = child;
        for (let hit = findFirst(cursor.data); hit; hit = findFirst(cursor.data)) {
          seen.add(hit.term);
          cursor = wrap(cursor, hit);
        }
      } else if (child.nodeType === Node.ELEMENT_NODE) {
        // h2 ごとに初出を数え直す。長い文書で節をまたいで語を追わずに済む
        if (child.nodeName === 'H2') seen.clear();
        if (!SKIP.has(child.nodeName) && child.id !== 'glossary') visit(child);
      }
      child = next;
    }
  };

  visit(document.body);

  // 用語が無い文書では吹き出しを作らない
  if (!document.querySelector('.term')) return;

  // 吹き出しの表示。位置は表示のたびに計算し、画面の左右上下に収める
  const tip = document.createElement('div');
  tip.id = 'tip';
  tip.setAttribute('role', 'tooltip');
  tip.hidden = true;
  document.body.append(tip);

  const MARGIN = 8;

  const show = (el, event) => {
    tip.textContent = el.dataset.desc;
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
