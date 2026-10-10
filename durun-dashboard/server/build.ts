// 画面のバンドルの作り方。pnpm build はこのファイルを動かし、pnpm start は同じ設定で作り直し続ける。
import fs from 'node:fs';
import path from 'node:path';
import * as esbuild from 'esbuild';

const ROOT = path.join(import.meta.dirname, '..');
const CHUNKS_DIR = path.join(ROOT, 'public', 'chunks');

/** チャンクは名前に中身のハッシュが入り、作り直すたびに増えるので、使わなくなったものを消す。 */
const pruneChunks: esbuild.Plugin = {
  name: 'prune-chunks',
  setup: (build) => {
    build.onEnd((result) => {
      if (!result.metafile || !fs.existsSync(CHUNKS_DIR)) return;
      const built = new Set(Object.keys(result.metafile.outputs).map((file) => path.basename(file)));
      for (const file of fs.readdirSync(CHUNKS_DIR)) if (!built.has(file)) fs.rmSync(path.join(CHUNKS_DIR, file), { force: true });
    });
  },
};

/**
 * 最初に読み込む app.js と、要るときに読み込むチャンクに分ける。イルカの 3D の描画は大きいので、イルカを出すときだけ読み込む。
 * 改善要望に載る例外のスタックを読めるよう、空白と構文だけを詰め、名前は縮めない。
 */
export const buildOptions: esbuild.BuildOptions = {
  absWorkingDir: ROOT,
  entryPoints: { app: 'client/main.tsx' },
  bundle: true,
  format: 'esm',
  target: 'es2022',
  splitting: true,
  outdir: 'public',
  chunkNames: 'chunks/[name]-[hash]',
  minifyWhitespace: true,
  minifySyntax: true,
  metafile: true,
  plugins: [pruneChunks],
};

if (process.argv[1] && path.resolve(process.argv[1]) === import.meta.filename) await esbuild.build({ ...buildOptions, logLevel: 'info' });
