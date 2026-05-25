import type { Page } from '@playwright/test';

/**
 * 向聊天输入框写入草稿并触发 input 事件（Leptos `draft` 信号依赖 on:input，单纯 fill 无效）。
 */
export async function fillComposerDraft(page: Page, text: string): Promise<void> {
  const input = page.getByTestId('chat-composer-input');
  await input.click();
  await input.fill(text);
  await input.dispatchEvent('input', { bubbles: true });
}
