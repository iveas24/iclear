// Original soundtrack for the iclear story, synthesized from scratch (no
// samples, no dependencies) and synced to timeline.js: one chord per scene,
// kick on every beat (every cut lands on one), and sound effects on the key
// moments of each scene.
//
//   node promo/audio.mjs   → promo/music.wav (44.1 kHz, stereo)
import { writeFileSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadTimeline } from './timeline.mjs';

const tl = loadTimeline();
const { scenes, beat: BEAT } = tl;
const S = id => tl.at(id).start;
const SR = 44100, DUR = tl.duration, N = Math.round(SR * DUR);
const L = new Float32Array(N), R = new Float32Array(N);
const SEND = new Float32Array(N); // reverb bus (mono in)
const GROOVE_START = S('intro'), GROOVE_END = S('end');

let seed = 1234567; // deterministic noise
const noise = () => { seed = (seed * 1103515245 + 12345) & 0x7fffffff; return seed / 0x3fffffff - 1; };
const mtof = m => 440 * Math.pow(2, (m - 69) / 12);
const clamp = (v, a, b) => Math.min(b, Math.max(a, v));
const s2i = s => Math.round(s * SR);

function put(i, v, pan = 0, send = 0) {
  if (i < 0 || i >= N) return;
  L[i] += v * Math.cos((pan + 1) * Math.PI / 4);
  R[i] += v * Math.sin((pan + 1) * Math.PI / 4);
  SEND[i] += v * send;
}
// Sidechain feel: pad and bass dip on every kick.
function duck(t) {
  if (t < GROOVE_START || t >= GROOVE_END) return 1;
  return 1 - 0.55 * Math.exp(-((t - GROOVE_START) % BEAT) / 0.11);
}

// ---------- pad: detuned saws through a moving low-pass ----------
scenes.forEach((sc, si) => {
  const first = si === 0, last = si === scenes.length - 1;
  const a = s2i(sc.start), b = Math.min(N, s2i(sc.end + 0.5));
  sc.chord.forEach((m, vi) => {
    for (const det of [-0.11, 0, 0.12]) {
      const f = mtof(m + det);
      let ph = (vi * 0.37 + det + 1) % 1, lp1 = 0, lp2 = 0;
      const pan = ((vi % 3) - 1) * 0.5 + det;
      for (let i = a; i < b; i++) {
        const t = i / SR, lt = t - sc.start;
        ph += f / SR; if (ph >= 1) ph -= 1;
        let cutoff = first ? 300 + 1700 * clamp(lt / sc.dur, 0, 1) ** 2 : last ? 2600 : 1500;
        cutoff *= 1 + 0.25 * Math.sin(t * 1.3 + vi);
        const k = 1 - Math.exp(-2 * Math.PI * cutoff / SR);
        lp1 += k * (2 * ph - 1 - lp1); lp2 += k * (lp1 - lp2);
        const att = clamp(lt / (first ? 1.2 : 0.25), 0, 1);
        const rel = t > sc.end ? Math.max(0, 1 - (t - sc.end) / 0.5) : 1;
        put(i, lp2 * 0.022 * att * rel * duck(t), pan, 0.55);
      }
    }
  });
});

