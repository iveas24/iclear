// Renders promo/story.html frame by frame with headless Chrome (DevTools
// protocol over Node's built-in WebSocket, no dependencies) and pipes the PNGs
// into ffmpeg.
//
//   node promo/audio.mjs && node promo/voice.mjs   (soundtrack + voice-over)
//   node promo/render.mjs                 → promo/iclear-story.mp4
//   node promo/render.mjs --stills 2,6,9  → only PNG stills at those seconds
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, rmSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const FPS = 30;
const CHROME = '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const PORT = 9300 + Math.floor(Math.random() * 500);
const stillsArg = process.argv.indexOf('--stills');
const stills = stillsArg > 0 ? process.argv[stillsArg + 1].split(',').map(Number) : null;

// Music (audio.mjs) ducked under the voice-over (voice.mjs), normalized to
// -14 LUFS like Instagram. Falls back to silence if they weren't generated.
function audioArgs() {
  const music = join(here, 'music.wav'), voice = join(here, 'voice.wav');
  if (!existsSync(music) || !existsSync(voice)) {
    return ['-f', 'lavfi', '-i', 'anullsrc=channel_layout=stereo:sample_rate=44100', '-shortest', '-map', '0:v', '-map', '1:a'];
  }
  const graph = [
    '[2:a]asplit=2[vo][key]',
    '[1:a]volume=0.8[mu]',
    '[mu][key]sidechaincompress=threshold=0.02:ratio=6:attack=30:release=350[ducked]',
    '[ducked][vo]amix=inputs=2:normalize=0,loudnorm=I=-14:TP=-1.5:LRA=11,aresample=44100[a]',
  ].join(';');
  return ['-i', music, '-i', voice, '-filter_complex', graph, '-map', '0:v', '-map', '[a]', '-shortest'];
}

const profile = mkdtempSync(join(tmpdir(), 'iclear-promo-'));
const chrome = spawn(CHROME, [
  '--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  '--window-size=1080,1920', '--hide-scrollbars', '--force-color-profile=srgb',
  pathToFileURL(join(here, 'story.html')).href + '?render',
], { stdio: 'ignore' });

const sleep = ms => new Promise(r => setTimeout(r, ms));
let target;
for (let i = 0; i < 50 && !target; i++) {
  await sleep(200);
  try {
    const list = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
    target = list.find(t => t.type === 'page' && t.url.includes('story.html'));
  } catch {}
}
if (!target) throw new Error('Chrome no respondió');

const ws = new WebSocket(target.webSocketDebuggerUrl);
await new Promise(r => ws.addEventListener('open', r, { once: true }));
let id = 0;
const pending = new Map();
ws.addEventListener('message', e => {
  const m = JSON.parse(e.data);
  if (m.id && pending.has(m.id)) { pending.get(m.id)(m); pending.delete(m.id); }
});
const send = (method, params = {}) => new Promise(r => { const i = ++id; pending.set(i, r); ws.send(JSON.stringify({ id: i, method, params })); });
const evaluate = async expr => {
  const m = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (m.result?.exceptionDetails) throw new Error(JSON.stringify(m.result.exceptionDetails));
  return m.result.result.value;
};

await evaluate('Promise.all([document.fonts.ready, window.READY]).then(() => true)');
const duration = await evaluate('window.DURATION');
const frame = t => evaluate(`renderFrame(${t}); document.getElementById('c').toDataURL('image/png').slice(22)`);

if (stills) {
  for (const s of stills) {
    const out = join(here, `still-${s}s.png`);
    writeFileSync(out, Buffer.from(await frame(s), 'base64'));
    console.log(out);
  }
} else {
  const out = join(here, 'iclear-story.mp4');
  const ff = spawn('ffmpeg', [
    '-y', '-loglevel', 'error', '-f', 'image2pipe', '-framerate', String(FPS), '-i', '-',
    ...audioArgs(),
    '-c:v', 'libx264', '-preset', 'slow', '-crf', '17', '-pix_fmt', 'yuv420p', '-profile:v', 'high',
    '-c:a', 'aac', '-b:a', '128k', '-movflags', '+faststart', out,
  ], { stdio: ['pipe', 'inherit', 'inherit'] });
  const total = Math.round(duration * FPS);
  for (let i = 0; i < total; i++) {
    const ok = ff.stdin.write(Buffer.from(await frame(i / FPS), 'base64'));
    if (!ok) await new Promise(r => ff.stdin.once('drain', r));
    if (i % 60 === 0) process.stdout.write(`\r${i}/${total}`);
  }
  ff.stdin.end();
  await new Promise(r => ff.on('close', r));
  console.log(`\r${total}/${total} → ${out}`);
}

ws.close();
chrome.kill();
await sleep(300);
rmSync(profile, { recursive: true, force: true });
