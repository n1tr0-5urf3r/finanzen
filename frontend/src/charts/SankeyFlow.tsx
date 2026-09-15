import type { SVGProps } from 'react';

import { ChartFrame, type ChartDatum } from './ChartFrame';
import { formatEuro } from '../lib/format';
import { useMaskAmount } from '../lib/privacy';
import { useNarrow } from '../lib/useNarrow';
import type { CategoryTypeCode } from '../lib/types';
import type { FlowModel, FlowNode, FlowTone } from '../lib/sankey';

/* The viewBox is 720 units wide like every other chart here, and the columns are
   placed so that the two label gutters survive the phone font size: below 560px
   the CSS draws text at 22 units, so 150 units on the left is about 12 characters
   and 200 on the right about 16. Longer names are cut with an ellipsis and kept
   whole in the node's tooltip and in the data table underneath — which is visible
   on exactly those screens. Nothing is ever drawn wider than the viewBox: a chart
   wider than the phone makes the whole page wider than the phone. */
const SRC_X = 150;
const HUB_X = 330;
const TGT_X = 500;
const NODE_W = 10;
const LABEL_GAP = 8;

const TOP = 30;
const BOTTOM = 12;
const PAD = 14;

/* How many characters fit a gutter, and how tall a node's slot is. Both depend on
   the rendered scale, which depends on the screen: a phone draws this 720-unit
   viewBox at about half size, so the text is twice as big in viewBox units and a
   diagram that is comfortable on a desktop is half as tall and half as legible
   there. Same data, two geometries. */
const SRC_CHARS = { wide: 22, narrow: 11 };
const TGT_CHARS = { wide: 26, narrow: 15 };
const SLOT = { wide: 58, narrow: 104 };
const MIN_HEIGHT = { wide: 300, narrow: 520 };

/* A category name is ground-truth data: it stays German in the English interface
   and page translation must leave it alone. React's SVG typings have no
   `translate` attribute, though the DOM does, so it is spread in from here. */
const DATA_TEXT = { lang: 'de', translate: 'no' } as SVGProps<SVGTextElement>;

function cut(label: string, max: number): string {
  return label.length <= max ? label : `${label.slice(0, max - 1).trimEnd()}…`;
}

function toneClass(tone: FlowTone): string {
  switch (tone) {
    case 'income':
      return 'flow--einkommen';
    case 'credit':
      return 'flow--credit';
    case 'surplus':
      return 'flow--surplus';
    case 'none':
      return 'flow--none';
    default:
      return `flow--${tone}`;
  }
}

interface Placed {
  node: FlowNode;
  y: number;
  h: number;
}

/** Stack a column, centred in the space the taller column needs. */
function place(nodes: FlowNode[], k: number, usable: number): Placed[] {
  const bodies = nodes.map((n) => Math.max(1, n.amountCents * k));
  const used = bodies.reduce((a, b) => a + b, 0) + Math.max(0, nodes.length - 1) * PAD;
  let y = TOP + Math.max(0, (usable - used) / 2);
  return nodes.map((node, i) => {
    const h = bodies[i] as number;
    const placed = { node, y, h };
    y += h + PAD;
    return placed;
  });
}

/** The ribbon between two horizontal edges — a band, not a line, so its width IS
    the amount and no second encoding can contradict it. */
function ribbon(x0: number, y0: number, x1: number, y1: number, h: number): string {
  const mid = (x0 + x1) / 2;
  return [
    `M${x0},${y0}`,
    `C${mid},${y0} ${mid},${y1} ${x1},${y1}`,
    `L${x1},${y1 + h}`,
    `C${mid},${y1 + h} ${mid},${y0 + h} ${x0},${y0 + h}`,
    'Z',
  ].join(' ');
}

/**
 * The money flow of one period: what came in, through one total, back out.
 *
 * Every band's width is its share of the same total, both columns are drawn to
 * one scale, and the two sides balance by construction — so an eye can read
 * "half of it went to fixed costs" off the picture without reading a number.
 */
