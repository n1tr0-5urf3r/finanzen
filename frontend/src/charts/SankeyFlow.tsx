import type { SVGProps } from 'react';

import { ChartFrame, type ChartDatum } from './ChartFrame';
import { formatEuro } from '../lib/format';
import { useMaskAmount } from '../lib/privacy';
import { useNarrow } from '../lib/useNarrow';
import type { CategoryTypeCode } from '../lib/types';
import type { FlowModel, FlowNode, FlowTone } from '../lib/sankey';

/* The viewBox is 720 units wide like every other chart here, and the columns are
   placed so that the label gutters survive the phone font size: below 560px the
   CSS draws text at 22 units, so 150 units is about 12 characters. Longer names
   are cut with an ellipsis and kept whole in the node's tooltip and in the data
   table underneath — which is visible on exactly those screens. Nothing is ever
   drawn wider than the viewBox: a chart wider than the phone makes the whole page
   wider than the phone.

   Fanning the types out into their categories adds a fourth column, and 720
   units cannot carry three label gutters. So in that layout the middle column
   labels itself ABOVE its own band, in the gap the padding opens up, and the two
   outer gutters keep the rest. */
const COLUMNS = {
  three: { src: 150, hub: 330, tgt: 500, leaf: 0 },
  four: { src: 126, hub: 276, tgt: 410, leaf: 560 },
};
const NODE_W = 10;
const LABEL_GAP = 8;

const TOP = 30;
const BOTTOM = 12;
/* Room between two nodes of a column. The four-column layout needs more of it,
   because the type labels live in that gap. */
const PAD = { three: { wide: 14, narrow: 14 }, four: { wide: 24, narrow: 38 } };

/* How many characters fit a gutter, and how tall a node's slot is. Both depend on
   the rendered scale, which depends on the screen: a phone draws this 720-unit
   viewBox at about half size, so the text is twice as big in viewBox units and a
   diagram that is comfortable on a desktop is half as tall and half as legible
   there. Same data, two geometries. */
const SRC_CHARS = { wide: 22, narrow: 11 };
const TGT_CHARS = { wide: 26, narrow: 15 };
const LEAF_CHARS = { wide: 20, narrow: 11 };
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

