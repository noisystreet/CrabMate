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
});