// ---------- drums ----------
function kick(t0, gain = 1, len = 0.35) {
  const a = s2i(t0), n = s2i(len);
  let ph = 0;
  for (let j = 0; j < n; j++) {
    const t = j / SR;
    ph += (48 + 110 * Math.exp(-t / 0.035)) / SR;
    const click = j < 90 ? noise() * 0.25 * (1 - j / 90) : 0;
    put(a + j, (Math.sin(2 * Math.PI * ph) * Math.exp(-t / (len * 0.35)) + click) * 0.55 * gain, 0, 0.03);
  }
}
function hat(t0, gain, open = false) {
  const a = s2i(t0), n = s2i(open ? 0.18 : 0.05);
  let hp = 0, prev = 0;
  for (let j = 0; j < n; j++) {
    const x = noise();
    hp = 0.92 * (hp + x - prev); prev = x;
    put(a + j, hp * Math.exp(-j / SR / (open ? 0.06 : 0.015)) * 0.12 * gain, 0.25, 0.05);
  }
}
function clap(t0, gain = 1) {
  const a = s2i(t0);
  for (const off of [0, 0.011, 0.022]) {
    const b = a + s2i(off), n = s2i(0.12);
    let bp = 0, lp = 0;
    for (let j = 0; j < n; j++) {
      const x = noise();
      lp += 0.35 * (x - lp); bp = x - lp;
      put(b + j, bp * Math.exp(-j / SR / 0.035) * 0.16 * gain, -0.1, 0.25);
    }
  }
}
const beats = [];
for (let t = GROOVE_START; t < GROOVE_END - 0.01; t += BEAT) beats.push(+t.toFixed(4));
beats.forEach((t, k) => {
  kick(t, t < S('memory') ? 0.8 : 1);
  if (t >= S('memory') && k % 2 === 1) clap(t, 0.9);
  if (t >= S('recs')) {
    hat(t + BEAT / 2, 1, k % 4 === 3);
    if (t >= S('cleanup')) { hat(t + BEAT / 4, 0.45); hat(t + 3 * BEAT / 4, 0.45); }
  }
});

// ---------- bass: off-beat sub notes, busier later ----------
for (const sc of scenes.slice(1, -1)) {
  for (const t of beats.filter(t => t >= sc.start && t < sc.end)) {
    const offs = sc.start >= S('apps') ? [BEAT / 2, BEAT * 0.75] : [BEAT / 2];
    for (const off of offs) {
      const a = s2i(t + off), n = s2i(0.26), f = mtof(sc.bass + 12);
      let ph = 0;
      for (let j = 0; j < n; j++) {
        ph += f / SR;
        const env = Math.min(1, j / 200) * Math.exp(-j / SR / 0.16);
        put(a + j, (Math.sin(2 * Math.PI * ph) + 0.25 * Math.sin(4 * Math.PI * ph)) * env * 0.2, 0, 0);
      }
    }
  }
}

// ---------- plucks: 16th arpeggio with a dotted-8th echo ----------
function pluck(t0, m, gain, pan) {
  const a = s2i(t0), n = s2i(0.35), f = mtof(m);
  let ph = 0, mph = 0;
  for (let j = 0; j < n; j++) {
    const t = j / SR;
    ph += f / SR; mph += 2 * f / SR;
    const v = Math.sin(2 * Math.PI * ph + 2.2 * Math.exp(-t / 0.05) * Math.sin(2 * Math.PI * mph));
    put(a + j, v * Math.exp(-t / 0.09) * Math.min(1, j / 40) * gain, pan, 0.35);
  }
}
const PATTERN = [0, 2, 4, 1, 3, 4, 2, 4];
for (const sc of scenes.filter(s => s.start >= S('recs') && s.start < S('end'))) {
  const notes = sc.chord.map(m => m + 12);
  let k = 0;
  for (let t = sc.start; t < sc.end - 0.05; t += BEAT / 4, k++) {
    const m = notes[PATTERN[k % PATTERN.length] % notes.length];
    const accent = k % 4 === 0 ? 1 : 0.6;
    pluck(t, m, 0.065 * accent, -0.3);
    pluck(t + BEAT * 0.75, m, 0.028 * accent, 0.4);
  }
}

