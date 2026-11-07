import js from "@eslint/js";
import tseslint from "typescript-eslint";

const MAX_FILE_LINES = 500;
const MAX_FUNCTION_LINES = 50;

export default tseslint.config(
  { ignores: ["**/generated/**", "**/dist/**", "**/node_modules/**", "tools/screenshot.mjs", "panels/*/screenshots.mjs"] },
  js.configs.recommended,
  ...tseslint.configs.strict,
  {
    linterOptions: { reportUnusedDisableDirectives: "error" },
    rules: {
      "no-var": "error",
      "no-console": ["error", { allow: ["debug", "warn", "error"] }],
      "prefer-arrow-callback": "error",
      "func-style": ["error", "expression"],
      "max-lines": ["error", MAX_FILE_LINES],
      "max-lines-per-function": ["error", MAX_FUNCTION_LINES],
      "max-depth": ["error", 3],
    },
  },
  {
    // 実アプリに CDP でつなぐ撮影のスクリプト。page.evaluate の中身はブラウザーで動く。
    files: ["tools/*.mjs"],
    languageOptions: { globals: { process: "readonly", window: "readonly", document: "readonly" } },
  },
  {
    files: ["**/src/**/*.{ts,tsx}"],
    ignores: ["**/*.test.{ts,tsx}"],
    rules: {
      "no-magic-numbers": ["error", { ignore: [0, 1, -1], ignoreArrayIndexes: true, enforceConst: true }],
    },
  },
);
