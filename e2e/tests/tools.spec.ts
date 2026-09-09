import { test, expect } from "@playwright/test";
import { ForgeApp } from "../helpers/forge";

test.beforeEach(async ({ page }) => {
  const app = new ForgeApp(page);
  await app.boot();
  await app.clearErrors();
});

test("measure: two surface picks produce a label", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();

  await app.click("tool:measure");
  await app.waitForFrames(2);
  expect((await app.state()).measureMode).toBe(true);

  // Two picks on the body (viewport center hits the box).
  await app.clickViewport(0.45, 0.5);
  await app.waitForFrames(2);
  expect((await app.state()).measurePicks).toBe(1);
  await app.clickViewport(0.55, 0.5);
  await app.waitForFrames(2);
  const s = await app.state();
  expect(s.measurePicks).toBe(2);
  expect(s.measureLabel).toBeTruthy();
  await app.snap("measure-label");

  // Third click restarts.
  await app.clickViewport(0.5, 0.45);
  await app.waitForFrames(2);
  expect((await app.state()).measurePicks).toBe(1);
  await app.expectHealthy();
});

test("measure: empty-space click is safe", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.click("tool:measure");
  await app.waitForFrames(2);
  await app.clickViewport(0.5, 0.5);
  await app.clickViewport(0.95, 0.95);
  expect((await app.state()).measurePicks).toBe(0);
  await app.expectHealthy();
});

test("section: toggle + flip", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();

  await app.click("tool:section");
  await app.waitForFrames(2);
  await app.snap("section-on");
  await app.expectHealthy();
  await app.click("tool:section");
  await app.waitForFrames(2);
  await app.expectHealthy();
});

test("keyboard shortcuts toggle view state", async ({ page }) => {
  const app = new ForgeApp(page);
  let s = await app.state();

  const grid = s.grid;
  await page.keyboard.press("g");
  await app.waitForFrames(2);
  expect((await app.state()).grid).not.toBe(grid);

  const edges = s.edges;
  await page.keyboard.press("e");
  await app.waitForFrames(2);
  expect((await app.state()).edges).not.toBe(edges);

  await page.keyboard.press("t");
  await app.waitForFrames(2);
  expect((await app.state()).gizmoMode).toBe("Translate");
  await page.keyboard.press("r");
  await app.waitForFrames(2);
  expect((await app.state()).gizmoMode).toBe("Rotate");

  // Regression guard: the T/R handlers used to deadlock the egui
  // context (write-lock inside read-lock) and kill the app.
  await app.expectHealthy();
});

test("camera: orbit drag + wheel zoom + nav cube", async ({ page }) => {
  const app = new ForgeApp(page);
  const s0 = await app.state();

  // Orbit (Alt + drag).
  await app.viewportDrag([0.5, 0.5], [0.3, 0.35]);
  const s1 = await app.state();
  expect(Math.abs(s1.camera.yaw - s0.camera.yaw)).toBeGreaterThan(1e-4);

  // Zoom.
  const vp = await app.widget("viewport");
  await page.mouse.move(
    (vp.rect[0] + vp.rect[2]) / 2,
    (vp.rect[1] + vp.rect[3]) / 2,
  );
  await page.mouse.wheel(0, 120);
  await app.waitForFrames(3);
  const s2 = await app.state();
  expect(Math.abs(s2.camera.distance - s1.camera.distance)).toBeGreaterThan(1e-6);
  await app.expectHealthy();
});
