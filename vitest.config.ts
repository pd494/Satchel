import { defineConfig } from "vitest/config";

/** Test configuration for finite, credential-free Effect tests. */
export default defineConfig({
  test: {
    include: ["test/**/*.test.ts"],
    environment: "node",
  },
});
