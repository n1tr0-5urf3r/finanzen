import { describe, expect, it } from 'vitest';

import { sortedByName } from './categories';

const cat = (name: string) => ({ name });

describe('categories in a picker', () => {
  it('sorts by name, not by the order the API happens to return', () => {
    const asStored = [cat('Miete'), cat('Abos & Streaming'), cat('Gehalt')];
    expect(sortedByName(asStored).map((c) => c.name)).toEqual([
      'Abos & Streaming',
      'Gehalt',
      'Miete',
    ]);
  });

  /**
   * German collation, which is the whole reason this is a collator and not
   * `a.name < b.name`: Ärzte belongs with A, not after Z, and a user hunting for
   * "Öffentliche" should not find it exiled to the end of a list of thirty-two.
   */
  it('files umlauts where a German reader looks for them', () => {
    const names = ['Zoo', 'Ärzte', 'Apotheke', 'Öl', 'Obst'].map(cat);
    expect(sortedByName(names).map((c) => c.name)).toEqual([
      'Apotheke',
      'Ärzte',
      'Obst',
      'Öl',
      'Zoo',
    ]);
  });

  it('leaves the caller’s array alone', () => {
    const original = [cat('Miete'), cat('Auto & Parken')];
    const sorted = sortedByName(original);
    expect(original.map((c) => c.name)).toEqual(['Miete', 'Auto & Parken']);
    expect(sorted).not.toBe(original);
  });
});
