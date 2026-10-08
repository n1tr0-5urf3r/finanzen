import { useEffect, useMemo, useState, type FormEvent, type ReactNode } from 'react';
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query';
import { AlertTriangle } from 'lucide-react';

import { DataLabel } from '../../components/DataLabel';
import { Money } from '../../components/Money';
import { Banner, Button, ErrorState, LoadingState } from '../../components/ui';
import { api, asList, jsonBody } from '../../lib/api';
import { formatDate, parseEuroInput } from '../../lib/format';
import { useT } from '../../lib/i18n';
import { invalidateAfterKitchenOwlChange, qk } from '../../lib/queryKeys';
import type {
  Booking,
  KoMetadata,
  KoPushChoices,
  KoPushIntent,
  KoTagSuggestion,
} from '../../lib/types';
import { useMaskedFieldClass } from '../../lib/privacy';

/**
 * KitchenOwl's own *add expense* form, mirrored.
 *
 * Mirrored rather than reinvented, because the person filling it in has the
 * KitchenOwl app on their phone and already knows this shape: name, amount, date,
 * KitchenOwl category, who paid, who it is split with and in what integer shares.
 *
 * Two rules the layout enforces:
 *
 * - The KitchenOwl category is its **own taxonomy**. It is not prefilled from the
 *   booking's category, there is no mapping behind it, and it is not styled like an
 *   app category chip — the note under the select says so in as many words.
 * - The **full booking amount** is what goes across, not the user's share. That is
 *   what the booking records and what actually left the account; KitchenOwl derives
 *   the shares itself from the weights below.
 *
 * The dialogue opens even when KitchenOwl is unreachable. Members and categories are
 * served from the cache with a staleness warning, and the push is an outbox entry
 * that survives the outage — refusing to open would defeat the whole mechanism.
 */
export function PushDialog({
  booking,
  onClose,
}: {
  booking: Booking;
  onClose: (message?: string) => void;
}) {
  const t = useT();
  const client = useQueryClient();
  const [waiting, setWaiting] = useState(false);

  // Where it landed, in the words the question is asked in: KitchenOwl sorts by
  // DATE, so a booking dated three weeks ago arrives three weeks down the list —
  // saying which date turns "it did not work" into "it is further down".
  const pushedMessage = (intent: KoPushIntent) =>
    t('ko.pushDone', {
      date: formatDate(intent.date),
      id: intent.externalId ? `#${intent.externalId}` : '',
    });

  const push = useMutation({
    mutationFn: (choices: KoPushChoices) =>
      api<KoPushIntent>(`/bookings/${booking.id}/kitchenowl`, {
        method: 'POST',
        ...jsonBody(choices),
      }),
    onSuccess: async (intent) => {
      invalidateAfterKitchenOwlChange(client);

      // The 202 says "durably queued", not "delivered", and closing on that left
      // the one question the user actually has unanswered — they went looking in
      // KitchenOwl, did not find it where they expected, and concluded it had
      // failed. So wait for the attempt, which normally takes about two seconds,
      // and say what happened. The waiting is bounded: the outbox is what makes
      // an unanswered push safe, so after eight seconds we stop watching and say
      // that honestly instead of spinning.
      if (intent.state === 'pushed') return onClose(pushedMessage(intent));

      setWaiting(true);
      // Short first, then backing off: a push usually lands in about two seconds,
      // so the common case should not wait a fixed 800ms to say so.
      for (let attempt = 0; attempt < 10; attempt += 1) {
        await new Promise((r) => setTimeout(r, attempt === 0 ? 300 : 800));
        const mine = asList<KoPushIntent>(
          await api<KoPushIntent[]>('/kitchenowl/push'),
        ).find((x) => x.bookingId === booking.id);
        if (mine?.state === 'pushed') {
          invalidateAfterKitchenOwlChange(client);
          setWaiting(false);
          return onClose(pushedMessage(mine));
        }
        if (mine?.state === 'failed' || mine?.state === 'abandoned') {
          setWaiting(false);
          return onClose(t('ko.pushFailed', { error: mine.lastError ?? '' }));
        }
      }
      setWaiting(false);
      onClose(t('ko.pushQueued'));
    },
  });

  return (
    <PushForm
      seed={{
        name: booking.comment,
        amountCents: booking.amountCents,
        date:
          booking.bookedOn ??
          `${booking.year}-${String(booking.month).padStart(2, '0')}-01`,
      }}
      initial={null}
      title={t('ko.pushTitle')}
      intro={t('ko.pushIntro')}
      // Says which of the two it is doing: handing the request over, then
      // waiting to hear back.
      submitLabel={waiting ? t('ko.pushWaiting') : t('ko.pushSubmit')}
      busy={push.isPending || waiting}
      error={push.isError ? push.error : null}
      onSubmit={(choices) => push.mutate(choices)}
      onCancel={() => onClose()}
    />
  );
}

