// Reads timeline.js (written for the browser) from Node and adds start times.
import { readFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

export function loadTimeline() {
  const src = readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'timeline.js'), 'utf8');
  const tl = JSON.parse(src.slice(src.indexOf('{'), src.lastIndexOf('}') + 1));
  let t = 0;
  for (const s of tl.scenes) {
    s.start = +t.toFixed(4);
    t += s.dur;
    s.end = +t.toFixed(4);
  }
  tl.duration = +t.toFixed(4);
  tl.at = id => tl.scenes.find(s => s.id === id);
  return tl;
}
