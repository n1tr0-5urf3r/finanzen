import type { Category } from './types';

/**
 * German collation, so Ä sorts with A rather than after Z, and ß with ss.
 * Built once: constructing a collator per comparison is the slow way to do this.
 */
const byName = new Intl.Collator('de', { sensitivity: 'base', numeric: true });

/**
 * Categories in the order a person looks for them.
 *
 * The API returns them in the app's own `sort_order`, which is the right order
 * for the Kategorien screen — it groups by type and keeps a deliberate sequence
 * within each. In a PICKER that order is invisible: you know the name you want,
 * and hunting for "Miete" somewhere in a list of thirty-two sorted by something
 * you cannot see is the whole problem. Every dropdown sorts by name instead.
 *
 * Returns a new array; the query cache's data is never mutated.
 */
export function sortedByName<T extends { name: string }>(categories: T[]): T[] {
  return [...categories].sort((a, b) => byName.compare(a.name, b.name));
}

/** The same, for anything whose display name is a `label` rather than a `name` —
    the over-the-years rows, which are categories, types or household members. */
export function sortedByLabel<T extends { label: string }>(items: T[]): T[] {
  return [...items].sort((a, b) => byName.compare(a.label, b.label));
}

/** The same, for the KitchenOwl categories, which carry a nullable name. */
export function sortedByNameNullable<T extends { name: string | null }>(items: T[]): T[] {
  return [...items].sort((a, b) => byName.compare(a.name ?? '', b.name ?? ''));
}

export type { Category };
