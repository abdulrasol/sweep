/** A box in the map, in pixels. */
export interface Rect { x: number; y: number; w: number; h: number }

/** Worst aspect ratio of a row of areas laid along a side of length `side`. */
function worst(sum: number, max: number, min: number, side: number): number {
  const s2 = side * side;
  return Math.max((s2 * max) / (sum * sum), (sum * sum) / (s2 * min));
}

/**
 * Squarified treemap (Bruls, Huizing, van Wijk). `sizes` must be positive and
 * sorted largest first. Returns one rect per size, in the same order, that
 * together fill `box`.
 */
export function squarify(sizes: number[], box: Rect): Rect[] {
  const total = sizes.reduce((a, b) => a + b, 0);
  if (sizes.length === 0) return [];
  if (total <= 0 || box.w <= 0 || box.h <= 0) {
    return sizes.map(() => ({ x: box.x, y: box.y, w: 0, h: 0 }));
  }

  const scale = (box.w * box.h) / total;
  const areas = sizes.map(s => s * scale);
  const out: Rect[] = [];
  let { x, y, w, h } = box;
  let i = 0;

  while (i < areas.length) {
    const side = Math.min(w, h);
    let end = i + 1;
    let sum = areas[i];
    let max = areas[i];
    let min = areas[i];
    let current = worst(sum, max, min, side);

    while (end < areas.length) {
      const a = areas[end];
      const next = worst(sum + a, Math.max(max, a), Math.min(min, a), side);
      if (next > current) break;
      sum += a;
      max = Math.max(max, a);
      min = Math.min(min, a);
      current = next;
      end++;
    }

    if (w >= h) {
      // The row is a column on the left, items stacked top to bottom.
      const colW = sum / h;
      let cy = y;
      for (let k = i; k < end; k++) {
        const itemH = areas[k] / colW;
        out.push({ x, y: cy, w: colW, h: itemH });
        cy += itemH;
      }
      x += colW;
      w -= colW;
    } else {
      // The row is a band on top, items side by side.
      const rowH = sum / w;
      let cx = x;
      for (let k = i; k < end; k++) {
        const itemW = areas[k] / rowH;
        out.push({ x: cx, y, w: itemW, h: rowH });
        cx += itemW;
      }
      y += rowH;
      h -= rowH;
    }
    i = end;
  }
  return out;
}
