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
        statements: 50,
        branches: 70,
        functions: 60,
        lines: 50,
        "src/connection.ts": {
          statements: 90,
          branches: 80,
          functions: 100,
          lines: 90,
        },
      },
    },
  },
});