// ---------- sound design ----------
function sweep(t0, len, from, to, gain, pan = 0, rising = true) {
  const a = s2i(t0), n = s2i(len);
  let low = 0, band = 0;
  for (let j = 0; j < n; j++) {
    const k = j / n, f = from * Math.pow(to / from, k);
    const F = 2 * Math.sin(Math.PI * Math.min(f, SR / 6) / SR);
    const high = noise() - low - 0.35 * band;
    band += F * high; low += F * band;
    const env = rising ? Math.pow(k, 2.2) * (k > 0.97 ? (1 - k) / 0.03 : 1) : Math.sin(Math.PI * k) ** 1.5;
    put(a + j, band * env * gain, pan, 0.4);
  }
}
function tone(t0, len, m, gain, { type = 'sine', decay = 0.2, pan = 0, send = 0.3, glide = 0 } = {}) {
  const a = s2i(t0), n = s2i(len);
  let ph = 0;
  for (let j = 0; j < n; j++) {
    const t = j / SR;
    ph += mtof(m + glide * (t / len)) / SR;
    const raw = type === 'square' ? Math.sign(Math.sin(2 * Math.PI * ph)) * 0.5 : Math.sin(2 * Math.PI * ph);
    put(a + j, raw * Math.min(1, j / 60) * Math.exp(-t / decay) * gain, pan, send);
  }
}
const pop = (t, m, pan = 0) => tone(t, 0.08, m, 0.055, { decay: 0.03, pan });
const chime = t => { tone(t, 0.9, 88, 0.07, { decay: 0.35, pan: -0.2, send: 0.5 }); tone(t + 0.12, 1.0, 95, 0.06, { decay: 0.4, pan: 0.2, send: 0.5 }); };
const click = t => tone(t, 0.04, 96, 0.08, { decay: 0.01, send: 0.1 });
const ticks = (t0, t1) => { for (let t = t0; t < t1; t += 0.06) tone(t, 0.02, 100, 0.02 * (1 - (t - t0) / (t1 - t0)), { decay: 0.006, send: 0 }); };

// hook: riser into the drop + warning blips when demand overflows the RAM bar
sweep(1.2, S('intro') - 1.2, 250, 7000, 0.22);
tone(1.2, S('intro') - 1.2, 45, 0.05, { decay: 9, glide: 12, send: 0.2 });
tone(2.05, 0.09, 81, 0.05, { type: 'square', decay: 0.05, pan: 0.2 });
tone(2.2, 0.09, 81, 0.05, { type: 'square', decay: 0.05, pan: 0.2 });
kick(S('intro'), 1.4, 0.8); // the drop

// whoosh into every cut after the intro
for (const sc of scenes.slice(2, -1)) sweep(sc.start - 0.45, 0.6, 5000, 500, 0.15, 0, false);

// intro: window flies in, screens slide
sweep(S('intro') + 0.05, 0.6, 400, 3000, 0.12, 0, false);
[0.45 + 1.45, 0.45 + 2.9].forEach(d => sweep(S('intro') + d - 0.1, 0.35, 3000, 900, 0.09, 0.3, false));
for (let i = 0; i < 6; i++) pop(S('intro') + 0.7 + i * 0.08, 84 + i, -0.4 + i * 0.16);
// memory: score dropping, app rows
ticks(S('memory') + 0.3, S('memory') + 1.6);
for (let i = 0; i < 5; i++) pop(S('memory') + 0.6 + i * 0.13, 79 + i * 2, -0.3 + i * 0.15);
// recs: cards, button press, success
[0.35, 0.65, 0.95].forEach((d, i) => pop(S('recs') + d, 84 + i * 2, -0.2 + i * 0.2));
click(S('recs') + 2.0); chime(S('recs') + 2.3);
// apps: big counter, uninstall
ticks(S('apps') + 0.2, S('apps') + 1.3);
chime(S('apps') + 2.1);
// cleanup: four checks, button, done
for (let i = 0; i < 4; i++) pop(S('cleanup') + 0.8 + i * 0.2, 86 + i * 2, -0.3 + i * 0.2);
click(S('cleanup') + 2.2); sweep(S('cleanup') + 2.5, 0.9, 600, 4000, 0.08); chime(S('cleanup') + 3.4);
// control: tiles
for (let i = 0; i < 4; i++) pop(S('control') + 0.25 + i * 0.1, 86 + i, -0.3 + i * 0.2);
// optimize: each action completes with a rising note
for (let i = 0; i < 4; i++) tone(S('optimize') + 1.4 + i * 0.5, 0.25, 84 + i * 3, 0.06, { decay: 0.1, pan: -0.2 + i * 0.13, send: 0.4 });
// trust: shield lands
tone(S('trust') + 0.15, 1.2, 64, 0.06, { decay: 0.5, send: 0.6 });
for (let i = 0; i < 3; i++) pop(S('trust') + 0.9 + i * 0.18, 88 + i * 2, -0.2 + i * 0.2);
// end: riser, impact, shimmer
sweep(S('end') - 1.1, 1.1, 300, 8000, 0.2);
kick(S('end'), 1.6, 1.2);
sweep(S('end'), 1.4, 3000, 120, 0.25, 0, false);
[79, 83, 86, 91].forEach((m, i) => tone(S('end') + 0.3 + i * 0.12, 1.8, m, 0.035, { decay: 0.9, pan: -0.45 + i * 0.3, send: 0.8 }));

