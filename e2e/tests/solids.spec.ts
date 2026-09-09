import { test, expect } from "@playwright/test";
import { ForgeApp } from "../helpers/forge";

test.beforeEach(async ({ page }) => {
  const app = new ForgeApp(page);
  await app.boot();
  await app.clearErrors();
});

test("solid menu: every primitive creates a body", async ({ page }) => {
  // Software-rendered WebGL is slow: five primitives + five undo
  // evaluations under SwiftShader need headroom.
  test.setTimeout(180_000);
  const app = new ForgeApp(page);

  for (const kind of ["Box", "Sphere", "Cylinder", "Cone", "Torus"]) {
    await app.addSolid(kind);
    const s = await app.state();
    // The wasm eval worker runs synchronously in-frame; bodies appear
    // after the next evaluation settles.
    expect(s.features, `${kind} added`).toBeGreaterThan(0);
    await app.expectHealthy();
  }

  await app.waitForEval();
  const s = await app.state();
  expect(s.features).toBe(5);
  expect(s.bodies).toBe(5);
  await app.snap("solids-all-five");

  // Undo the whole stack back to empty, then redo one.
  for (let i = 0; i < 5; i++) {
    await app.click("tool:undo");
    await app.waitForEval();
  }
  expect((await app.state()).features).toBe(0);
  await app.click("tool:redo");
  await app.waitForEval();
  expect((await app.state()).features).toBe(1);
  await app.expectHealthy();
});

test("tree: select opens inspector, apply edits, delete removes", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();
  expect((await app.state()).bodies).toBe(1);

  // Select via the real tree row.
  await app.clickPrefix("tree:");
  await app.waitForFrames(2);
  let s = await app.state();
  expect(s.selected).toBe(1);

  // Inspector: Apply button exists and clicking it is panic-free.
  await app.click("btn:Apply");
  await app.waitForEval();
  s = await app.state();
  expect(s.features).toBe(1);

  // Delete via the right-click context menu path: select row, open the
  // row menu through the ⋮ menu button's popup, click Delete.
  await app.clickPrefix("tree:");
  await app.waitForFrames(2);
  // Right-click the tree row for the context menu.
  const row = await app.widgetPrefix("tree:");
  const cx = (row.rect[0] + row.rect[2]) / 2;
  const cy = (row.rect[1] + row.rect[3]) / 2;
  await page.mouse.click(cx, cy, { button: "right" });
  await app.waitForFrames(2);
  await app.click("rowmenu:delete");
  await app.waitForEval();
  s = await app.state();
  expect(s.features).toBe(0);
  await app.expectHealthy();
});

test("tree: suppress eye toggles body visibility", async ({ page }) => {
  const app = new ForgeApp(page);
  await app.addSolid("Box");
  await app.waitForEval();
  expect((await app.state()).bodies).toBe(1);

  await app.clickPrefix("tree:");
  await app.waitForFrames(2);
  const eye = (await app.widgets()).find((w) => w.id.endsWith(":suppress"));
  expect(eye).toBeDefined();
  const cx = (eye!.rect[0] + eye!.rect[2]) / 2;
  const cy = (eye!.rect[1] + eye!.rect[3]) / 2;
  await page.mouse.click(cx, cy);
  await app.waitForEval();
  expect((await app.state()).bodies).toBe(0);

  // Back on.
  await app.clickPrefix("tree:");
  await app.waitForFrames(2);
  const eye2 = (await app.widgets()).find((w) => w.id.endsWith(":suppress"));
  await page.mouse.click(
    (eye2!.rect[0] + eye2!.rect[2]) / 2,
    (eye2!.rect[1] + eye2!.rect[3]) / 2,
  );
  await app.waitForEval();
  expect((await app.state()).bodies).toBe(1);
  await app.expectHealthy();
});
