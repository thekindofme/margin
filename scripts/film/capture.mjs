#!/usr/bin/env node
/* Frame-by-frame capture of the Milkrice film. Node 22+, no dependencies.
   Drives headless Chrome over the DevTools protocol and calls window.__film.seek(t).

   All frames:  node capture.mjs --out /path/to/frames
   Stills:      node capture.mjs --out /path/to/stills --times 2,6.5,10
   Then encode: see the ffmpeg lines at the bottom of this file. */
import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const args = Object.fromEntries(process.argv.slice(2).join(' ').split('--').filter(Boolean).map((a) => { const [k, ...v] = a.trim().split(' '); return [k, v.join(' ') || true]; }));
const here = dirname(fileURLToPath(import.meta.url));
const out = resolve(args.out || 'frames');
const W = +(args.width || 1920), H = +(args.height || 1080);
const chrome = args.chrome || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const port = +(args.port || 9377);
const url = pathToFileURL(join(here, 'index.html')).href + '?capture';
mkdirSync(out, { recursive: true });

const profile = mkdtempSync(join(tmpdir(), 'milkrice-film-'));
const proc = spawn(chrome, ['--headless=new', `--remote-debugging-port=${port}`, `--user-data-dir=${profile}`, `--window-size=${W},${H}`, '--hide-scrollbars', '--force-device-scale-factor=1', '--no-first-run', '--disable-extensions', 'about:blank'], { stdio: 'ignore' });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function target() {
  for (let i = 0; i < 60; i++) {
    try { const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json(); const p = list.find((t) => t.type === 'page'); if (p) return p.webSocketDebuggerUrl; } catch {}
    await sleep(250);
  }
  throw new Error('Chrome did not start');
}

try {
  const ws = new WebSocket(await target());
  await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
  let id = 0; const pending = new Map();
  ws.onmessage = (m) => { const d = JSON.parse(m.data); if (d.id && pending.has(d.id)) { const [r, j] = pending.get(d.id); pending.delete(d.id); d.error ? j(new Error(d.error.message)) : r(d.result); } };
  const send = (method, params = {}) => new Promise((r, j) => { pending.set(++id, [r, j]); ws.send(JSON.stringify({ id, method, params })); });
  const ev = async (expression) => { const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true }); if (r.exceptionDetails) throw new Error(r.exceptionDetails.text + ' ' + (r.exceptionDetails.exception?.description || '')); return r.result.value; };

  await send('Page.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
  await send('Page.navigate', { url });
  for (let i = 0; i < 80 && !(await ev('!!(window.__film)').catch(() => false)); i++) await sleep(100);
  await ev('window.__film.ready');
  await ev('document.fonts.load(\'400 40px "Fraunces"\').then(() => document.fonts.load(\'italic 400 40px "Fraunces"\')).then(() => true)');
  const { duration, fps } = await ev('({duration: __film.duration, fps: __film.fps})');
  const fontOk = await ev('document.fonts.check(\'400 40px "Fraunces"\')');
  if (!fontOk) console.warn('warning: Fraunces did not load, falling back');

  const times = args.times ? String(args.times).split(',').map(Number) : Array.from({ length: Math.round(duration * fps) }, (_, i) => i / fps);
  const t0 = Date.now();
  for (let i = 0; i < times.length; i++) {
    await ev(`__film.seek(${times[i]}); new Promise(r => requestAnimationFrame(() => requestAnimationFrame(() => r(true))))`);
    const shot = await send('Page.captureScreenshot', { format: 'png', clip: { x: 0, y: 0, width: W, height: H, scale: 1 }, optimizeForSpeed: true });
    const name = args.times ? `t${times[i].toFixed(2).padStart(5, '0')}.png` : `f${String(i).padStart(5, '0')}.png`;
    writeFileSync(join(out, name), Buffer.from(shot.data, 'base64'));
    if (!args.times && i % 150 === 0) console.log(`${i}/${times.length} frames, ${((Date.now() - t0) / 1000).toFixed(0)}s`);
  }
  console.log(`done: ${times.length} frames in ${out} (${duration}s at ${fps} fps)`);
  ws.close();
} finally {
  proc.kill();
  await sleep(300);
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
}

/* Encode (one at a time):
   ffmpeg -y -framerate 30 -i frames/f%05d.png -c:v libx264 -preset slow -crf 22 -pix_fmt yuv420p -movflags +faststart -an film.mp4
   ffmpeg -y -framerate 30 -i frames/f%05d.png -c:v libvpx-vp9 -crf 31 -b:v 0 -row-mt 1 -pix_fmt yuv420p -an film.webm
   cwebp -q 88 frames/f01695.png -o poster.webp */
