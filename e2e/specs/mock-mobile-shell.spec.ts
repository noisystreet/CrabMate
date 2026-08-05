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

  test("bottom tab bar switches workspace tab and hides chat column", async ({
    page,
  }) => {
    const sid = `s_e2e_mobile_tabs_${Date.now()}`;
    await seedSession(page, sid);

    const tabBar = page.locator('[data-testid="mobile-bottom-tab-bar"]');
    await expect(tabBar).toBeVisible();

    const shellMain = page.locator("#layout-mode-panel-main");
    await expect(shellMain).toHaveAttribute("data-mobile-tab", "chat");

    await page.locator('[data-testid="mobile-tab-workspace"]').click();
    await expect(shellMain).toHaveAttribute("data-mobile-tab", "workspace");

    await expect(page.locator(".chat-column")).toBeHidden();

    await page.locator('[data-testid="mobile-tab-chat"]').click();
    await expect(shellMain).toHaveAttribute("data-mobile-tab", "chat");
    await expect(page.locator(".chat-column")).toBeVisible();
  });

  test("status overflow menu exposes model and mode on narrow viewport", async ({
    page,
  }) => {
    const sid = `s_e2e_mobile_status_overflow_${Date.now()}`;
    await seedSession(page, sid);

    const trigger = page.locator(
      '[data-testid="status-chip-overflow-trigger"]',
    );
    await expect(trigger).toBeVisible({ timeout: 15_000 });
    await trigger.click();

    const menu = page.locator(".status-chip-overflow-menu");
    await expect(menu).toBeVisible();
    await expect(
      menu.locator(".status-chip-model .status-chip-value"),
    ).not.toBeEmpty();
    await expect(
      menu.locator(".status-chip-mode .status-chip-value"),
    ).not.toBeEmpty();
  });
});