// ---------- reverb (Schroeder: 4 damped combs + 2 allpasses) ----------
function reverb(input, spread) {
  const out = new Float32Array(N);
  for (const [d0, fb] of [[1557, 0.84], [1617, 0.83], [1491, 0.85], [1422, 0.84]]) {
    const d = d0 + spread, buf = new Float32Array(d);
    let idx = 0, damp = 0;
    for (let i = 0; i < N; i++) {
      const y = buf[idx];
      damp = y * 0.6 + damp * 0.4;
      buf[idx] = input[i] + damp * fb;
      out[i] += y * 0.25;
      idx = (idx + 1) % d;
    }
  }
  for (const d0 of [556, 225]) {
    const d = d0 + spread, buf = new Float32Array(d);
    let idx = 0;
    for (let i = 0; i < N; i++) {
      const bo = buf[idx], x = out[i];
      out[i] = -x + bo;
      buf[idx] = x + bo * 0.5;
      idx = (idx + 1) % d;
    }
  }
  return out;
}
const revL = reverb(SEND, 0), revR = reverb(SEND, 23);
for (let i = 0; i < N; i++) { L[i] += revL[i] * 0.35; R[i] += revR[i] * 0.35; }

// ---------- master ----------
let peak = 0;
for (let i = 0; i < N; i++) {
  const t = i / SR;
  const fade = Math.min(1, t / 0.05) * Math.min(1, (DUR - t) / 0.9);
  L[i] = Math.tanh(L[i] * 1.3) * fade;
  R[i] = Math.tanh(R[i] * 1.3) * fade;
  peak = Math.max(peak, Math.abs(L[i]), Math.abs(R[i]));
}
const norm = 0.89 / peak;
const data = Buffer.alloc(N * 4);
for (let i = 0; i < N; i++) {
  data.writeInt16LE(Math.round(clamp(L[i] * norm, -1, 1) * 32767), i * 4);
  data.writeInt16LE(Math.round(clamp(R[i] * norm, -1, 1) * 32767), i * 4 + 2);
}
const h = Buffer.alloc(44);
h.write('RIFF', 0); h.writeUInt32LE(36 + data.length, 4); h.write('WAVE', 8);
h.write('fmt ', 12); h.writeUInt32LE(16, 16); h.writeUInt16LE(1, 20); h.writeUInt16LE(2, 22);
h.writeUInt32LE(SR, 24); h.writeUInt32LE(SR * 4, 28); h.writeUInt16LE(4, 32); h.writeUInt16LE(16, 34);
h.write('data', 36); h.writeUInt32LE(data.length, 40);
const out = join(dirname(fileURLToPath(import.meta.url)), 'music.wav');
writeFileSync(out, Buffer.concat([h, data]));
console.log(out);
