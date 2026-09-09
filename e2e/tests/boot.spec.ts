import { test, expect } from "@playwright/test";
import { ForgeApp } from "../helpers/forge";

test.beforeEach(async ({ page }) => {
  const app = new ForgeApp(page);
  await app.boot();
  await app.clearErrors();
});

test("boot: canvas renders, bridge alive, toolbar complete", async ({ page }) => {
  const app = new ForgeApp(page);
  const s = await app.state();

  // Fresh untitled document, one initial evaluation done.
  expect(s.doc).toBe("untitled");
  expect(s.features).toBe(0);
  expect(s.evalDone).toBeGreaterThanOrEqual(1);

  // Every core toolbar widget is present and laid out.
  const ids = (await app.widgets()).map((w) => w.id);
  for (const id of [
    "tool:undo",
    "tool:redo",
    "tool:sketch-xy",
    "tool:extrude",
    "tool:drill",
    "tool:projection",
    "tool:fit",
    "tool:grid",
    "tool:edges",
    "tool:section",
    "tool:measure",
    "tool:gizmo-move",
    "tool:gizmo-rotate",
    "tool:save",
    "tool:palette",
    "menu:Solid",
    "menu:Export",
    "viewport",
  ]) {
    expect(ids, `toolbar exposes ${id}`).toContain(id);
  }

  await app.snap("boot");
  await app.expectHealthy();
});

test("boot: heartbeat advances (no hang)", async ({ page }) => {
  const app = new ForgeApp(page);
  const f1 = await app.frame();
  await app.waitForFrames(10);
  const f2 = await app.frame();
  expect(f2 - f1).toBeGreaterThanOrEqual(10);
  await app.expectHealthy();
});
