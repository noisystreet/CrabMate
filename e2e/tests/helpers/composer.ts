import { expect, type Page } from '@playwright/test';

/**
 * 向聊天输入框写入草稿并触发 input 事件（Leptos `draft` 信号依赖 on:input，单纯 fill 无效）。
 */
export async function fillComposerDraft(page: Page, text: string): Promise<void> {
  const input = page.getByTestId('chat-composer-input');
  await input.click();
  await input.fill(text);
  await input.dispatchEvent('input', { bubbles: true });
}

/** 澄清问卷字段：与 `composer-clarification-input` 相同，须触发 input。 */
export async function fillClarificationAnswer(page: Page, index: number, text: string): Promise<void> {
  const input = page.getByTestId('composer-clarification-input').nth(index);
  await input.click();
  await input.fill(text);
  await input.dispatchEvent('input', { bubbles: true });
}
