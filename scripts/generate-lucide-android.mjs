// Official Lucide geometry shared with the desktop's installed lucide-react.
// Run after pnpm install: node scripts/generate-lucide-android.mjs [--check]
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const pkg = path.join(root, 'node_modules/lucide-react');
const version = JSON.parse(fs.readFileSync(path.join(pkg, 'package.json'), 'utf8')).version;
const mapping = {
  Library: 'library-big', Discover: 'compass', Heart: 'heart', Queue: 'list-music',
  Menu: 'menu', Search: 'search', Plus: 'plus', Play: 'play', Pause: 'pause',
  Next: 'skip-forward', Previous: 'skip-back', Back: 'chevron-left', Eye: 'eye',
  Download: 'download', Settings: 'settings', Sliders: 'sliders-horizontal', Audio: 'audio-lines', Stop: 'square',
  More: 'ellipsis', Folder: 'folder', Collection: 'bookmark', CollectionWide: 'bookmark',
  TrackBookmark: 'flag', Marker: 'map-pin', Sleep: 'moon', Sequence: 'list-end', Shuffle: 'shuffle',
  Repeat: 'repeat', RepeatOne: 'repeat-1', Check: 'check', Close: 'x', Drag: 'grip-vertical',
  Details: 'file-text', FileAudio: 'file-audio', FileImage: 'file-image', ChevronDown: 'chevron-down', GroupAdd: 'square-plus',
  Flame: 'flame', Sales: 'trending-up', NewArrival: 'sparkles', Release: 'calendar-days', Discussion: 'messages-square',
  History: 'history', ChevronRight: 'chevron-right', Star: 'star', Copy: 'copy', Grid: 'layout-grid', List: 'list',
};
const filledIcons = new Set(['heart', 'bookmark', 'flag', 'play', 'pause', 'moon', 'star']);
const resource = (name, filled = false) => `lucide_${name.replaceAll('-', '_')}${filled ? '_filled' : ''}`;
let changes = 0;
function write(relative, value) {
  const target = path.join(root, relative);
  if (fs.existsSync(target) && fs.readFileSync(target, 'utf8') === value) return;
  changes++;
  if (process.argv.includes('--check')) throw new Error(`Out of date: ${relative}`);
  fs.mkdirSync(path.dirname(target), { recursive: true });
  fs.writeFileSync(target, value);
}
function geometry(name) {
  const source = fs.readFileSync(path.join(pkg, `dist/esm/icons/${name}.js`), 'utf8');
  const literal = source.match(/createLucideIcon\("[^"]+", (\[[\s\S]*?\])\);/)[1];
  return vm.runInNewContext(`(${literal})`).map(([tag, a]) => {
    if (tag === 'path') return a.d;
    if (tag === 'line') return `M${a.x1},${a.y1} L${a.x2},${a.y2}`;
    if (tag === 'polyline' || tag === 'polygon') {
      const coordinates = a.points.trim().split(/[\s,]+/);
      if (coordinates.length % 2) throw new Error(`Invalid points in ${name}`);
      const pairs = [];
      for (let i = 0; i < coordinates.length; i += 2) pairs.push(`${coordinates[i]},${coordinates[i+1]}`);
      return `M${pairs.join(' L')}${tag === 'polygon' ? ' Z' : ''}`;
    }
    if (tag === 'circle' || tag === 'ellipse') {
      const x = +a.cx, y = +a.cy, rx = +(a.r ?? a.rx), ry = +(a.r ?? a.ry);
      return `M${x-rx},${y} A${rx},${ry} 0 1,0 ${x+rx},${y} A${rx},${ry} 0 1,0 ${x-rx},${y} Z`;
    }
    if (tag === 'rect') {
      const x=+a.x, y=+a.y, w=+a.width, h=+a.height, r=+(a.rx ?? 0);
      if (!r) return `M${x},${y} h${w} v${h} h${-w} Z`;
      return `M${x+r},${y} H${x+w-r} A${r},${r} 0 0,1 ${x+w},${y+r} V${y+h-r} A${r},${r} 0 0,1 ${x+w-r},${y+h} H${x+r} A${r},${r} 0 0,1 ${x},${y+h-r} V${y+r} A${r},${r} 0 0,1 ${x+r},${y} Z`;
    }
    throw new Error(`Unsupported Lucide primitive ${tag} in ${name}`);
  });
}
const banner = `Generated from lucide-react ${version}; ISC. See licenses/Lucide-LICENSE.txt. Do not hand-edit.`;
function vector(name, filled, size=24) {
  return `<!-- ${banner} -->\n<vector xmlns:android="http://schemas.android.com/apk/res/android" android:width="${size}dp" android:height="${size}dp" android:viewportWidth="24" android:viewportHeight="24">\n` + geometry(name).map(d =>
    `    <path android:pathData="${d}" android:fillColor="${filled ? '#FFFFFFFF' : '#00000000'}" android:strokeColor="#FFFFFFFF" android:strokeWidth="${filled && name !== 'flag' ? 0 : 1.75}" android:strokeLineCap="round" android:strokeLineJoin="round"/>\n`).join('') + '</vector>\n';
}
for (const name of new Set(Object.values(mapping))) {
  write(`android/app/src/main/res/drawable/${resource(name)}.xml`, vector(name, false));
  if (filledIcons.has(name)) write(`android/app/src/main/res/drawable/${resource(name, true)}.xml`, vector(name, true));
}
const widgets = {
  widget_previous: ['skip-back', false, 24], widget_next: ['skip-forward', false, 24],
  widget_play: ['play', true, 28], widget_pause: ['pause', true, 28],
  widget_heart: ['flag', false, 24], widget_heart_filled: ['flag', true, 24],
  widget_library_add: ['bookmark', false, 24], widget_library_saved: ['bookmark', true, 24],
  widget_configure: ['sliders-horizontal', false, 24], widget_artwork: ['audio-lines', false, 40],
  ic_media_notification: ['audio-lines', false, 24],
  // Media3 uses these resources for app-rendered notification transport actions.
  media3_icon_play: ['play', true, 24], media3_icon_pause: ['pause', true, 24],
  media3_icon_next: ['skip-forward', false, 24], media3_icon_previous: ['skip-back', false, 24],
  media3_icon_stop: ['square', true, 24],
  media3_icon_repeat_all: ['repeat', false, 24], media3_icon_repeat_one: ['repeat-1', false, 24],
  media3_icon_shuffle_on: ['shuffle', false, 24],
  media3_icon_shuffle_off: ['list-end', false, 24], media3_icon_repeat_off: ['list-end', false, 24],
  media3_icon_heart_filled: ['heart', true, 24], media3_icon_heart_unfilled: ['heart', false, 24],
  media3_icon_bookmark_filled: ['bookmark', true, 24], media3_icon_bookmark_unfilled: ['bookmark', false, 24],
};
for (const [name, [icon, filled, size]] of Object.entries(widgets))
  // Match Media3's qualifier: a default drawable would lose to its anydpi-v21 asset.
  write(`android/app/src/main/res/${name.startsWith('media3_') ? 'drawable-anydpi-v21' : 'drawable'}/${name}.xml`, vector(icon, filled, size));
write('android/app/src/main/kotlin/com/asmrplayer/android/ui/LucideGlyphs.kt',
  `// ${banner}\npackage com.asmrplayer.android.ui\n\nimport androidx.annotation.DrawableRes\nimport com.asmrplayer.android.R\n\nenum class Glyph { ${Object.keys(mapping).join(', ')} }\n\n@DrawableRes\ninternal fun Glyph.lucideResource(filled: Boolean): Int = when (this) {\n` + Object.entries(mapping).map(([kind, name]) =>
    `    Glyph.${kind} -> ${filledIcons.has(name) ? `if (filled) R.drawable.${resource(name, true)} else ` : ''}R.drawable.${resource(name)}\n`).join('') + '}\n');
write('licenses/Lucide-LICENSE.txt', fs.readFileSync(path.join(pkg, 'LICENSE'), 'utf8'));
write('android/app/src/main/assets/licenses/Lucide-LICENSE.txt', fs.readFileSync(path.join(pkg, 'LICENSE'), 'utf8'));
console.log(`Lucide ${version}: ${Object.keys(mapping).length} glyph roles; ${changes} generated files ${process.argv.includes('--check') ? 'out of date' : 'updated'}.`);
