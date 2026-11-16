// Voice-over for the iclear story with macOS `say`: one line per scene, taken
// from timeline.js. Each line uses the slowest rate that fits its scene.
//
//   node promo/voice.mjs                  → promo/voice.wav, "trailer" style (default)
//   STYLE=social node promo/voice.mjs     → female TikTok/Reels-style voice
//   STYLE=natural node promo/voice.mjs    → calm narrator, no pitch processing
//   VOICE="Reed (Español (México))" node promo/voice.mjs   (any voice from `say -v '?'`)
//
// "trailer": deep male movie-trailer narrator. A macOS Spanish male voice
// pitched down 3 semitones, chest EQ, broadcast compression and a short
// multi-tap room so it sounds big and enveloping.
// "social": the typical short-video TTS sound (pitched up, faster, bright).
// Both are processed macOS voices, not any platform's own voice.
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { loadTimeline } from './timeline.mjs';

const here = dirname(fileURLToPath(import.meta.url));
const STYLE = process.env.STYLE || 'trailer';
const VOICE = process.env.VOICE || (STYLE === 'trailer' ? 'Eddy (Español (México))' : 'Paulina');
const { scenes, duration: DUR } = loadTimeline();

// Speech starts shortly after each cut and must end before the next one.
const LEAD_IN = 0.3, TAIL = 0.15;

// Pitch shift without changing speed: change the sample rate (pitch and tempo
// move together), then bring tempo back with atempo (times the wanted tempo).
const trim = 'silenceremove=start_periods=1:start_threshold=-50dB'; // `say` pads with silence
const shift = (semitones, tempo) => {
  const p = Math.pow(2, semitones / 12);
  return `asetrate=${Math.round(22050 * p)},aresample=44100,atempo=${(tempo / p).toFixed(4)}`;
};
const LINE_FX = {
  trailer: `${trim},areverse,${trim},areverse,${shift(-3, 0.97)}`,
  social: `${trim},areverse,${trim},areverse,${shift(2.5, 1.06)}`,
  natural: `${trim},areverse,${trim},areverse,aresample=44100`,
};
const BUS_FX = {
  // Chest (110 Hz) and presence (3 kHz) up, mud down, heavy broadcast
  // compression, early reflections, then a Haas widener (one side a few ms
  // later) so the voice fills the stereo field.
  trailer:
    'highpass=f=55,equalizer=f=110:t=q:w=1:g=5,equalizer=f=380:t=q:w=1.2:g=-3,equalizer=f=3000:t=q:w=1.5:g=3,' +
    'equalizer=f=7500:t=q:w=1:g=-2,acompressor=threshold=-26dB:ratio=6:attack=4:release=150:makeup=8dB,' +
    'aecho=0.85:0.55:37|71|113:0.28|0.2|0.12,haas=level_in=1:side_gain=0.6:left_delay=2.05:right_delay=11.5:left_balance=-0.6:right_balance=0.6,alimiter=limit=0.9',
  social:
    'highpass=f=140,equalizer=f=300:t=q:w=1:g=-3,equalizer=f=2800:t=q:w=1.4:g=4,' +
    'equalizer=f=9000:t=q:w=1:g=3,acompressor=threshold=-24dB:ratio=5:attack=3:release=80:makeup=7dB,alimiter=limit=0.9',
  natural:
    'highpass=f=90,equalizer=f=3200:t=q:w=1.2:g=3,equalizer=f=250:t=q:w=1:g=-2,' +
    'acompressor=threshold=-20dB:ratio=3:attack=5:release=120:makeup=4dB',
};

const tmp = mkdtempSync(join(tmpdir(), 'iclear-voice-'));
const duration = f => parseFloat(execFileSync('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', f]).toString());

const clips = scenes.map((sc, i) => {
  const start = sc.start + LEAD_IN, max = sc.dur - LEAD_IN - TAIL;
  let file, len, rate;
  for (rate = 175; rate <= 200; rate += 5) {
    const raw = join(tmp, `line${i}.aiff`);
    file = join(tmp, `line${i}.wav`);
    execFileSync('say', ['-v', VOICE, '-r', String(rate), '-o', raw, sc.vo]);
    execFileSync('ffmpeg', ['-y', '-loglevel', 'error', '-i', raw, '-af', LINE_FX[STYLE], file]);
    len = duration(file);
    if (len <= max) break;
  }
  const fits = len <= max;
  console.log(`${fits ? '✓' : '✗'} ${sc.id.padEnd(9)} ${start.toFixed(2)}s  ${len.toFixed(2)}/${max.toFixed(2)}s  r=${rate}  ${sc.vo}`);
  if (!fits) throw new Error(`La frase de "${sc.id}" no cabe en su escena; acórtala en timeline.js.`);
  return { file, start };
});

const inputs = clips.flatMap(c => ['-i', c.file]);
const chains = clips.map((c, i) => `[${i}:a]pan=stereo|c0=c0|c1=c0,adelay=${Math.round(c.start * 1000)}:all=1[v${i}]`);
const mix = `${clips.map((_, i) => `[v${i}]`).join('')}amix=inputs=${clips.length}:normalize=0,${BUS_FX[STYLE]},apad=whole_dur=${DUR},atrim=0:${DUR}[out]`;
const out = join(here, 'voice.wav');
execFileSync('ffmpeg', ['-y', '-loglevel', 'error', ...inputs, '-filter_complex', [...chains, mix].join(';'), '-map', '[out]', out]);
rmSync(tmp, { recursive: true, force: true });
console.log(out);