/** Stack a column, centred in the space the tallest column needs. */
function place(nodes: FlowNode[], k: number, usable: number, pad: number): Placed[] {
  const bodies = nodes.map((n) => Math.max(1, n.amountCents * k));
  const used = bodies.reduce((a, b) => a + b, 0) + Math.max(0, nodes.length - 1) * pad;
  let y = TOP + Math.max(0, (usable - used) / 2);
  return nodes.map((node, i) => {
    const h = bodies[i] as number;
    const placed = { node, y, h };
    y += h + pad;
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
 *
 * With `model.leaves` filled, the targets fan out once more into their own
 * categories. Same totals, one column further: the type keeps its band and its
 * subtotal and the detail hangs off it, instead of the two being alternatives.
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

  const fan = model.leaves.length > 0;
  const shape = fan ? 'four' : 'three';
  const x = COLUMNS[shape];
  const pad = PAD[shape][size];

  const count = Math.max(model.sources.length, model.targets.length, model.leaves.length, 1);
  // The fanned-out layout is allowed to be a good deal taller: it is opt-in, and
  // thirty categories squeezed into the three-column cap is the crowding this
  // switch exists to let you choose.
  const height = Math.min(
    fan ? 2600 : 1600,
    Math.max(MIN_HEIGHT[size], TOP + BOTTOM + count * SLOT[size]),
  );
  const usable = height - TOP - BOTTOM - Math.max(0, count - 1) * pad;
  const k = model.totalCents > 0 ? usable / model.totalCents : 0;

  const sources = place(model.sources, k, usable, pad);
  const leaves = place(model.leaves, k, usable, pad);
  const targets = place(model.targets, k, usable, pad);

  // With a fourth column the middle one belongs to its children, not to a stack
  // of its own: each type is centred on the block of categories hanging off it.
  // Its own band is never taller than that block (the block carries the padding
  // as well), so centring cannot make two types overlap.
  const childrenOf = new Map<string, Placed[]>();
  if (fan) {
    const parent = new Map(model.links.map((link) => [link.to, link.from] as const));
    for (const leaf of leaves) {
      const key = parent.get(leaf.node.key);
      if (key === undefined) continue;
      const bucket = childrenOf.get(key);
      if (bucket) bucket.push(leaf);
      else childrenOf.set(key, [leaf]);
    }
    for (const target of targets) {
      const mine = childrenOf.get(target.node.key);
      if (!mine || mine.length === 0) continue;
      const first = mine[0] as Placed;
      const last = mine[mine.length - 1] as Placed;
      target.y = (first.y + last.y + last.h) / 2 - target.h / 2;
    }
  }

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

  // Each leaf leaves its parent's right face, in the order the model listed them
  // — which is the order they are stacked in, so again no ribbon crosses.
  const targetIndex = new Map(targets.map((t, i) => [t.node.key, i] as const));
  const faceCursor = targets.map((t) => t.y);
  const leafSlots = leaves.map((leaf) => {
    const key = [...childrenOf.entries()].find(([, kids]) => kids.includes(leaf))?.[0];
    const i = key === undefined ? undefined : targetIndex.get(key);
    if (i === undefined) return leaf.y;
    const y = faceCursor[i] as number;
    faceCursor[i] = y + leaf.h;
    return y;
  });

  const labelOf = new Map<string, string>(
    [model.hub, ...model.sources, ...model.targets, ...model.leaves].map((n) => [n.key, n.label]),
  );
  const data: ChartDatum[] = model.links.map((link) => ({
    label: (
      <span lang="de" translate="no">
        {`${labelOf.get(link.from) ?? ''} → ${labelOf.get(link.to) ?? ''}`}
      </span>
    ),
    values: [link.amountCents],
  }));

  /**
   * Room for a second line is not a property of the node but of its NEIGHBOURS:
   * two labels collide when their centres are closer than the block is tall.
   * Deciding on the node's own height alone dropped the figure from a band that
   * was a hair thinner than the one above it, which reads as missing data rather
   * than as a deliberate omission.
   */
  function sideLabel(placed: Placed, column: Placed[], i: number, side: 'left' | 'right') {
    const { node, y, h } = placed;
    const left = side === 'left';
    const centreOf = (p: Placed | undefined) => (p ? p.y + p.h / 2 : null);
    const here = y + h / 2;
    const above = centreOf(column[i - 1]);
    const below = centreOf(column[i + 1]);
    const need = narrow ? 52 : 32;
    const twoLines =
      (above === null || here - above >= need) && (below === null || below - here >= need);
    const textX = left ? x.src - LABEL_GAP : (fan ? x.leaf : x.tgt) + NODE_W + LABEL_GAP;
    const anchor = left ? 'end' : 'start';
    const centre = here + (narrow ? -6 : 0);
    const chars = left ? SRC_CHARS[size] : fan ? LEAF_CHARS[size] : TGT_CHARS[size];
    return (
      <>
        <text
          {...DATA_TEXT}
          x={textX}
          y={twoLines ? centre - 2 : centre + 4}
          textAnchor={anchor}
          className="flow__label"
        >
          {cut(node.label, chars)}
        </text>
        {twoLines && (
          <text
            x={textX}
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

  /** In the fanned-out layout the middle column has no gutter of its own, so it
      writes its name and subtotal on one line just above its band. */
  function capLabel(placed: Placed) {
    const { node, y } = placed;
    return (
      <text
        {...DATA_TEXT}
        x={x.tgt}
        y={y - 6}
        textAnchor="start"
        className="flow__label flow__label--cap"
      >
        {`${cut(node.label, TGT_CHARS[size])} · ${mask(formatEuro(node.amountCents))}`}
      </text>
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
            d={ribbon(x.src + NODE_W, s.y, x.hub, leftSlots[i] as number, s.h)}
          />
        ))}
        {targets.map((tgt, i) => (
          <path
            key={`link-${tgt.node.key}`}
            className={`flow__link ${toneClass(tgt.node.tone)}`}
            d={ribbon(x.hub + NODE_W, rightSlots[i] as number, x.tgt, tgt.y, tgt.h)}
          />
        ))}
        {leaves.map((leaf, i) => (
          <path
            key={`link-${leaf.node.key}`}
            className={`flow__link ${toneClass(leaf.node.tone)}`}
            d={ribbon(x.tgt + NODE_W, leafSlots[i] as number, x.leaf, leaf.y, leaf.h)}
          />
        ))}

        <rect
          x={x.hub}
          y={hubY}
          width={NODE_W}
          height={hubH}
          className="flow__node flow--einkommen"
        />
        <text x={x.hub + NODE_W / 2} y={TOP - 14} textAnchor="middle" className="flow__hub">
          {`${model.hub.label} · ${mask(formatEuro(model.hub.amountCents))}`}
        </text>

        {sources.map((s, i) => (
          <g key={s.node.key}>
            <title>{`${s.node.label} · ${mask(formatEuro(s.node.amountCents))}`}</title>
            <rect
              x={x.src}
              y={s.y}
              width={NODE_W}
              height={s.h}
              className={`flow__node ${toneClass(s.node.tone)}`}
            />
            {sideLabel(s, sources, i, 'left')}
          </g>
        ))}

        {targets.map((tgt, i) => {
          const code = tgt.node.typeCode;
          const clickable = Boolean(
            code && (tgt.node.expandable || tgt.node.nested) && onToggleType,
          );
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
                x={x.tgt}
                y={tgt.y}
                width={NODE_W}
                height={tgt.h}
                className={`flow__node ${toneClass(tgt.node.tone)}${
                  expanded && tgt.node.typeCode === expanded ? ' flow__node--open' : ''
                }`}
              />
              {fan ? capLabel(tgt) : sideLabel(tgt, targets, i, 'right')}
            </g>
          );
        })}

        {leaves.map((leaf, i) => (
          <g key={leaf.node.key}>
            <title>{`${leaf.node.label} · ${mask(formatEuro(leaf.node.amountCents))}`}</title>
            <rect
              x={x.leaf}
              y={leaf.y}
              width={NODE_W}
              height={leaf.h}
              className={`flow__node ${toneClass(leaf.node.tone)}`}
            />
            {sideLabel(leaf, leaves, i, 'right')}
          </g>
        ))}
      </g>
    </ChartFrame>
  );
}
