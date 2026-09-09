import { test, expect } from "@playwright/test";
import { ForgeApp } from "../helpers/forge";

test.beforeEach(async ({ page }) => {
  const app = new ForgeApp(page);
  await app.boot();
  await app.clearErrors();
});

// Exports on wasm are browser downloads — Playwright intercepts them.
test("exports: every format downloads with a body present", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();

  for (const fmt of ["STL (binary)", "OBJ", "glTF 2.0"]) {
    const dl = page.waitForEvent("download", { timeout: 15_000 });
    await app.click("menu:Export");
    await app.waitForFrames(2);
    await app.clickLabel(fmt);
    const download = await dl;
    expect(await download.suggestedFilename()).toMatch(/untitled\.(stl|obj|gltf)$/);
    // Cancel to avoid temp files piling up.
    await download.cancel();
    await app.expectHealthy();
  }
});

test("save: .forgecad document downloads", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();

  const dl = page.waitForEvent("download", { timeout: 15_000 });
  await app.click("tool:save");
  const download = await dl;
  expect(await download.suggestedFilename()).toBe("untitled.forgecad");
  const s = await app.state();
  expect(s.modified).toBe(false); // save clears the dirty flag
  await download.cancel();
  await app.expectHealthy();
});

// The bridge action queue replaces drag-and-drop for E2E: mesh bytes
// feed the I-01 import pipeline directly.
test("import: ASCII STL via bridge action adds a feature", async ({ page }) => {
  const app = new ForgeApp(page);
  const stl = [
    "solid test",
    "  facet normal 0 0 1",
    "    outer loop",
    "      vertex 0 0 0",
    "      vertex 10 0 0",
    "      vertex 0 10 0",
    "    endloop",
    "  endfacet",
    "endsolid test",
    "",
  ].join("\n");
  await app.action("import", `stl:${stl}`);
  await app.waitForEval(15_000);
  const s = await app.state();
  expect(s.features).toBe(1);
  expect(s.bodies).toBe(1);
  await app.snap("imported-stl");
  await app.expectHealthy();
});

test("import: garbage bytes fail gracefully (no panic)", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.action("import", "stl:not an stl at all!!");
  await app.waitForFrames(4);
  const s = await app.state();
  expect(s.status).toContain("failed");
  expect(s.features).toBe(0);
  await app.expectHealthy();
});

test("gizmo: drag with a selection never kills the app", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();
  await app.clickPrefix("tree:");
  await app.waitForFrames(2);
  await app.click("tool:gizmo-move");
  await app.waitForFrames(2);
  await app.viewportDrag([0.5, 0.5], [0.62, 0.5], false);
  await app.waitForEval();
  await app.expectHealthy();
  await app.snap("gizmo-after-drag");
});
