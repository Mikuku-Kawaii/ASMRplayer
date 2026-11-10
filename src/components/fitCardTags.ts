/** Largest ordered prefix whose tags and exact +N pill fit into two rows. */
export function fitCardTags(widths: number[], countWidths: number[], width: number, gap: number): number {
  const rows = [1], used = [0];
  for (const item of widths) {
    const index = used.length - 1;
    const next = used[index] ? used[index]! + gap + item : item;
    rows.push(rows[index]! + (next > width ? 1 : 0));
    used.push(next > width ? item : next);
  }
  if (rows[widths.length]! <= 2) return widths.length;
  for (let count = widths.length - 1; count >= 0; count--) {
    const indicator = countWidths[widths.length - count - 1]!;
    const next = used[count] ? used[count]! + gap + indicator : indicator;
    if (rows[count]! + (next > width ? 1 : 0) <= 2) return count;
  }
  return 0;
}
