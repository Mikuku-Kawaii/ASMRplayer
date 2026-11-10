import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const read = name => fs.readFileSync(path.join(root, `android/app/src/main/res/${name.startsWith('media3_') ? 'drawable-anydpi-v21' : 'drawable'}/${name}.xml`), 'utf8');

test('committed Android glyphs match the installed desktop Lucide version', () => {
  execFileSync(process.execPath, ['scripts/generate-lucide-android.mjs', '--check'], { cwd: root });
});

test('play and skip triangles retain visible nonzero area after SVG point conversion', () => {
  for (const name of ['lucide_play', 'lucide_play_filled', 'lucide_skip_forward', 'lucide_skip_back', 'widget_play', 'media3_icon_play']) {
    const data = read(name).match(/android:pathData="([^"]+)"/)[1];
    const points = [...data.matchAll(/[ML](-?[\d.]+),(-?[\d.]+)/g)].map(m => [+m[1], +m[2]]);
    assert.ok(points.length >= 3, `${name}: malformed coordinate pairs`);
    let area = 0;
    points.forEach(([x, y], i) => { const next = points[(i + 1) % points.length]; area += x * next[1] - next[0] * y; });
    assert.ok(Math.abs(area / 2) >= 70, `${name}: invisible triangle`);
  }
});

test('collection and heart selection keep identical geometry and change fill only', () => {
  for (const name of ['bookmark', 'heart', 'flag']) {
    const outline = read(`lucide_${name}`), filled = read(`lucide_${name}_filled`);
    assert.deepEqual([...outline.matchAll(/android:pathData="([^"]+)"/g)].map(m => m[1]),
      [...filled.matchAll(/android:pathData="([^"]+)"/g)].map(m => m[1]));
    assert.match(outline, /fillColor="#00000000"/);
    assert.match(filled, /fillColor="#FFFFFFFF"/);
  }
});
