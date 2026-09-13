import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { ArrowLeftRight, ChevronRight, Delete, X } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { api, jsonBody, asList } from '../../lib/api';
import { formatEuro, monthName } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterBookingChange, qk } from '../../lib/queryKeys';
import type { Booking, Category, CommentSummary, Rule } from '../../lib/types';
import { CategorySheet } from './CategorySheet';
import { CommentSheet } from './CommentSheet';
import { useKeyboardBridge, usePrediction, useQuickAdd, useSuggestions } from './useQuickAdd';
import { useMaskAmount } from '../../lib/privacy';

const KEYS = ['1', '2', '3', '4', '5', '6', '7', '8', '9', '00', '0', '⌫'] as const;

export function QuickAddPage() {
  const t = useT();
  const navigate = useNavigate();
  const client = useQueryClient();
  const { state, dispatch } = useQuickAdd();
  const [sheetOpen, setSheetOpen] = useState(false);
  const [categoryOpen, setCategoryOpen] = useState(false);
  // A category chosen by hand. The rule table predicts one from the comment, but
  // a new account has no rules and some bookings simply do not follow one, so
  // there has to be a way to say it outright.
  const [override, setOverride] = useState<{ id: string; name: string } | null>(null);
  const [saved, setSaved] = useState<Booking | null>(null);

  const rules = useQuery({
    queryKey: qk.taxonomy.rules(),
    queryFn: () => api<Rule[]>('/rules'),
    staleTime: 30 * 60_000,
  });
  const comments = useQuery({
    queryKey: qk.bookings.comments(),
    queryFn: () => api<CommentSummary[]>('/bookings/comments'),
    staleTime: 5 * 60_000,
  });
  const categories = useQuery({
    queryKey: qk.taxonomy.categories(),
    queryFn: () => api<Category[]>('/categories'),
    staleTime: 30 * 60_000,
  });

  const prediction = usePrediction(state.comment, rules.data);
  const tiles = useSuggestions(comments.data, rules.data, 5);

  const onKey = useKeyboardBridge(dispatch);
  useEffect(() => {
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onKey]);

  const create = useMutation({
    mutationFn: () =>
      api<Booking>('/bookings', {
        method: 'POST',
        ...jsonBody({
          year: state.year,
          month: state.month,
          kind: state.kind,
          amountCents: state.cents,
          comment: state.comment.trim(),
          // Sent only when chosen by hand; otherwise the server resolves it from
          // the rule table and stays the authority.
          ...(override ? { categoryId: override.id } : {}),
        }),
      }),
    onSuccess: (booking) => {
      invalidateAfterBookingChange(client, [booking.year]);
      client.invalidateQueries({ queryKey: qk.bookings.comments() });
      setSaved(booking);
      setOverride(null);
      dispatch({ type: 'reset' });
    },
  });

  const ready = state.cents > 0 && state.comment.trim().length > 0;
  // A greyed-out button that will not say why is the worst thing on the screen
  // that matters most. Both conditions are reachable by accident — an amount can
  // be backspaced to nothing, a comment can be cleared — so the bar says which
  // one is missing rather than leaving the user to guess at the one screen where
  // guessing costs a booking.
  const missing = !ready
    ? state.cents === 0
      ? t('quick.needAmount')
      : t('quick.needComment')
    : null;
  const confirmTone = override
    ? 'rule'
    : prediction.status === 'rule'
      ? 'rule'
      : prediction.status === 'guess'
        ? 'guess'
        : 'unknown';
  const confirmLabel = override
    ? t('quick.saveWith', { category: override.name })
    : prediction.status === 'rule'
      ? t('quick.saveWith', { category: prediction.categoryName })
      : prediction.status === 'guess'
        ? t('quick.saveGuess', { category: prediction.categoryName })
        : t('quick.saveUnknown');

  return (
    <div className="quick">
      <div className="quick__top">
        <button className="icon-button" onClick={() => navigate('/dashboard')} aria-label={t('common.close')}>
          <X size={20} aria-hidden="true" />
        </button>
        <h1>{t('quick.title')}</h1>
        <button
          className="icon-button"
          onClick={() => dispatch({ type: 'cycleKind' })}
          aria-label={t('quick.switchKind')}
        >
          <ArrowLeftRight size={18} aria-hidden="true" />
        </button>
      </div>

      <div className="quick__amount">
        {/* aria-live so the running value is announced as it is typed. */}
        <output className={`amount-display amount-display--${state.kind}`} aria-live="polite" aria-atomic="true">
          {formatEuro(state.cents)}
        </output>
        <span className="quick__meta">
          {t(`bookings.kind.${state.kind}`)} · {monthName(state.month)} {state.year}
        </span>
      </div>

      <div className="quick__suggestions">
        <p className="quick__section-label">{t('quick.frequent')}</p>
        <div className="tile-grid">
          {tiles.map((tile) => (
            <SuggestionTile
              key={tile.comment}
              tile={tile}
              selected={tile.comment === state.comment}
              onSelect={() => dispatch({ type: 'setComment', comment: tile.comment })}
            />
          ))}
          <button className="tile tile--more" onClick={() => setSheetOpen(true)}>
            <span className="tile__comment" style={{ textAlign: 'center' }}>⌨</span>
            <span className="tile__category" style={{ textAlign: 'center' }}>
              {t('quick.otherComment')}
            </span>
          </button>
        </div>
      </div>

      <div className="keypad" role="group" aria-label={t('quick.keypad')}>
        {KEYS.map((key) => (
          <button
            key={key}
            onClick={() => {
              if (key === '⌫') dispatch({ type: 'backspace' });
              else if (key === '00') dispatch({ type: 'doubleZero' });
              else dispatch({ type: 'digit', value: Number(key) });
            }}
            aria-label={key === '⌫' ? t('quick.backspace') : key}
          >
            {key === '⌫' ? <Delete size={20} aria-hidden="true" /> : key}
          </button>
        ))}
      </div>

      {/* The comment has its own row, next to the category's.
          It used to be reachable only through a "⌨" tile inside
          `.quick__suggestions`, which is a flex child that scrolls — on a short
          screen it collapses to a single row and that tile is simply not on the
          page. The confirm bar then said "enter a comment" and pointed at nothing
          the user could see. A row that is always there cannot collapse. */}
      <button type="button" className="quick__category" onClick={() => setSheetOpen(true)}>
        <span className="quick__category-label">{t('bookings.comment')}</span>
        <span className="quick__category-value">
          {state.comment.trim() ? (
            <DataLabel>{state.comment}</DataLabel>
          ) : (
            <span className="quick__category-none">{t('quick.needComment')}</span>
          )}
        </span>
        <ChevronRight size={16} aria-hidden="true" />
      </button>

      {/* The rule table predicts a category from the comment, but a fresh account
          has no rules and some bookings follow none — so the category is always
          visible here and always changeable, rather than only inferable. */}
      <button
        type="button"
        className="quick__category"
        onClick={() => setCategoryOpen(true)}
      >
        <span className="quick__category-label">{t('bookings.category')}</span>
        <span className="quick__category-value">
          {override ? (
            <DataLabel>{override.name}</DataLabel>
          ) : prediction.status === 'rule' || prediction.status === 'guess' ? (
            <DataLabel>{prediction.categoryName}</DataLabel>
          ) : (
            <span className="quick__category-none">{t('bookings.sourceNone')}</span>
          )}
        </span>
        <ChevronRight size={16} aria-hidden="true" />
      </button>

      {/* Missing a comment is not a dead end — the bar becomes the way to fix it.
          Missing an amount still disables it, because the keypad is right there
          and visible; there is nowhere to send anyone. */}
      <button
        className={`quick__confirm quick__confirm--${confirmTone}`}
        disabled={state.cents === 0 || create.isPending}
        onClick={() => (ready ? create.mutate() : setSheetOpen(true))}
      >
        {missing ?? confirmLabel}
      </button>
      {create.isError && (
        <p className="quick__error" role="alert">
          {t('quick.saveFailed')}
        </p>
      )}

      {sheetOpen && (
        <CommentSheet
          rules={asList(rules.data)}
          comments={asList(comments.data)}
          onPick={(comment) => {
            dispatch({ type: 'setComment', comment });
            setSheetOpen(false);
          }}
          onClose={() => setSheetOpen(false)}
        />
      )}

      {categoryOpen && (
        <CategorySheet
          categories={asList(categories.data)}
          selectedId={override?.id ?? null}
          onPick={(picked) => {
            setOverride(picked);
            setCategoryOpen(false);
          }}
          onClose={() => setCategoryOpen(false)}
        />
      )}

      {saved && <SavedToast booking={saved} onDismiss={() => setSaved(null)} />}
    </div>
  );
}

