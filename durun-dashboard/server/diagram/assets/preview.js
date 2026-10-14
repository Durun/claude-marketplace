// プレビュー領域（実演パネル）を組み立てる。
// <figure class="preview"> の中の <template> を Shadow DOM へ複製し、実演の CSS と DOM を文書本体から隔離する。
// iframe を使わないのは、単一ファイルで完結させるためと、実演の中身を素の HTML として書けるようにするためである。
// カスタムプロパティは Shadow 境界を越えるため、実演の中でも base.css の --fg などがそのまま使える。
//
// マークアップ:
//   <figure class="preview" data-label="ラベル" data-height="14rem" data-reset>
//     <template> …実演の中身。<style> と <script> を含めてよい… </template>
//     <figcaption>…</figcaption>
//   </figure>
// data-reset を付けると「初期状態に戻す」ボタンが出る。動く実演には必ず付ける。
// data-height は面の最低の高さを決める。
// 中身を position: absolute だけで組んだ実演は自分の高さを持たないため、指定しないと既定の高さまで縮んで隠れる。
//
// テンプレート内の <script> は複製しても実行されないため、本文を関数として評価し直す。
// 関数の引数 root には ShadowRoot を渡す。実演のスクリプトは root 配下だけを触ること。
// リセットは中身を作り直すだけなので、setInterval のように残り続ける効果は自前で止める必要がある。
document.addEventListener('DOMContentLoaded', () => {
  for (const panel of document.querySelectorAll('.preview')) {
    const template = panel.querySelector('template');
    if (!template) continue;

    const bar = document.createElement('div');
    bar.className = 'preview-bar';
    const label = document.createElement('span');
    label.className = 'grow';
    label.textContent = panel.dataset.label ?? 'プレビュー';
    bar.append(label);

    const stage = document.createElement('div');
    stage.className = 'preview-stage';
    if (panel.dataset.height) stage.style.minHeight = panel.dataset.height;
    const root = stage.attachShadow({ mode: 'open' });

    const mount = () => {
      const fragment = template.content.cloneNode(true);
      // 複製した <script> は文書へ挿入した時点でグローバルに実行されてしまう。
      // root が見えず、リセットのたびに同じ変数を再宣言することになるため、挿入前に取り出す。
      const sources = [...fragment.querySelectorAll('script')].map((script) => {
        script.remove();
        return script.textContent;
      });
      root.replaceChildren(fragment);
      for (const source of sources) new Function('root', source)(root);
    };

    if ('reset' in panel.dataset) {
      const button = document.createElement('button');
      button.type = 'button';
      button.textContent = '初期状態に戻す';
      button.addEventListener('click', mount);
      bar.append(button);
    }

    panel.prepend(bar, stage);
    mount();
  }
});
