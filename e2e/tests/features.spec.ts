import { test, expect } from "@playwright/test";
import { ForgeApp } from "../helpers/forge";

test.beforeEach(async ({ page }) => {
  const app = new ForgeApp(page);
  await app.boot();
  await app.clearErrors();
});

test("sketch → solve → extrude creates a solid", async ({ page }) => {
  const app = new ForgeApp(page);

  await app.click("tool:sketch-xy");
  await app.waitForEval();
  let s = await app.state();
  expect(s.features).toBe(1);
  expect(s.bodies).toBe(0); // a sketch alone renders no body

  // Select the sketch, solve constraints.
  await app.clickPrefix("tree:");
  await app.waitForFrames(2);
  await app.click("btn:Solve constraints");
  await app.waitForEval();

  // Extrude.
  await app.click("tool:extrude");
  await app.waitForEval(20_000);
  s = await app.state();
  expect(s.features).toBe(2);
  expect(s.bodies).toBe(1);
  await app.snap("sketch-extruded");
  await app.expectHealthy();
});

test("command palette: keyboard flow runs an action", async ({ page }) => {
  const app = new ForgeApp(page);

  await page.keyboard.press("Control+Shift+P");
  await app.waitForFrames(3);
  let s = await app.state();
  expect(s.paletteOpen).toBe(true);

  await page.keyboard.type("box");
  await app.waitForFrames(3);
  await page.keyboard.press("Enter");
  await app.waitForFrames(4);
  s = await app.state();
  expect(s.paletteOpen).toBe(false);
  await app.waitForEval();
  expect(s.features).toBe(1);
  await app.expectHealthy();
});

test("command palette: button + row click flow", async ({ page }) => {
  const app = new ForgeApp(page);

  await app.click("tool:palette");
  await app.waitForFrames(2);
  expect((await app.state()).paletteOpen).toBe(true);

  await app.clickPrefix("palette:Add Sphere");
  await app.waitForEval();
  expect((await app.state()).features).toBe(1);

  // Escape closes.
  await app.click("tool:palette");
  await app.waitForFrames(2);
  await page.keyboard.press("Escape");
  await app.waitForFrames(2);
  expect((await app.state()).paletteOpen).toBe(false);
  await app.expectHealthy();
});

test("palette: union two bodies", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.addSolid("Sphere");
  await app.waitForEval(20_000);
  expect((await app.state()).bodies).toBe(2);

  await page.keyboard.press("Control+Shift+P");
  await app.waitForFrames(3);
  await page.keyboard.type("union");
  await app.waitForFrames(3);
  await page.keyboard.press("Enter");
  await app.waitForEval(30_000);
  expect((await app.state()).bodies).toBe(1);
  await app.snap("union-result");
  await app.expectHealthy();
});

test("params: add parameter button works (left panel reachability)", async ({
  page,
}) => {
  const app = new ForgeApp(page);
  // Regression: the tree ScrollArea used to push the params panel
  // off-screen (invisible on default windows).
  const w = await app.widgetPrefix("btn:");
  expect(w).toBeDefined();
  await app.clickLabel("Add parameter");
  await app.waitForFrames(3);
  const s = await app.state();
  await app.expectHealthy();
  await app.snap("params");
});
