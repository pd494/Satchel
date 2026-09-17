import { defineConfig } from "vitest/config";

/** Test configuration for finite, credential-free Effect tests. */
export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
    coverage: {
      provider: "v8",
      include: ["src/**/*.ts"],
      reporter: ["text", "json-summary", "html"],
      reportsDirectory: "coverage",
      thresholds: {
        statements: 85,
        branches: 90,
        functions: 75,
        lines: 85,
        "src/connection.ts": {
          statements: 90,
          branches: 90,
          functions: 100,
          lines: 90,
        },
        "src/webhook.ts": {
          statements: 90,
          branches: 90,
          functions: 75,
          lines: 90,
        },
        "src/worker.ts": {
          statements: 100,
          branches: 100,
          functions: 100,
          lines: 100,
        },
      },
    },
  },
});