export function SankeyFlow({
  title,
  note,
  model,
  expanded,
  onToggleType,
  labels,
}: {
  title: string;
  note?: string;
  model: FlowModel;
  expanded: CategoryTypeCode | null;
  onToggleType?: (code: CategoryTypeCode) => void;
  /** Chrome for the parts a chart cannot speak: the table caption and the two
      accessible action names. */
  labels: { amount: string; expand: string; collapse: string };
}) {
  const mask = useMaskAmount();
  const narrow = useNarrow();
  const size = narrow ? 'narrow' : 'wide';

  const count = Math.max(model.sources.length, model.targets.length, 1);
  const height = Math.min(
    1600,
    Math.max(MIN_HEIGHT[size], TOP + BOTTOM + count * SLOT[size]),
  );
  const usable = height - TOP - BOTTOM - Math.max(0, count - 1) * PAD;
  const k = model.totalCents > 0 ? usable / model.totalCents : 0;

  const sources = place(model.sources, k, usable);
  const targets = place(model.targets, k, usable);

  // The hub is one rect; its two faces are consumed in the same order as the
  // columns beside it, which is what keeps the ribbons from crossing.
  const hubH = Math.max(1, model.totalCents * k);
  const hubY = TOP + Math.max(0, (usable - hubH) / 2);

  let leftCursor = hubY;
  const leftSlots = sources.map((s) => {
    const y = leftCursor;
    leftCursor += s.h;
    return y;
  });
  let rightCursor = hubY;
  const rightSlots = targets.map((tgt) => {
    const y = rightCursor;
    rightCursor += tgt.h;
    return y;
  });

  const data: ChartDatum[] = model.links.map((link) => {
    const from = model.sources.find((n) => n.key === link.from);
    const to = model.targets.find((n) => n.key === link.to);
    return {
      label: (
        <span lang="de" translate="no">
          {from ? `${from.label} → ${model.hub.label}` : `${model.hub.label} → ${to?.label ?? ''}`}
        </span>
      ),
      values: [link.amountCents],
    };
  });

  /**
   * Room for a second line is not a property of the node but of its NEIGHBOURS:
   * two labels collide when their centres are closer than the block is tall.
   * Deciding on the node's own height alone dropped the figure from a band that
   * was a hair thinner than the one above it, which reads as missing data rather
   * than as a deliberate omission.
   */
  function nodeLabel(placed: Placed, side: 'source' | 'target', column: Placed[], i: number) {
    const { node, y, h } = placed;
    const source = side === 'source';
    const centreOf = (p: Placed | undefined) => (p ? p.y + p.h / 2 : null);
    const here = y + h / 2;
    const above = centreOf(column[i - 1]);
    const below = centreOf(column[i + 1]);
    const need = narrow ? 52 : 32;
    const roomAbove = above === null || here - above >= need;
    const roomBelow = below === null || below - here >= need;
    const x = source ? SRC_X - LABEL_GAP : TGT_X + NODE_W + LABEL_GAP;
    const anchor = source ? 'end' : 'start';
    const centre = here + (narrow ? -6 : 0);
    const twoLines = roomAbove && roomBelow;
    return (
      <>
        <text
          {...DATA_TEXT}
          x={x}
          y={twoLines ? centre - 2 : centre + 4}
          textAnchor={anchor}
          className="flow__label"
        >
          {cut(node.label, source ? SRC_CHARS[size] : TGT_CHARS[size])}
        </text>
        {twoLines && (
          <text
            x={x}
            y={centre + (narrow ? 26 : 16)}
            textAnchor={anchor}
            className="flow__value"
          >
            {mask(formatEuro(node.amountCents))}
          </text>
        )}
      </>
    );
  }

  return (
    <ChartFrame
      title={title}
      note={note}
      columns={[labels.amount]}
      data={data}
      valueBasis="gross"
      height={height}
    >
      <g className="flow">
        {/* Ribbons first: the nodes and their labels sit on top of them. */}
        {sources.map((s, i) => (
          <path
            key={`link-${s.node.key}`}
            className={`flow__link ${toneClass(s.node.tone)}`}
            d={ribbon(SRC_X + NODE_W, s.y, HUB_X, leftSlots[i] as number, s.h)}
          />
        ))}
        {targets.map((tgt, i) => (
          <path
            key={`link-${tgt.node.key}`}
            className={`flow__link ${toneClass(tgt.node.tone)}`}
            d={ribbon(HUB_X + NODE_W, rightSlots[i] as number, TGT_X, tgt.y, tgt.h)}
          />
        ))}

        <rect
          x={HUB_X}
          y={hubY}
          width={NODE_W}
          height={hubH}
          className="flow__node flow--einkommen"
        />
        <text x={HUB_X + NODE_W / 2} y={TOP - 14} textAnchor="middle" className="flow__hub">
          {`${model.hub.label} · ${mask(formatEuro(model.hub.amountCents))}`}
        </text>

        {sources.map((s, i) => (
          <g key={s.node.key}>
            <title>{`${s.node.label} · ${mask(formatEuro(s.node.amountCents))}`}</title>
            <rect
              x={SRC_X}
              y={s.y}
              width={NODE_W}
              height={s.h}
              className={`flow__node ${toneClass(s.node.tone)}`}
            />
            {nodeLabel(s, 'source', sources, i)}
          </g>
        ))}

        {targets.map((tgt, i) => {
          const code = tgt.node.typeCode;
          const clickable = Boolean(code && (tgt.node.expandable || tgt.node.nested) && onToggleType);
          const action = tgt.node.nested ? labels.collapse : labels.expand;
          return (
            <g
              key={tgt.node.key}
              className={clickable ? 'flow__group flow__group--clickable' : 'flow__group'}
              role={clickable ? 'button' : undefined}
              tabIndex={clickable ? 0 : undefined}
              aria-label={clickable ? `${tgt.node.label}: ${action}` : undefined}
              onClick={clickable && code ? () => onToggleType?.(code) : undefined}
              onKeyDown={
                clickable && code
                  ? (event) => {
                      if (event.key === 'Enter' || event.key === ' ') {
                        event.preventDefault();
                        onToggleType?.(code);
                      }
                    }
                  : undefined
              }
            >
              <title>
                {`${tgt.node.label} · ${mask(formatEuro(tgt.node.amountCents))}${
                  clickable ? ` — ${action}` : ''
                }`}
              </title>
              <rect
                x={TGT_X}
                y={tgt.y}
                width={NODE_W}
                height={tgt.h}
                className={`flow__node ${toneClass(tgt.node.tone)}${
                  expanded && tgt.node.typeCode === expanded ? ' flow__node--open' : ''
                }`}
              />
              {nodeLabel(tgt, 'target', targets, i)}
            </g>
          );
        })}
      </g>
    </ChartFrame>
  );
}