function SuggestionTile({
  tile,
  selected,
  onSelect,
}: {
  tile: CommentSummary;
  selected: boolean;
  onSelect: () => void;
}) {
  return (
    <button className="tile" aria-pressed={selected} onClick={onSelect}>
      {/* Both lines are data: the comment the user types, and the category the
          rule table resolves it to. Neither is translated. */}
      <span className="tile__comment">
        <DataLabel>{tile.comment}</DataLabel>
      </span>
      <span className="tile__category">
        {tile.categoryName ? <DataLabel>{tile.categoryName}</DataLabel> : '—'}
      </span>
    </button>
  );
}

/**
 * After an unmatched save, the fix is offered inline: two taps after the mistake,
 * and creating the rule recategorises every existing booking with that comment.
 * This is the single highest-leverage action in the app and it has to be reachable
 * from the phone.
 */
function SavedToast({ booking, onDismiss }: { booking: Booking; onDismiss: () => void }) {
  const t = useT();
  const maskAmount = useMaskAmount();
  const navigate = useNavigate();
  const unmatched = booking.categorySource === 'unresolved';

  useEffect(() => {
    if (unmatched) return;
    const timer = setTimeout(onDismiss, 4000);
    return () => clearTimeout(timer);
  }, [unmatched, onDismiss]);

  return (
    <div className="toast-stack">
      <div className={`toast ${unmatched ? 'toast--warn' : ''}`} role="status">
        <div>
          <div>
            {t('quick.saved', {
              amount: maskAmount(formatEuro(booking.amountCents)),
              comment: booking.comment,
            })}
          </div>
          {unmatched && <div style={{ opacity: 0.8 }}>{t('quick.savedUnknown')}</div>}
        </div>
        {unmatched ? (
          <button onClick={() => navigate('/kategorien')}>{t('quick.createRule')}</button>
        ) : (
          <button onClick={onDismiss}>{t('common.close')}</button>
        )}
      </div>
    </div>
  );
}
