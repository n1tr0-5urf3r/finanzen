import { act, renderHook } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { useQuickAdd, usePrediction } from './useQuickAdd';
import type { Rule } from '../../lib/types';

describe('cents-first amount entry', () => {
  it('appends digits to a cent buffer', () => {
    const { result } = renderHook(() => useQuickAdd());
    const type = (...digits: number[]) =>
      act(() => digits.forEach((d) => result.current.dispatch({ type: 'digit', value: d })));

    type(1);
    expect(result.current.state.cents).toBe(1); // 0,01
    type(2);
    expect(result.current.state.cents).toBe(12); // 0,12
    type(5, 0);
    expect(result.current.state.cents).toBe(1250); // 12,50
  });

  it('has a double-zero key because most amounts are whole euros', () => {
    const { result } = renderHook(() => useQuickAdd());
    act(() => result.current.dispatch({ type: 'digit', value: 5 }));
    act(() => result.current.dispatch({ type: 'doubleZero' }));
    expect(result.current.state.cents).toBe(500); // 5,00
  });

  it('pops one digit on backspace', () => {
    const { result } = renderHook(() => useQuickAdd());
    act(() => [1, 2, 5, 0].forEach((d) => result.current.dispatch({ type: 'digit', value: d })));
    act(() => result.current.dispatch({ type: 'backspace' }));
    expect(result.current.state.cents).toBe(125);
  });

  it('keeps kind and month after a save, because entries come in runs', () => {
    const { result } = renderHook(() => useQuickAdd({ kind: 'income', month: 3 }));
    act(() => result.current.dispatch({ type: 'digit', value: 9 }));
    act(() => result.current.dispatch({ type: 'setComment', comment: 'Gehalt' }));
    act(() => result.current.dispatch({ type: 'reset' }));
    expect(result.current.state.cents).toBe(0);
    expect(result.current.state.comment).toBe('');
    expect(result.current.state.kind).toBe('income');
    expect(result.current.state.month).toBe(3);
  });

  it('cycles expense to income to transfer', () => {
    const { result } = renderHook(() => useQuickAdd());
    expect(result.current.state.kind).toBe('expense');
    act(() => result.current.dispatch({ type: 'cycleKind' }));
    expect(result.current.state.kind).toBe('income');
    act(() => result.current.dispatch({ type: 'cycleKind' }));
    expect(result.current.state.kind).toBe('transfer');
  });
});

describe('category prediction', () => {
  const rules: Rule[] = [
    { id: '1', comment: 'tanken', normalizedComment: 'tanken', categoryId: 'c1',
      categoryName: 'Auto & Parken', kindOverride: null, source: 'seed', matchCount: 12 },
    { id: '2', comment: 'Essen', normalizedComment: 'essen', categoryId: 'c2',
      categoryName: 'Essen auswärts', kindOverride: null, source: 'seed', matchCount: 90 },
    { id: '3', comment: 'Essen Silvester', normalizedComment: 'essen silvester', categoryId: 'c2',
      categoryName: 'Essen auswärts', kindOverride: null, source: 'seed', matchCount: 1 },
  ];

  it('matches an exact rule case-insensitively', () => {
    const { result } = renderHook(() => usePrediction('TANKEN', rules));
    expect(result.current).toMatchObject({ status: 'rule', categoryName: 'Auto & Parken' });
  });

  it('offers a guess for a unique prefix', () => {
    const { result } = renderHook(() => usePrediction('tank', rules));
    expect(result.current).toMatchObject({ status: 'guess', categoryName: 'Auto & Parken' });
  });

  it('reports unmatched rather than guessing between two prefixes', () => {
    // "essen" prefixes two rules, so picking one would be a coin flip.
    const { result } = renderHook(() => usePrediction('esse', rules));
    expect(result.current.status).toBe('unmatched');
  });

  it('reports unmatched for a comment no rule covers', () => {
    const { result } = renderHook(() => usePrediction('Hofladen Brinkmann', rules));
    expect(result.current.status).toBe('unmatched');
  });
});
