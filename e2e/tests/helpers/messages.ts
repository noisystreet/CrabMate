/** 分页 API 响应形状（与后端 GET /conversation/messages 对齐）。 */
export type MessagesPage = {
  conversation_id: string;
  revision: number;
  messages: { role: string; content: string }[];
  total_count: number;
  window_start_index: number;
  has_older: boolean;
};

export function userMessages(n: number): { role: string; content: string }[] {
  return Array.from({ length: n }, (_, i) => ({
    role: 'user',
    content: `e2e-msg-${i}`,
  }));
}
