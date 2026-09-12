import { useId, type ReactNode } from 'react';

import { Money } from '../components/Money';
import { useT } from '../lib/i18n';

export interface ChartDatum {
  /** Already a data string (a German month name) or UI chrome; the caller decides. */
  label: ReactNode;
  values: (number | null)[];
}

/**
 * Every chart is also a table.
 *
 * The same markup is the accessibility story, the answer to "is that number
 * right?", and the phone fallback: below 560px the SVG is hidden and the table
 * shown, because a twelve-band chart on a 400px screen is decoration. One
 * mechanism, three jobs — which is why it lives in the frame rather than being
 * re-derived per chart.
 */
export function ChartFrame({
  title,
  note,
  columns,
  data,
  height = 240,
  children,
}: {
  title: string;
  note?: string;
  /** Column headings for the data table, one per series. */
  columns: string[];
  data: ChartDatum[];
  height?: number;
  children: ReactNode;
}) {
  const t = useT();
  const id = useId();

  return (
    <figure className="chart">
      <figcaption className="chart__title" id={`${id}-title`}>
        {title}
      </figcaption>
      <svg
        className="chart__svg"
        viewBox={`0 0 720 ${height}`}
        role="img"
        aria-labelledby={`${id}-title`}
      >
        {children}
      </svg>
      <div className="chart__data">
        <table className="data-table">
          <caption>{t('chart.dataTable')}</caption>
          <thead>
            <tr>
              <th />
              {columns.map((c) => (
                <th key={c} className="num">
                  {c}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {data.map((row, i) => (
              <tr key={i}>
                <th scope="row">{row.label}</th>
                {row.values.map((v, j) => (
                  <td key={j} className="num">
                    <Money cents={v} basis="net" tone="auto" />
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {note && <p className="chart__note">{note}</p>}
    </figure>
  );
}

/** Horizontal gridlines and their value labels, shared by every chart body. */
export function Gridlines({
  ticks,
  y,
  left,
  right,
  format,
}: {
  ticks: number[];
  y: (v: number) => number;
  left: number;
  right: number;
  format: (v: number) => string;
}) {
  return (
    <g>
      {ticks.map((tick) => (
        <g key={tick}>
          <line
            x1={left}
            x2={right}
            y1={y(tick)}
            y2={y(tick)}
            className={tick === 0 ? 'chart__baseline' : 'chart__grid'}
          />
          <text x={left - 6} y={y(tick) + 4} className="chart__tick" textAnchor="end">
            {format(tick)}
          </text>
        </g>
      ))}
    </g>
  );
}
