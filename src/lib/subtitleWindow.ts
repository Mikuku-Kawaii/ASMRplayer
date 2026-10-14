/** Keep the explicitly expanded prefix and a small context around a distant active cue. */
export function subtitleWindows(count: number, prefix: number, activeIndex: number): { start: number; end: number }[] {
  if (count <= 0) return [];
  const end = Math.min(count, Math.max(0, prefix));
  const ranges = end > 0 ? [{ start: 0, end }] : [];
  if (activeIndex >= end && activeIndex < count) {
    const start = Math.max(end, activeIndex - 25);
    const contextEnd = Math.min(count, activeIndex + 50);
    if (start === end && ranges[0]) ranges[0].end = contextEnd;
    else ranges.push({ start, end: contextEnd });
  }
  return ranges;
}
