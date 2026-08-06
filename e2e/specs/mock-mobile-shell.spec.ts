/**
 * 移动端壳层回归：窄屏视口下导航抽屉、主列全宽与 data-narrow-viewport 标记。
 *
 * 运行：cd e2e && no_proxy=127.0.0.1,localhost npx playwright test specs/mock-mobile-shell.spec.ts
 */
import { expect, test } from "@playwright/test";
import { seedSession } from "../fixtures/helpers";

const MOBILE_VIEWPORT = { width: 390, height: 844 };

test.describe("移动端壳层", () => {
  test.use({ viewport: MOBILE_VIEWPORT });

  test("narrow viewport sets data-narrow-viewport and hamburger opens nav drawer", async ({
    page,
  }) => {
    const sid = `s_e2e_mobile_shell_${Date.now()}`;
    await seedSession(page, sid);

    await expect
      .poll(async () =>
        page.evaluate(() =>
          document.documentElement.hasAttribute("data-narrow-viewport"),
        ),
      )
      .toBe(true);

    const navRail = page.locator(".nav-rail");
    await expect(navRail).not.toHaveClass(/nav-rail-mobile-open/);

    const hamburger = page.locator(".shell-topbar-nav .btn-icon").first();
    await expect(hamburger).toBeVisible();
    await hamburger.click();

    await expect(navRail).toHaveClass(/nav-rail-mobile-open/);

    await page.locator(".nav-rail-backdrop").click();
    await expect(navRail).not.toHaveClass(/nav-rail-mobile-open/);
  });

  test("chat column uses full width when side panel hidden on mobile", async ({
    page,
  }) => {
    const sid = `s_e2e_mobile_chat_width_${Date.now()}`;
    await seedSession(page, sid);

    const chatWidth = await page.evaluate(() => {
      const chat = document.querySelector<HTMLElement>(".chat-column");
      const main = document.querySelector<HTMLElement>(".main-row");
      if (!chat || !main) return 0;
      return (
        chat.getBoundingClientRect().width / main.getBoundingClientRect().width
      );
    });

    expect(chatWidth).toBeGreaterThan(0.92);
  });

  test("nav toggle search opens filter panel from drawer", async ({ page }) => {
    const sid = `s_e2e_mobile_search_btn_${Date.now()}`;
    await seedSession(page, sid);

    const hamburger = page.locator(".shell-topbar-nav .btn-icon").first();
    await hamburger.click();
    await expect(page.locator(".nav-rail")).toHaveClass(/nav-rail-mobile-open/);

    const filter = page.locator("#nav-session-filter");
    await expect(filter).toBeHidden();

    await page.getByTestId("nav-toggle-search").click();
    await expect(filter).toBeVisible();
    await expect(page.getByTestId("nav-toggle-search")).toHaveAttribute(
      "aria-expanded",
      "true",
    );

    await page.getByTestId("nav-toggle-search").click();
    await expect(filter).toBeHidden();
  });

  test("side panel opens as right drawer with backdrop", async ({ page }) => {
    const sid = `s_e2e_mobile_side_drawer_${Date.now()}`;
    await seedSession(page, sid);

    await expect(page.getByTestId("side-column-backdrop")).toBeHidden();
    await expect(page.locator(".side-column")).toHaveClass(
      /side-column-rail-only/,
    );

    // 窄屏主屏无浮动工具栏；自右缘左划打开右侧抽屉
    const vp = page.viewportSize()!;
    const y = Math.floor(vp.height / 2);
    const startX = vp.width - 8;
    await page.mouse.move(startX, y);
    await page.mouse.down();
    await page.mouse.move(startX - 90, y, { steps: 8 });
    await page.mouse.up();

    const side = page.locator(".side-column");
    await expect(side).not.toHaveClass(/side-column-rail-only/);
    await expect(page.getByTestId("side-column-backdrop")).toBeVisible();
    await expect(page.getByTestId("side-panel")).toBeVisible();
    await expect(page.getByTestId("side-shell-toolbar")).toBeVisible();
    await expect(page.getByTestId("settings-open")).toBeVisible();
    await expect(page.getByTestId("side-view-trigger")).toBeVisible();

    const box = await side.boundingBox();
    expect(box).toBeTruthy();
    if (box) {
      expect(box.width).toBeLessThan(MOBILE_VIEWPORT.width * 0.95);
      expect(box.x + box.width).toBeGreaterThan(MOBILE_VIEWPORT.width - 8);
    }

    await page.getByTestId("side-column-backdrop").click();
    await expect(page.locator(".side-column")).toHaveClass(
      /side-column-rail-only/,
    );
    await expect(page.getByTestId("side-column-backdrop")).toBeHidden();
  });

  test("main chat hides floating shell toolbar on mobile", async ({ page }) => {
    const sid = `s_e2e_mobile_no_float_tb_${Date.now()}`;
    await seedSession(page, sid);

    await expect(page.locator(".shell-main-toolbar--rail-float")).toHaveCount(
      0,
    );
    await expect(page.getByTestId("side-shell-toolbar")).toHaveCount(0);
    await expect(page.getByTestId("settings-open")).toHaveCount(0);
  });

  test("session context menu delete opens in-app shell confirm", async ({
    page,
  }) => {
    const sid = `s_e2e_mobile_del_confirm_${Date.now()}`;
    await seedSession(page, sid);

    const hamburger = page.locator(".shell-topbar-nav .btn-icon").first();
    await hamburger.click();
    await expect(page.locator(".nav-rail")).toHaveClass(/nav-rail-mobile-open/);

    const row = page.getByTestId(`nav-session-${sid}`);
    await expect(row).toBeVisible();
    await row.dispatchEvent("contextmenu", {
      bubbles: true,
      cancelable: true,
      clientX: 80,
      clientY: 200,
    });

    await expect(page.getByTestId("session-ctx-delete")).toBeVisible();
    await page.getByTestId("session-ctx-delete").click();

    const confirm = page.getByTestId("shell-confirm-dialog");
    await expect(confirm).toBeVisible();
    await page.getByTestId("shell-confirm-cancel").click();
    await expect(confirm).toBeHidden();
    await expect(row).toBeVisible();
  });
});
