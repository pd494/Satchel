import { readdir, readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const skills = fileURLToPath(new URL("../.agents/skills/", import.meta.url));

describe("project skills", () => {
  it("has one uniquely named SKILL.md per skill directory", async () => {
    const directories = (await readdir(skills, { withFileTypes: true }))
      .filter((entry) => entry.isDirectory())
      .map((entry) => entry.name)
      .sort();

    const names = await Promise.all(
      directories.map(async (directory) => {
        const source = await readFile(
          `${skills}/${directory}/SKILL.md`,
          "utf8",
        );

        const name = source.match(/^name:\s*["']?([^"'\n]+)["']?$/m)?.[1];

        expect(source).toMatch(/^---\n[\s\S]+?\n---\n\s*\S/);
        expect(source).toMatch(/^description:\s*.+$/m);
        expect(name).toBe(directory);

        return name;
      }),
    );

    expect(new Set(names).size).toBe(names.length);
  });

  it("loads anti-slop from its canonical skill directory", async () => {
    const config = await readFile(
      fileURLToPath(new URL("../oxlint.config.ts", import.meta.url)),
      "utf8",
    );

    expect(config).toContain("./.agents/skills/anti-slop/index.ts");
    expect(config).toContain("./.agents/skills/anti-slop/effect/index.ts");
    expect(config).not.toContain("tools/oxlint/anti-slop");
  });
});
