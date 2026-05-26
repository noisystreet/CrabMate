import { fillComposerDraft } from './composer';

/** Leptos 受控输入：填草稿并派发 input 事件。 */
export async function typeComposerDraft(
  page: import('@playwright/test').Page,
  text: string,
): Promise<void> {
  await fillComposerDraft(page, text);
}
