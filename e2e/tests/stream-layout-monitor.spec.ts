import { expect, test } from '@playwright/test';

import {
  analyzeTimelineSamples,
  excerptTimelineSamples,
  type TimelineBlockSample,
  type TimelineMessageSlot,
} from './helpers/stream-layout-monitor';

function snap(
  keys: string[],
  messages: TimelineMessageSlot[],
  t = 0,
): TimelineBlockSample {
  return { t, keys, messages };
}

function msg(id: string, loading = false, isTool = false, isUser = id.startsWith('u')): TimelineMessageSlot {
  return { id, loading, isTool, isUser };
}

test.describe('stream layout monitor (offline analyzer)', () => {
  test('detects backward reorder of stable batch row', () => {
    const report = analyzeTimelineSamples([
      snap(
        ['u1', 'tg:a+b', 'turn-batch-narration'],
        [msg('u1'), msg('a', false, true), msg('b', false, true), msg('turn-batch-narration')],
        0,
      ),
      snap(
        ['u1', 'turn-batch-narration', 'tg:a+b'],
        [msg('u1'), msg('turn-batch-narration'), msg('a', false, true), msg('b', false, true)],
        1,
      ),
    ]);
    expect(report.violations.some((v) => v.kind === 'reorder' && v.key === 'turn-batch-narration')).toBe(
      true,
    );
  });

  test('ignores loading-only assistant row reorder and vanish', () => {
    const report = analyzeTimelineSamples([
      snap(['u1', 's_load_a', 'turn-final-answer'], [msg('u1'), msg('s_load_a', true), msg('turn-final-answer')], 0),
      snap(['u1', 'turn-final-answer'], [msg('u1'), msg('turn-final-answer')], 1),
    ]);
    expect(report.violations).toEqual([]);
    expect(report.committed_message_ids).not.toContain('s_load_a');
  });

  test('detects vanished committed user message', () => {
    const report = analyzeTimelineSamples([
      snap(['u1'], [msg('u1')], 0),
      snap(['turn-batch-narration'], [msg('turn-batch-narration')], 1),
    ]);
    expect(report.violations.some((v) => v.kind === 'vanished' && v.key === 'u1')).toBe(true);
  });

  test('detects vanish then reappear of turn-final-answer', () => {
    const report = analyzeTimelineSamples([
      snap(['u1', 'turn-final-answer'], [msg('u1'), msg('turn-final-answer')], 0),
      snap(['u1'], [msg('u1')], 1),
      snap(['u1', 'turn-final-answer'], [msg('u1'), msg('turn-final-answer')], 2),
    ]);
    expect(report.violations.some((v) => v.kind === 'vanished' && v.key === 'turn-final-answer')).toBe(
      true,
    );
    expect(
      report.violations.some((v) => v.kind === 'vanish_reappear' && v.key === 'turn-final-answer'),
    ).toBe(true);
  });

  test('detects vanished tool message after commit', () => {
    const report = analyzeTimelineSamples([
      snap(['u1', 'tg:h_tool_0'], [msg('u1'), msg('h_tool_0', false, true)], 0),
      snap(['u1'], [msg('u1')], 1),
    ]);
    expect(report.violations.some((v) => v.kind === 'vanished' && v.key === 'h_tool_0')).toBe(true);
  });

  test('detects stable_order_inversion when final before batch', () => {
    const report = analyzeTimelineSamples([
      snap(
        ['u1', 'turn-final-answer', 'turn-batch-narration', 'tg:t0'],
        [msg('u1'), msg('turn-final-answer'), msg('turn-batch-narration'), msg('h_t0')],
        0,
      ),
    ]);
    expect(
      report.violations.some((v) => v.kind === 'stable_order_inversion' && v.key === 'turn-final-answer'),
    ).toBe(true);
  });

  test('detects committed_reorder of tool card', () => {
    const report = analyzeTimelineSamples([
      snap(
        ['u1', 'turn-batch-narration', 'tg:h_t0'],
        [msg('u1'), msg('turn-batch-narration'), msg('h_t0', false, true)],
        0,
      ),
      snap(
        ['u1', 'tg:h_t0', 'turn-batch-narration'],
        [msg('u1'), msg('h_t0', false, true), msg('turn-batch-narration')],
        1,
      ),
    ]);
    expect(report.violations.some((v) => v.kind === 'committed_reorder' && v.key === 'h_t0')).toBe(true);
  });

  test('allows tool loading alongside single assistant tail loading', () => {
    const report = analyzeTimelineSamples([
      snap(
        ['u1', 'tg:t0', 's_tail'],
        [msg('u1'), msg('h_t0', true, true), msg('s_tail', true)],
        0,
      ),
    ]);
    expect(report.violations.some((v) => v.kind === 'multiple_loading')).toBe(false);
  });

  test('detects multiple concurrent assistant loading tails', () => {
    const report = analyzeTimelineSamples([
      snap(
        ['u1', 'tail_a', 'tail_b'],
        [msg('u1'), msg('tail_a', true), msg('tail_b', true)],
        0,
      ),
    ]);
    expect(report.violations.some((v) => v.kind === 'multiple_loading')).toBe(true);
  });

  test('detects loading_after_stream in final snapshot', () => {
    const report = analyzeTimelineSamples([
      snap(['u1', 'turn-final-answer'], [msg('u1'), msg('turn-final-answer')], 0),
      snap(['u1', 'turn-final-answer', 's_tail'], [msg('u1'), msg('turn-final-answer'), msg('s_tail', true)], 1),
    ]);
    expect(report.violations.some((v) => v.kind === 'loading_after_stream' && v.key === 's_tail')).toBe(
      true,
    );
  });

  test('ignores mass vanish glitch when stable rows drop together', () => {
    const report = analyzeTimelineSamples([
      snap(
        ['u1', 'turn-batch-narration', 'turn-final-answer'],
        [msg('u1'), msg('turn-batch-narration'), msg('turn-final-answer')],
        0,
      ),
      snap(
        ['s_interim', 's_loading'],
        [msg('s_interim'), msg('s_loading', true)],
        1,
      ),
      snap(
        ['u1', 'turn-batch-narration', 'turn-final-answer'],
        [msg('u1'), msg('turn-batch-narration'), msg('turn-final-answer')],
        2,
      ),
    ]);
    expect(report.violations.filter((v) => v.kind === 'vanished')).toEqual([]);
  });

  test('ignores corrupt empty glitch sample after committed content', () => {
    const report = analyzeTimelineSamples([
      snap(['u1', 'turn-final-answer'], [msg('u1'), msg('turn-final-answer')], 0),
      snap(['anon:empty'], [], 1),
    ]);
    expect(report.violations).toEqual([]);
  });

  test('append-only growth has no violations', () => {
    const report = analyzeTimelineSamples([
      snap(['u1'], [msg('u1')], 0),
      snap(['u1', 'tg:t0'], [msg('u1'), msg('h_t0', false, true)], 1),
      snap(
        ['u1', 'turn-batch-narration', 'tg:t0'],
        [msg('u1'), msg('turn-batch-narration'), msg('h_t0', false, true)],
        2,
      ),
      snap(
        ['u1', 'turn-batch-narration', 'tg:t0+t1', 'turn-final-answer'],
        [
          msg('u1'),
          msg('turn-batch-narration'),
          msg('h_t0', false, true),
          msg('h_t1', false, true),
          msg('turn-final-answer'),
        ],
        3,
      ),
    ]);
    expect(report.violations).toEqual([]);
  });

  test('excerptTimelineSamples keeps violation neighborhood', () => {
    const samples = Array.from({ length: 100 }, (_, i) =>
      snap([`k${i}`], [msg(`m${i}`)], i),
    );
    const violations = [
      {
        kind: 'reorder' as const,
        key: 'turn-batch-narration',
        detail: 'test',
        sample_index: 50,
      },
    ];
    const excerpt = excerptTimelineSamples(samples, violations, 10);
    expect(excerpt.some((s) => s.t === 48)).toBe(true);
    expect(excerpt.some((s) => s.t === 50)).toBe(true);
    expect(excerpt.length).toBeLessThanOrEqual(10);
  });
});