/**
 * The same form for a bank statement line, which is not a booking yet.
 *
 * Nothing is sent from here. The choices are kept on the line, and the commit
 * that books it queues the push in the same transaction — so a line that is
 * never booked is never sent, and one that is arrives in KitchenOwl already
 * linked to its booking.
 */
export function StagePushDialog({
  seed,
  initial,
  onSave,
  onRemove,
  onClose,
}: {
  seed: PushSeed;
  initial: KoPushChoices | null;
  onSave: (choices: KoPushChoices) => Promise<unknown>;
  onRemove: () => Promise<unknown>;
  onClose: () => void;
}) {
  const t = useT();
  const save = useMutation({ mutationFn: onSave, onSuccess: () => onClose() });
  const remove = useMutation({ mutationFn: onRemove, onSuccess: () => onClose() });
  return (
    <PushForm
      seed={seed}
      initial={initial}
      title={t('ko.stageTitle')}
      intro={t('ko.stageIntro')}
      submitLabel={t('ko.stageSubmit')}
      busy={save.isPending || remove.isPending}
      error={save.error ?? remove.error}
      onSubmit={(choices) => save.mutate(choices)}
      onCancel={onClose}
      extraAction={
        initial && (
          <Button
            variant="ghost"
            type="button"
            busy={remove.isPending}
            onClick={() => remove.mutate()}
          >
            {t('ko.stageRemove')}
          </Button>
        )
      }
    />
  );
}

/** What the form starts from: a booking, or a statement line about to become one. */
export interface PushSeed {
  name: string;
  amountCents: number;
  date: string;
}

