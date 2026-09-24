import { useSearchParams } from "react-router-dom";

import { ScopeNote } from "../../components/Money";
import { PageHeader } from "../../components/ui";
import { YearPicker } from "../../components/YearPicker";
import { useT } from "../../lib/i18n";
import type { MessageKey } from "../../lib/messages/de";
import { CompareTab } from "../compare/CompareTab";
import { MonthsTab } from "../months/MonthsTab";
import { CategoryTab } from "./CategoryTab";
import { FlowPeriodPicker, FlowTab } from "./FlowTab";
import { OverYearsTab } from "./OverYearsTab";

type Tab = "kategorien" | "monate" | "fluss" | "vergleich" | "jahre";

/**
 * Five views, in two groups, because they answer two different kinds of
 * question: what ONE year looked like, and how years compare. The split is drawn
 * rather than labelled — two segmented groups side by side, which also wrap at
 * the group boundary on a phone instead of mid-sentence.
 */
const ONE_YEAR: [Tab, MessageKey][] = [
  ["kategorien", "analysis.tabCategories"],
  ["monate", "analysis.tabMonths"],
  ["fluss", "analysis.tabFlow"],
];
const ACROSS_YEARS: [Tab, MessageKey][] = [
  ["vergleich", "analysis.tabCompare"],
  ["jahre", "analysis.tabOverYears"],
];
const TABS: Tab[] = [...ONE_YEAR, ...ACROSS_YEARS].map(([value]) => value);

/**
 * Everything that answers "where did the money go this year".
 *
 * The category breakdown and the year-against-year comparison were two separate
 * screens, which is how the household's identical pair ended up as one screen
 * with a switch and the personal one as two entries in the navigation. They are
 * near-clones — same columns shape, same sorter, same year, same table-and-cards
 * duality — so they are two tabs of one screen now, the way Kategorien, Import
 * and Einstellungen have always done it.
 *
 * The year lives in the URL and is shared by both tabs: switching from the
 * breakdown of 2024 to the comparison of 2024 should not send you back to today.
 */
/** Each tab states its own convention; one subtitle for all would be wrong for most. */
const SUBTITLE: Record<Tab, MessageKey> = {
  kategorien: "analysis.intro",
  monate: "months.intro",
  fluss: "flow.intro",
  vergleich: "compare.intro",
  jahre: "overYears.intro",
};

export function AnalysisPage() {
  const t = useT();
  const [params, setParams] = useSearchParams();
  const year = Number(params.get("jahr")) || new Date().getFullYear();
  const view = params.get("ansicht");
  const tab: Tab = TABS.find((value) => value === view) ?? "kategorien";

  function setParam(key: string, value: string | null) {
    setParams(
      (prev) => {
        const next = new URLSearchParams(prev);
        if (value === null) next.delete(key);
        else next.set(key, value);
        return next;
      },
      { replace: true },
    );
  }

  return (
    <>
      <PageHeader
        title={t("analysis.title")}
        // Each tab states its own convention: the breakdown reads as a flow, the
        // comparison explains what it compares. One subtitle for both would be
        // wrong for one of them.
        subtitle={t(SUBTITLE[tab])}
      />

      <div className="tabs tabs--split">
        {[ONE_YEAR, ACROSS_YEARS].map((group) => (
          <div key={group[0]![0]} className="segmented" role="tablist">
            {group.map(([value, labelKey]) => (
              <button
                key={value}
                type="button"
                role="tab"
                aria-pressed={tab === value}
                aria-selected={tab === value}
                onClick={() =>
                  setParam("ansicht", value === "kategorien" ? null : value)
                }
              >
                {t(labelKey)}
              </button>
            ))}
          </div>
        ))}
      </div>

      {/* The over-the-years tab IS every year, so the year picker there is a
          control that does nothing — and a filter bar holding nothing else is
          furniture. Both go; the year keeps its value for the other tabs. */}
      {tab !== "jahre" && (
        <div className="panel panel--pad filter-bar">
          <div style={{ minWidth: "8rem" }}>
            <YearPicker
              id="analysis-year"
              value={year}
              onChange={(next) => setParam("jahr", String(next))}
            />
          </div>
          {tab === "fluss" && (
            <div style={{ minWidth: "10rem" }}>
              <FlowPeriodPicker year={year} />
            </div>
          )}
          <ScopeNote transfersIncluded={false} />
        </div>
      )}

      {tab === "vergleich" && <CompareTab year={year} />}
      {tab === "jahre" && <OverYearsTab />}
      {tab === "fluss" && <FlowTab year={year} />}
      {tab === "kategorien" && <CategoryTab year={year} />}
      {tab === "monate" && <MonthsTab year={year} />}
    </>
  );
}
