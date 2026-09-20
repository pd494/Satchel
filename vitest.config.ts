import { cloudflareTest } from "@cloudflare/vitest-plugin";
import { defineConfig } from "vitest/config";

/** Test configuration for finite, credential-free Effect tests. */
export default defineConfig({
  test: {
    projects: [
      {
        plugins: [
          cloudflareTest({
            wrangler: { configPath: "./wrangler.jsonc" },
            miniflare: {
              compatibilityDate: "2026-08-27",
              bindings: {
                WEBHOOK_SECRET: "test-webhook-secret",
                ACCOUNT_ID_SECRET: "test-account-id-secret",
              },
            },
          }),
        ],
        test: {
          name: "worker",
          include: [
            "test/accounts.test.ts",
            "test/connection.test.ts",
            "test/messages.test.ts",
            "test/webhook.test.ts",
          ],
        },
      },
    ],
    coverage: {
      provider: "istanbul",
      include: ["src/**/*.ts"],
      reporter: ["text", "json", "json-summary", "html"],
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
        "src/worker/webhook.ts": {
          statements: 90,
          branches: 90,
          functions: 75,
          lines: 90,
        },
        "src/worker/app.ts": {
          statements: 100,
          branches: 100,
          functions: 100,
          lines: 100,
        },
      },
    },
  },
});