function PushForm({
  seed,
  initial,
  title,
  intro,
  submitLabel,
  busy,
  error,
  onSubmit,
  onCancel,
  extraAction,
}: {
  seed: PushSeed;
  /** Choices made earlier, to edit rather than start over. */
  initial: KoPushChoices | null;
  title: string;
  intro: string;
  submitLabel: string;
  busy: boolean;
  error: Error | null;
  onSubmit: (choices: KoPushChoices) => void;
  onCancel: () => void;
  extraAction?: ReactNode;
}) {
  const t = useT();
  const maskedField = useMaskedFieldClass();

  const metadata = useQuery({
    queryKey: qk.kitchenowl.metadata(),
    queryFn: () => api<KoMetadata>('/kitchenowl/metadata'),
  });

  const [name, setName] = useState(initial?.name ?? seed.name);
  const [description, setDescription] = useState(initial?.description ?? '');
  const [amount, setAmount] = useState(
    ((initial?.amountCents ?? seed.amountCents) / 100).toFixed(2).replace('.', ','),
  );
  const [date, setDate] = useState(initial?.date ?? seed.date);
  const [koCategoryId, setKoCategoryId] = useState(
    initial?.koCategoryId != null ? String(initial.koCategoryId) : '',
  );
  // The dialogue used to open with the category empty, so a Kaufland booking
  // landed in KitchenOwl uncategorised although the household has filed Kaufland
  // under Wocheneinkauf every time so far. The tagging queue's own suggestion now fills
  // it — for as long as the user has not picked something themselves.
  // Choices made earlier are the user's own; a suggestion does not overwrite them.
  const [categoryTouched, setCategoryTouched] = useState(initial !== null);
  const [suggestFor, setSuggestFor] = useState(seed.name);
  useEffect(() => {
    const timer = window.setTimeout(() => setSuggestFor(name), 300);
    return () => window.clearTimeout(timer);
  }, [name]);
  const suggestion = useQuery({
    queryKey: qk.kitchenowl.categorySuggestion(suggestFor.trim().toLowerCase()),
    queryFn: () =>
      api<KoTagSuggestion | null>(
        `/kitchenowl/category-suggestion?name=${encodeURIComponent(suggestFor.trim())}`,
      ),
    enabled: suggestFor.trim() !== '',
  });
  useEffect(() => {
    if (categoryTouched || !suggestion.isSuccess) return;
    // An untouched select mirrors the suggestion, including "none": a name that
    // changed to something unknown must not keep the last name's category.
    setKoCategoryId(suggestion.data ? String(suggestion.data.koCategoryId) : '');
  }, [suggestion.isSuccess, suggestion.data, categoryTouched]);
  const suggested =
    !categoryTouched && suggestion.data && String(suggestion.data.koCategoryId) === koCategoryId
      ? suggestion.data
      : null;
  const [paidBy, setPaidBy] = useState<number | null>(null);
  // Who is in the split and what weight they carry are two separate pieces of
  // state, and the weight is kept as TEXT. Coercing every keystroke to a number
  // makes the field impossible to clear — emptying it snaps back to 1, so typing
  // "3" over it yields 13 — and unticking somebody would destroy their weight.
  const [participants, setParticipants] = useState<Set<number>>(new Set());
  const [factors, setFactors] = useState<Record<number, string>>({});

  const members = useMemo(() => metadata.data?.members ?? [], [metadata.data]);

  // Default: the token's own member pays, everyone shares equally. Both are the
  // overwhelmingly common shape in the real household and both stay editable.
  // A staged push reopens with what was chosen before.
  useEffect(() => {
    if (members.length === 0 || paidBy !== null) return;
    if (initial?.paidById != null && initial.paidFor.length > 0) {
      setPaidBy(initial.paidById);
      setParticipants(new Set(initial.paidFor.map((share) => share.memberId)));
      setFactors(
        Object.fromEntries(
          members.map((m) => [
            m.memberId,
            String(initial.paidFor.find((share) => share.memberId === m.memberId)?.factor ?? 1),
          ]),
        ),
      );
      return;
    }
    setPaidBy(members.find((m) => m.isMe)?.memberId ?? members[0].memberId);
    setParticipants(new Set(members.map((m) => m.memberId)));
    setFactors(Object.fromEntries(members.map((m) => [m.memberId, '1'])));
  }, [members, paidBy, initial]);

  const amountCents = parseEuroInput(amount);
  const weight = (memberId: number) => Math.max(1, Number(factors[memberId]) || 1);
  const chosen = members.filter((m) => participants.has(m.memberId));

  function submit(event: FormEvent) {
    event.preventDefault();
    onSubmit({
      name,
      description: description || null,
      amountCents,
      date,
      koCategoryId: koCategoryId ? Number(koCategoryId) : null,
      paidById: paidBy,
      paidFor: chosen.map((m) => ({
        memberId: m.memberId,
        factor: weight(m.memberId),
      })),
    });
  }

  return (
    <div className="dialog__panel ko-push" role="dialog" aria-modal="true">
      <header>
        <h2>{title}</h2>
      </header>
      <p className="ko-push__intro">{intro}</p>

      {metadata.isLoading && <LoadingState />}
      {metadata.isError && <ErrorState error={metadata.error} />}

      {metadata.data && (
        <>
          {(metadata.data.stale || members.length === 0) && (
            <Banner tone="warn">
              <AlertTriangle size={14} aria-hidden="true" />{' '}
              {members.length === 0 ? t('ko.noMembers') : t('ko.metadataStale')}
            </Banner>
          )}

          <form className="form-stack" onSubmit={submit}>
            <div className="field">
              <label htmlFor="ko-name">{t('ko.pushName')}</label>
              <input
                id="ko-name"
                className="input"
                value={name}
                onChange={(e) => setName(e.target.value)}
                required
              />
            </div>

            <div className="ko-push__row">
              <div className="field">
                <label htmlFor="ko-amount">{t('ko.pushAmount')}</label>
                <input
                  id="ko-amount"
                  className={`input ${maskedField}`}
                  inputMode="decimal"
                  value={amount}
                  onChange={(e) => setAmount(e.target.value)}
                />
                {/* The full amount travels, not the share — spelled out, because
                    getting this backwards halves the household's record. */}
                <small>
                  {t('ko.amountHousehold')}:{' '}
                  <Money cents={amountCents} basis="household" />
                </small>
              </div>
              <div className="field">
                <label htmlFor="ko-date">{t('ko.date')}</label>
                <input
                  id="ko-date"
                  className="input"
                  type="date"
                  value={date}
                  onChange={(e) => setDate(e.target.value)}
                  required
                />
              </div>
            </div>

            <div className="field">
              <label htmlFor="ko-category">{t('ko.koCategory')}</label>
              <select
                id="ko-category"
                className="select"
                value={koCategoryId}
                onChange={(e) => {
                  setCategoryTouched(true);
                  setKoCategoryId(e.target.value);
                }}
              >
                <option value="">{t('ko.noKoCategory')}</option>
                {metadata.data.categories.map((category) => (
                  <option key={category.categoryId} value={category.categoryId}>
                    {category.name}
                  </option>
                ))}
              </select>
              {suggested ? (
                <small>
                  {suggested.source === 'precedent'
                    ? t('ko.suggestedFromHistory', { count: suggested.timesSeen })
                    : suggested.source === 'override'
                      ? t('ko.suggestedFromOverride')
                      : t('ko.suggestedFromRule')}
                </small>
              ) : (
                <small>{t('ko.koCategoryHint', { count: 32 })}</small>
              )}
            </div>

            <div className="field">
              <label htmlFor="ko-paidby">{t('ko.paidBy')}</label>
              <select
                id="ko-paidby"
                className="select"
                value={paidBy ?? ''}
                onChange={(e) => setPaidBy(Number(e.target.value))}
              >
                {members.map((member) => (
                  <option key={member.memberId} value={member.memberId}>
                    {member.name}
                  </option>
                ))}
              </select>
            </div>

            <fieldset className="field ko-push__split">
              <legend>{t('ko.paidFor')}</legend>
              {members.map((member) => (
                <label key={member.memberId} className="ko-push__share">
                  <input
                    type="checkbox"
                    checked={participants.has(member.memberId)}
                    onChange={(e) =>
                      setParticipants((prev) => {
                        const next = new Set(prev);
                        if (e.target.checked) next.add(member.memberId);
                        else next.delete(member.memberId);
                        return next;
                      })
                    }
                  />
                  <DataLabel>{member.name}</DataLabel>
                  <input
                    className="input ko-push__factor"
                    type="number"
                    min={1}
                    step={1}
                    aria-label={`${t('ko.splitFactor')} ${member.name}`}
                    value={factors[member.memberId] ?? ''}
                    disabled={!participants.has(member.memberId)}
                    onChange={(e) =>
                      setFactors((prev) => ({ ...prev, [member.memberId]: e.target.value }))
                    }
                    onBlur={(e) =>
                      setFactors((prev) => ({
                        ...prev,
                        [member.memberId]: String(Math.max(1, Number(e.target.value) || 1)),
                      }))
                    }
                  />
                </label>
              ))}
              {/* Weights, not percentages: 1 : 1 is an even split and 12 : 7 is a
                  real split from the household's own history. */}
              <small>{t('ko.splitFactor')} — 1 : 1</small>
            </fieldset>

            <div className="field">
              <label htmlFor="ko-description">{t('ko.pushDescription')}</label>
              <input
                id="ko-description"
                className="input"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
              />
              <small>{t('ko.pushMarkerNote')}</small>
            </div>

            {error && <ErrorState error={error} />}

            <div className="ko-push__actions">
              <Button variant="ghost" type="button" onClick={onCancel}>
                {t('common.cancel')}
              </Button>
              {extraAction}
              <Button
                type="submit"
                busy={busy}
                disabled={
                  !amountCents ||
                  amountCents <= 0 ||
                  paidBy === null ||
                  chosen.length === 0
                }
              >
                {submitLabel}
              </Button>
            </div>
          </form>
        </>
      )}
    </div>
  );
}
