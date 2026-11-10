type Color = { r: number; g: number; b: number };
type Swatch = Color & { count: number; weight: number; border: number };

const distance = (a: Color, b: Color) => Math.hypot(a.r - b.r, a.g - b.g, a.b - b.b);

/** Small cover-only raster: never sample the glass window or the desktop behind it. */
export function coverAccent(data: Uint8ClampedArray, width: number, height: number): string | undefined {
  if (width < 1 || height < 1 || data.length !== width * height * 4) return;
  const swatches = new Map<number, Swatch>();
  let borderCount = 0;
  for (let y = 0; y < height; y++) for (let x = 0; x < width; x++) {
    const i = (y * width + x) * 4;
    if (data[i + 3]! < 224) continue;
    const r = data[i]!, g = data[i + 1]!, b = data[i + 2]!;
    const key = (r >> 5) * 64 + (g >> 5) * 8 + (b >> 5);
    const border = x < width * .12 || x >= width * .88 || y < height * .12 || y >= height * .88;
    const center = 1 - Math.min(1, Math.hypot((x + .5) / width - .5, (y + .5) / height - .5) / .71);
    const s = swatches.get(key) ?? { r: 0, g: 0, b: 0, count: 0, weight: 0, border: 0 };
    s.r += r; s.g += g; s.b += b; s.count++; s.weight += .3 + center * .7;
    if (border) { s.border++; borderCount++; }
    swatches.set(key, s);
  }
  const colors = [...swatches.values()].map(s => ({ ...s, r: s.r / s.count, g: s.g / s.count, b: s.b / s.count }));
  // Group nearby border shades so gradients/JPEG noise do not evade the background penalty.
  const backgrounds = colors.filter(s => s.border > 0 && colors.reduce((n, c) =>
    n + (distance(s, c) < 60 ? c.border : 0), 0) >= borderCount * .35);
  let best: { score: number; h: number; s: number; l: number } | undefined;
  for (const c of colors) {
    const r = c.r / 255, g = c.g / 255, b = c.b / 255;
    const max = Math.max(r, g, b), min = Math.min(r, g, b), delta = max - min;
    const l = (max + min) / 2;
    if (delta < .1 || l < .08 || l > .92 || c.count < width * height * .008) continue;
    if (backgrounds.some(bg => distance(c, bg) < 65)) continue;
    const saturation = delta / (1 - Math.abs(2 * l - 1));
    const hue = ((max === r ? (g - b) / delta : max === g ? (b - r) / delta + 2 : (r - g) / delta + 4) * 60 + 360) % 360;
    const score = c.weight * (.35 + saturation);
    if (!best || score > best.score) best = { score, h: hue, s: saturation, l };
  }
  if (!best) return;
  // Retain the cover hue, with a readable, subdued tone on the dark island.
  return `hsl(${Math.round(best.h)} ${Math.round(Math.min(.72, Math.max(.32, best.s)) * 100)}% ${Math.round(Math.min(.76, Math.max(.64, best.l)) * 100)}%)`;
}

export function accentFromImage(image: HTMLImageElement): string | undefined {
  const canvas = document.createElement("canvas");
  canvas.width = 48; canvas.height = 48;
  const context = canvas.getContext("2d", { willReadFrequently: true });
  if (!context) return;
  context.drawImage(image, 0, 0, 48, 48);
  return coverAccent(context.getImageData(0, 0, 48, 48).data, 48, 48);
}
