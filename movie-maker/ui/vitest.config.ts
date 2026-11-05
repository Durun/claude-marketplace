import { defineConfig } from "vitest/config";

// Windows と Node 24 の組み合わせで子プロセスの受け渡しが落ちるので、スレッドで回す
export default defineConfig({ test: { pool: "threads", include: ["**/src/**/*.test.{ts,tsx}"] } });
