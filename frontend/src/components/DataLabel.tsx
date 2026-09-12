import type { CategoryTypeCode } from '../lib/types';

/**
 * Ground-truth data that must never be translated: category names, type labels,
 * booking comments and rule keys. They arrive from the database in German and stay
 * German in the English interface, because they are the user's own vocabulary and
 * the vocabulary of the spreadsheet this replaces.
 *
 * `lang="de"` + `translate="no"` also stops browser page translation from quietly
 * rewriting them.
 */
export function DataLabel({ children, className }: { children: string; className?: string }) {
  return (
    <span lang="de" translate="no" className={className}>
      {children}
    </span>
  );
}

const TYPE_CLASS: Record<string, string> = {
  einkommen: 'category-chip--einkommen',
  fixkosten: 'category-chip--fixkosten',
  variabel: 'category-chip--variabel',
  'variable kosten': 'category-chip--variabel',
  sparen: 'category-chip--sparen',
  sonstiges: 'category-chip--sonstiges',
};

function typeClass(typeLabel: string | null | undefined): string {
  if (!typeLabel) return 'category-chip--none';
  return TYPE_CLASS[typeLabel.toLowerCase()] ?? 'category-chip--sonstiges';
}

/**
 * A category, or the visible absence of one. An unmatched booking shows the
 * dashed "ohne Kategorie" chip rather than being folded into the real `Sonstiges`
 * category, which is a genuine category a user may have chosen deliberately.
 */
export function CategoryChip({
  name,
  typeLabel,
  fallback,
}: {
  name: string | null | undefined;
  typeLabel?: string | null;
  fallback: string;
}) {
  if (!name) {
    return (
      <span className="chip category-chip category-chip--none">
        <span aria-hidden="true">⚠</span> {fallback}
      </span>
    );
  }
  return (
    <span className={`chip category-chip ${typeClass(typeLabel)}`}>
      <DataLabel>{name}</DataLabel>
    </span>
  );
}

export function TypeChip({ label }: { label: string | null | undefined }) {
  if (!label) return null;
  return (
    <span className={`chip category-chip ${typeClass(label)}`}>
      <DataLabel>{label}</DataLabel>
    </span>
  );
}

export function typeCodeClass(code: CategoryTypeCode): string {
  return `category-chip--${code}`;
}
