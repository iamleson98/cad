import { test, expect } from "@playwright/test";
import { ForgeApp } from "../helpers/forge";

test.beforeEach(async ({ page }) => {
  const app = new ForgeApp(page);
  await app.boot();
  await app.clearErrors();
});

// The "fresh install, click everything" suite — the class of bugs the
// desktop app hit (app quits on button clicks).
test("empty scene: every toolbar button survives", async ({ page }) => {
  const app = new ForgeApp(page);

  for (const id of [
    "tool:undo", // empty stack — disabled click must not panic
    "tool:redo",
    "tool:extrude", // no sketch
    "tool:drill", // no body
    "tool:projection", // nothing to project
    "tool:fit", // empty bounds
    "tool:grid",
    "tool:edges",
    "tool:measure",
    "tool:gizmo-move",
    "tool:gizmo-rotate",
  ]) {
    const w = await app.widget(id);
    const cx = (w.rect[0] + w.rect[2]) / 2;
    const cy = (w.rect[1] + w.rect[3]) / 2;
    await page.mouse.click(cx, cy); // click even when disabled
    await app.waitForFrames(2);
    await app.expectHealthy();
  }

  // Viewport clicks on empty space.
  await app.clickViewport(0.5, 0.5);
  await app.clickViewport(0.95, 0.95);

  const s = await app.state();
  expect(s.features).toBe(0);
  await app.snap("empty-after-everything");
});

test("empty scene: every export format degrades to a status", async ({ page }) => {
  const app = new ForgeApp(page);

  for (const fmt of ["STL (binary)", "OBJ", "glTF 2.0", "3MF (3D print package)"]) {
    await app.click("menu:Export");
    await app.waitForFrames(2);
    await app.clickLabel(fmt);
    const s = await app.state();
    expect(
      /Nothing to export|No bodies/.test(s.status),
      `status after ${fmt} on empty scene: ${s.status}`,
    ).toBe(true);
    await app.expectHealthy();
  }
});
