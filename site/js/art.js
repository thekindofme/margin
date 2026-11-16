/* Margin — hero piece: sound becomes ink.
   A travelling sound wave (made of graphite particles) runs toward the ember margin rule.
   Each particle that crosses the rule flies to the nib and lays down a stroke of handwriting,
   so louder sound writes bigger, faster words. Pointer movement raises the voice, a click
   claps (a pulse plus ripples), and dragging grabs the wave like a string. Canvas 2D only. */
(() => {
  'use strict';
  const host = document.getElementById('art');
  const hero = host && host.closest('.hero');
  const canvas = host && host.querySelector('canvas');
  const ctx = canvas && canvas.getContext && canvas.getContext('2d');
  if (!ctx) { document.documentElement.classList.add('no-art'); return; }

  const wideMQ = matchMedia('(min-width: 900px)');
  const reduceMQ = matchMedia('(prefers-reduced-motion: reduce)');
  const darkMQ = matchMedia('(prefers-color-scheme: dark)');
  const TAU = Math.PI * 2;
  const rand = (a, b) => a + Math.random() * (b - a);
  const clamp = (v, a, b) => (v < a ? a : v > b ? b : v);
  const gauss = () => { let s = 0; for (let i = 0; i < 3; i++) s += Math.random(); return (s - 1.5) / 1.5; };

  // ---------------------------------------------------------------- geometry + colours
  let W = 0, H = 0, dpr = 1, wide = true;
  let rx = 300, baseY = 300, ampPx = 30, speed = 110, clipA = 80;
  let wx0 = 0, wx1 = 0, wy0 = 0, lineH = 46, nLines = 3;
  let col = {};
  const ink = document.createElement('canvas');
  const ictx = ink.getContext('2d');
  const ghost = document.createElement('canvas');
  const gctx = ghost.getContext('2d');

  function readColours() {
    const cs = getComputedStyle(document.documentElement);
    const v = (n, f) => (cs.getPropertyValue(n).trim() || f);
    col = { ink: v('--ink', '#151413'), ember: v('--ember', '#e2603a'), hair: v('--hair', 'rgba(21,20,19,.11)'), paper: v('--paper', '#f3efe6'), ink3: v('--ink-3', '#6b665e') };
  }

  function layout() {
    wide = wideMQ.matches;
    const r = host.getBoundingClientRect();
    W = Math.max(1, Math.round(r.width));
    H = Math.max(1, Math.round(r.height));
    dpr = Math.min(window.devicePixelRatio || 1, quality < 1 ? 1 : 2);
    canvas.width = Math.round(W * dpr);
    canvas.height = Math.round(H * dpr);

    if (wide) {
      const ruleEl = document.querySelector('.rule');
      const copy = hero.querySelector('.hero-copy');
      rx = ruleEl ? ruleEl.getBoundingClientRect().left - r.left + 0.75 : W * 0.3;
      const gap = parseFloat(getComputedStyle(document.documentElement).getPropertyValue('--gap')) || 52;
      wx0 = rx + gap;
      wx1 = Math.min(W - 48, wx0 + 760);
      lineH = 54;
      const lastEl = copy.lastElementChild || copy;
      const top = lastEl.getBoundingClientRect().bottom - r.top + 28;
      const bottom = H - 30;
      nLines = clamp(Math.floor((bottom - top) / lineH), 2, 4);
      wy0 = top + Math.max(0, (bottom - top - nLines * lineH) * 0.35);
      baseY = wy0 + nLines * lineH * 0.55;
      ampPx = 40; clipA = 90;
      const hint = hero.querySelector('.hint');
      if (hint) { hint.style.top = Math.max(top - 60, baseY - 150) + 'px'; hint.style.bottom = 'auto'; }
    } else {
      rx = Math.round(W * 0.3);
      lineH = 38;
      wx0 = rx + 16;
      wx1 = W - 16;
      nLines = clamp(Math.floor((H - 50) / lineH), 2, 6);
      wy0 = Math.round((H - nLines * lineH) / 2);
      baseY = H * 0.5;
      ampPx = 26; clipA = H * 0.36;
    }
    speed = clamp(rx / 3, 70, 140);
    ink.width = Math.round((wx1 - wx0) * dpr);
    ink.height = Math.round((nLines + 0.6) * lineH * dpr);
    ghost.width = ink.width;
    ghost.height = Math.round(lineH * dpr);
    readColours();
    resetSim();
  }

  // ---------------------------------------------------------------- wave field
  const N = 160;
  let dx = 2, acc = 0;
  const trav = new Float32Array(N);   // travelling sound displacement (px)
  const eng = new Float32Array(N);    // travelling loudness 0..~1.6
  const str = new Float32Array(N);    // string displacement (drag + claps)
  const strV = new Float32Array(N);

  // the "voice" at the source
  let t = 0, phase = 0, env = 0, talking = true, talkLeft = 2.5, loud = 0;

  function sourceStep(dt) {
    talkLeft -= dt;
    if (talkLeft <= 0) {
      talking = !talking;
      talkLeft = talking ? rand(1.4, 3.6) : rand(0.5, 1.4);
    }
    if (loud > 0.22) { talking = true; talkLeft = Math.max(talkLeft, 0.4); }
    const syll = 0.55 + 0.45 * Math.abs(Math.sin(TAU * 2.6 * t + Math.sin(t * 1.7) * 2));
    const target = talking ? (0.6 + loud) * syll : 0.03 + loud * 0.5;
    env += (target - env) * (1 - Math.exp(-dt * 9));
    phase += TAU * dt * (1.25 + 0.25 * Math.sin(t * 0.6));
    return env;
  }

  function sampleArr(a, x) {
    const f = x / dx - acc;
    if (f <= 0) return a[0];
    const i = Math.floor(f);
    if (i >= N - 1) return a[N - 1];
    const k = f - i;
    return a[i] + (a[i + 1] - a[i]) * k;
  }
  function strAt(x) {
    const f = clamp(x / dx, 0, N - 1);
    const i = Math.min(Math.floor(f), N - 2);
    const k = f - i;
    return str[i] + (str[i + 1] - str[i]) * k;
  }
  function disp(x) {
    const taper = clamp(x / 70, 0, 1);
    const d = (sampleArr(trav, x) * taper) + strAt(x);
    return clipA * Math.tanh(d / clipA);
  }

  // ---------------------------------------------------------------- handwriting
  let lines = [], lineIdx = 0, scrollBase = 0, scrollAnim = 0, ghostA = 0, ghostY = 0;
  let pen = null;
  let lastDep = null; // {line, X}

  function newLine() {
    const L = { k: lineIdx++, letters: [], gaps: [], end: 0, seed: Math.random() * 100 };
    lines.push(L);
    if (lines.length > nLines + 2) lines.shift();
    if (L.k - scrollBase >= nLines) shiftUp();
    return L;
  }
  function lineByK(k) { for (let i = lines.length - 1; i >= 0; i--) if (lines[i].k === k) return lines[i]; return null; }

  function shiftUp() {
    // keep the outgoing top line as a fading ghost, move the rest up one line
    gctx.clearRect(0, 0, ghost.width, ghost.height);
    gctx.drawImage(ink, 0, 0);
    ghostA = 1; ghostY = 0;
    gctx.globalCompositeOperation = 'source-over';
    const tmp = ictx.getImageData(0, Math.round(lineH * dpr), ink.width, ink.height - Math.round(lineH * dpr));
    ictx.clearRect(0, 0, ink.width, ink.height);
    ictx.putImageData(tmp, 0, 0);
    scrollBase++;
    scrollAnim += lineH;
  }

  function addLetter(L, s, loudness) {
    const lh = lineH;
    const roll = Math.random();
    const sc = clamp(0.8 + loudness * 0.6, 0.8, 1.5) * rand(0.85, 1.15);
    let w, h, loop, down = false;
    if (roll < 0.17) { h = lh * 0.4 * Math.min(sc, 1.25); w = rand(7, 10); loop = rand(1.6, 2.3); }          // tall, looped (l, h, k)
    else if (roll < 0.26) { h = lh * 0.26 * Math.min(sc, 1.3); w = rand(7, 10); loop = rand(1.4, 2); down = true; } // descender (g, y)
    else if (roll < 0.45) { h = lh * 0.17 * sc; w = rand(6, 8); loop = rand(1.2, 1.6); }                     // small loop (e)
    else if (roll < 0.6) { h = lh * 0.19 * sc; w = rand(9, 13); loop = rand(0.0, 0.25); }                   // wide arch (n, m)
    else { h = lh * 0.19 * sc; w = rand(6.5, 10); loop = rand(0.3, 0.9); }                                   // u, i, r
    const c = (w / TAU) * loop;
    L.letters.push({ s, w, h, c, down });
    L.end = s + w;
  }

  // point on the handwriting at baseline-advance X; returns null inside a word gap
  function penPoint(L, X, out) {
    const ls = L.letters;
    let lt = null;
    for (let i = ls.length - 1; i >= 0; i--) { if (ls[i].s <= X) { lt = ls[i]; break; } }
    if (!lt || X > lt.s + lt.w + 0.01) return null;
    const th = ((X - lt.s) / lt.w) * TAU;
    const lift = (1 - Math.cos(th)) / 2;
    const y = lt.down ? lt.h * lift : -lt.h * lift;
    const wob = Math.sin(X * 0.021 + L.seed) * 1.3 + Math.sin(X * 0.0063 + L.seed * 2) * 1.6;
    out.x = X - lt.c * Math.sin(th) - y * 0.3;
    out.y = y + wob;
    return out;
  }

  function slotBaseline(L) { return (L.k - scrollBase + 0.66) * lineH; }

  function penStep(dt, eRule) {
    if (!pen) { pen = { L: newLine(), X: 2, inWord: false, finishing: false }; }
    const speaking = eRule > 0.1;
    const usable = wx1 - wx0 - 6;
    if (speaking && !pen.inWord) {
      // start a word: leave a space, wrap the line if needed
      if (pen.L.letters.length) pen.X = pen.L.end + rand(9, 15);
      if (pen.X > usable - 40) { pen = { L: newLine(), X: 2, inWord: false }; }
      pen.L.gaps.push(pen.X);
      pen.inWord = true;
      pen.left = Math.round(rand(2, 9));
    }
    if (pen.inWord) {
      const rate = 22 + 62 * Math.min(eRule, 1.4);
      pen.X += rate * dt;
      const gapStart = pen.L.gaps[pen.L.gaps.length - 1];
      if (speaking && pen.L.end <= usable && pen.left > 0) {
        while ((pen.L.end < pen.X + 1 || pen.L.end <= gapStart) && pen.left > 0) { addLetter(pen.L, Math.max(pen.L.end, gapStart), eRule); pen.left--; }
      }
      const end = pen.L.end;
      if (pen.X >= end) {
        // finished the last letter and there's no more sound (or room): lift the pen
        pen.X = end;
        if (!speaking || end > usable || pen.left <= 0) pen.inWord = false;
        if (end > usable) pen.X = usable + 50;
      }
    }
  }

  const P1 = { x: 0, y: 0 }, P2 = { x: 0, y: 0 };
  function deposit(lk, X) {
    const L = lineByK(lk);
    if (!L) return;
    const slot = L.k - scrollBase;
    if (slot < 0) return;
    if (!lastDep || lastDep.line !== lk || X <= lastDep.X || X - lastDep.X > 40) {
      lastDep = { line: lk, X };
      return;
    }
    const a = lastDep.X, b = X;
    lastDep.X = X;
    const by = slotBaseline(L) * dpr;
    ictx.strokeStyle = col.ink;
    ictx.lineCap = 'round';
    ictx.lineJoin = 'round';
    let drawing = false, px = 0, py = 0;
    const n = Math.max(2, Math.ceil((b - a) / 0.7));
    ictx.beginPath();
    for (let i = 0; i <= n; i++) {
      const xx = a + (b - a) * (i / n);
      const p = penPoint(L, xx, P1);
      if (!p) { drawing = false; continue; }
      const cx = p.x * dpr, cy = by + p.y * dpr;
      if (!drawing) { ictx.moveTo(cx, cy); drawing = true; }
      else ictx.lineTo(cx, cy);
      px = cx; py = cy;
    }
    ictx.lineWidth = 1.25 * dpr;
    ictx.globalAlpha = 0.82;
    ictx.stroke();
    ictx.globalAlpha = 1;
  }

  // ---------------------------------------------------------------- particles
  const MAXP = 1400;
  const px = new Float32Array(MAXP), py = new Float32Array(MAXP), ps = new Float32Array(MAXP);
  const pv = new Float32Array(MAXP), pt = new Float32Array(MAXP), pd = new Float32Array(MAXP);
  const fx = new Float32Array(MAXP), fy = new Float32Array(MAXP), fX = new Float32Array(MAXP), fc = new Float32Array(MAXP);
  const pst = new Uint8Array(MAXP); // 0 free, 1 sound, 2 flying
  const pline = new Int32Array(MAXP);
  let spawnAcc = 0, cap = 900, quality = 1;

  function spawn() {
    for (let i = 0; i < MAXP; i++) if (pst[i] === 0) {
      pst[i] = 1; px[i] = -2; ps[i] = gauss(); pv[i] = speed * rand(0.97, 1.04); return;
    }
  }

  function particlesStep(dt) {
    const e0 = eng[0];
    spawnAcc += dt * (70 + 300 * Math.min(e0, 1.5));
    let alive = 0;
    for (let i = 0; i < MAXP; i++) if (pst[i]) alive++;
    while (spawnAcc >= 1) { spawnAcc -= 1; if (alive < cap) { spawn(); alive++; } }

    const writing = pen && pen.inWord;
    for (let i = 0; i < MAXP; i++) {
      const s = pst[i];
      if (s === 1) {
        px[i] += pv[i] * dt;
        const e = sampleArr(eng, px[i]);
        py[i] = baseY + disp(px[i]) + ps[i] * (1.5 + 9 * e);
        if (px[i] >= rx) {
          if (writing && Math.abs(ps[i]) < 1.1) {
            pst[i] = 2; fx[i] = rx; fy[i] = py[i]; pt[i] = 0; pd[i] = rand(0.5, 0.62);
            fX[i] = pen.X; pline[i] = pen.L.k; fc[i] = rand(-40, -8);
          } else if (px[i] > rx + 6) pst[i] = 0;
        }
      } else if (s === 2) {
        pt[i] += dt;
        if (pt[i] >= pd[i]) { deposit(pline[i], fX[i]); pst[i] = 0; }
      }
    }
  }

  function flightPos(i, out) {
    const L = lineByK(pline[i]);
    if (!L) return null;
    const p = penPoint(L, fX[i], P2);
    const tx = wx0 + (p ? p.x : fX[i]);
    const ty = wy0 + slotBaseline(L) + scrollAnim + (p ? p.y : 0);
    let k = pt[i] / pd[i];
    k = k < 0.5 ? 2 * k * k : 1 - Math.pow(-2 * k + 2, 2) / 2;
    const cx = fx[i] + (tx - fx[i]) * 0.3, cy = Math.min(fy[i], ty) + fc[i];
    const u = 1 - k;
    out.x = u * u * fx[i] + 2 * u * k * cx + k * k * tx;
    out.y = u * u * fy[i] + 2 * u * k * cy + k * k * ty;
    return out;
  }

  // ---------------------------------------------------------------- interaction state
  const ptr = { x: -1e4, y: -1e4, vx: 0, vy: 0, inside: false, down: false, drag: false, downAt: 0, sx: 0, sy: 0, t: 0 };
  let rings = [];

  function nearWave(x, y) { return x >= 0 && x <= rx + 24 && Math.abs(y - (baseY + disp(clamp(x, 0, rx)))) < 70; }

  function exciteAt(x, amount, vel) {
    const c = x / dx, R = Math.max(4, 24 / dx);
    for (let i = Math.max(1, Math.floor(c - R * 2)); i < Math.min(N - 1, Math.ceil(c + R * 2)); i++) {
      const g = Math.exp(-((i - c) * (i - c)) / (R * R));
      strV[i] += vel * g;
      eng[i] = Math.min(1.6, eng[i] + amount * g);
    }
  }

  function clap(x, y) {
    rings.push({ x, y, t: 0 });
    if (rings.length > 6) rings.shift();
    loud = Math.min(1.3, loud + 0.55);
    if (x <= rx + 40) exciteAt(clamp(x, 0, rx), 0.6, (y < baseY ? -1 : 1) * 260);
  }

  function stringStep(dt) {
    const sub = 4, h = dt / sub;
    const cs = Math.min(380, (0.7 * dx) / h), c2 = (cs * cs) / (dx * dx);
    const damp = 1.8, k0 = 4, lim = H * 0.35;
    for (let s = 0; s < sub; s++) {
      for (let i = 1; i < N - 1; i++) strV[i] += (c2 * (str[i - 1] + str[i + 1] - 2 * str[i]) - damp * strV[i] - k0 * str[i]) * h;
      for (let i = 1; i < N - 1; i++) { str[i] = clamp(str[i] + strV[i] * h, -lim, lim); }
      str[0] = str[N - 1] = 0;
      if (ptr.drag) {
        const c = clamp(ptr.x, 0, rx) / dx, R = Math.max(6, 30 / dx);
        const target = clamp(ptr.y - baseY, -H * 0.32, H * 0.32);
        for (let i = 1; i < N - 1; i++) {
          const g = Math.exp(-((i - c) * (i - c)) / (R * R));
          if (g < 0.02) continue;
          str[i] += (target * g - str[i]) * 0.35 * g;
          strV[i] *= 1 - 0.5 * g;
        }
      }
    }
    for (let i = 0; i < N; i++) { if (!(Math.abs(str[i]) < 1e4)) { str[i] = 0; strV[i] = 0; } }
  }

  // ---------------------------------------------------------------- simulation step
  function step(dt) {
    t += dt;
    loud *= Math.exp(-dt / 1.3);
    const e = sourceStep(dt);
    // advect the travelling sound toward the rule
    acc += (speed * dt) / dx;
    while (acc >= 1) {
      acc -= 1;
      trav.copyWithin(1, 0, N - 1);
      eng.copyWithin(1, 0, N - 1);
      trav[0] = ampPx * e * (Math.sin(phase) + 0.32 * Math.sin(phase * 2.7 + 1.1));
      eng[0] = e;
    }
    stringStep(dt);
    const eRule = sampleArr(eng, rx - 2) + Math.min(0.5, Math.abs(strAt(rx * 0.92)) / 80);
    penStep(dt, eRule);
    particlesStep(dt);
    scrollAnim *= Math.exp(-dt * 5); if (scrollAnim < 0.05) scrollAnim = 0;
    ghostA = Math.max(0, ghostA - dt * 1.4); ghostY -= dt * 26;
    for (const r of rings) r.t += dt;
    rings = rings.filter((r) => r.t < 1.6);
  }

  function resetSim() {
    dx = rx / (N - 1);
    trav.fill(0); eng.fill(0); str.fill(0); strV.fill(0);
    for (let i = 0; i < MAXP; i++) pst[i] = 0;
    lines = []; lineIdx = 0; scrollBase = 0; scrollAnim = 0; pen = null; lastDep = null; ghostA = 0;
    ictx.clearRect(0, 0, ink.width, ink.height);
    t = 0; env = 0; talking = true; talkLeft = 2.2; loud = 0; acc = 0;
    // warm up so the page opens mid-sentence
    const warm = reduceMQ.matches ? 26 : 9;
    for (let s = 0; s < warm * 60; s++) step(1 / 60);
  }

  // ---------------------------------------------------------------- render
  const FP = { x: 0, y: 0 };
  function render() {
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, W, H);

    // ruled lines on the writing side
    ctx.strokeStyle = col.hair; ctx.lineWidth = 1;
    ctx.beginPath();
    for (let j = 0; j <= nLines; j++) {
      const y = Math.round(wy0 + (j + 0.66) * lineH + scrollAnim) + 0.5;
      if (y > wy0 - 4 && y < wy0 + (nLines + 0.4) * lineH) { ctx.moveTo(wx0 - 6, y); ctx.lineTo(wx1, y); }
    }
    ctx.stroke();

    // ink
    ctx.drawImage(ink, wx0, wy0 + scrollAnim, ink.width / dpr, ink.height / dpr);
    if (ghostA > 0) { ctx.globalAlpha = ghostA; ctx.drawImage(ghost, wx0, wy0 + scrollAnim - lineH + ghostY, ghost.width / dpr, ghost.height / dpr); ctx.globalAlpha = 1; }

    // the wave line
    ctx.strokeStyle = col.ink; ctx.globalAlpha = 0.28; ctx.lineWidth = 1;
    ctx.beginPath();
    for (let x = 0; x <= rx; x += 3) { const y = baseY + disp(x); if (x === 0) ctx.moveTo(x, y); else ctx.lineTo(x, y); }
    ctx.stroke();

    // sound particles
    ctx.fillStyle = col.ink;
    ctx.globalAlpha = 0.5;
    for (let i = 0; i < MAXP; i++) if (pst[i] === 1) {
      const a = Math.abs(ps[i]);
      if (a > 0.85) continue;
      ctx.fillRect(px[i] - 0.6, py[i] - 0.6, 1.3, 1.3);
    }
    ctx.globalAlpha = 0.22;
    for (let i = 0; i < MAXP; i++) if (pst[i] === 1 && Math.abs(ps[i]) > 0.85) ctx.fillRect(px[i] - 0.6, py[i] - 0.6, 1.2, 1.2);

    // particles in flight, ember while crossing, ink as they land
    ctx.fillStyle = col.ember; ctx.globalAlpha = 0.85;
    for (let i = 0; i < MAXP; i++) if (pst[i] === 2 && pt[i] / pd[i] < 0.55) { const p = flightPos(i, FP); if (p) ctx.fillRect(p.x - 0.75, p.y - 0.75, 1.6, 1.6); }
    ctx.fillStyle = col.ink; ctx.globalAlpha = 0.7;
    for (let i = 0; i < MAXP; i++) if (pst[i] === 2 && pt[i] / pd[i] >= 0.55) { const p = flightPos(i, FP); if (p) ctx.fillRect(p.x - 0.6, p.y - 0.6, 1.3, 1.3); }
    ctx.globalAlpha = 1;

    // nib
    if (pen && pen.inWord) {
      const p = penPoint(pen.L, Math.min(pen.X, pen.L.end - 0.01), P1);
      if (p) { ctx.fillStyle = col.ember; ctx.beginPath(); ctx.arc(wx0 + p.x, wy0 + slotBaseline(pen.L) + scrollAnim + p.y, 2.2, 0, TAU); ctx.fill(); }
    }

    // the margin rule (the page draws its own on wide screens)
    if (!wide) { ctx.fillStyle = col.ember; ctx.fillRect(Math.round(rx) - 0.75, 0, 1.5, H); }

    // clap ripples
    if (rings.length) {
      ctx.strokeStyle = col.ink3; ctx.lineWidth = 1;
      for (const r of rings) {
        for (let k = 0; k < 4; k++) {
          const rad = 6 + r.t * 110 + k * 13;
          const a = Math.max(0, 0.5 * (1 - r.t / 1.6) * (1 - k * 0.2));
          if (a <= 0) continue;
          ctx.globalAlpha = a; ctx.beginPath(); ctx.arc(r.x, r.y, rad, 0, TAU); ctx.stroke();
        }
        ctx.globalAlpha = Math.max(0, 1 - r.t * 1.5); ctx.fillStyle = col.ember;
        ctx.beginPath(); ctx.arc(r.x, r.y, 3, 0, TAU); ctx.fill();
      }
      ctx.globalAlpha = 1;
    }
  }

  // ---------------------------------------------------------------- loop
  let running = false, visible = true, last = 0, frames = 0, slowAcc = 0;
  function frame(now) {
    if (!running) return;
    const dtRaw = last ? (now - last) / 1000 : 1 / 60;
    last = now;
    const dt = Math.min(dtRaw, 1 / 30);
    // pointer energy
    if (ptr.inside && !ptr.drag) {
      const sp = Math.hypot(ptr.vx, ptr.vy);
      if (sp > 0) {
        loud = Math.min(1.3, loud + sp * 0.00035 * (ptr.x < rx + 40 ? 1.6 : 0.7));
        if (nearWave(ptr.x, ptr.y)) exciteAt(clamp(ptr.x, 0, rx), Math.min(0.4, sp * 0.0004), clamp(ptr.vy * 0.25, -300, 300));
      }
      ptr.vx *= 0.6; ptr.vy *= 0.6;
    }
    step(dt);
    render();
    // step quality down on slow machines
    frames++; slowAcc += dtRaw;
    if (frames === 90) {
      if (slowAcc / frames > 0.024 && quality > 0.5) { quality = 0.5; cap = 480; dpr = 1; canvas.width = W; canvas.height = H; }
      frames = 0; slowAcc = 0;
    }
    requestAnimationFrame(frame);
  }
  function start() {
    if (running || reduceMQ.matches || !visible || document.hidden) return;
    running = true; last = 0; requestAnimationFrame(frame);
  }
  function stop() { running = false; }

  // ---------------------------------------------------------------- events
  function local(e) { const r = canvas.getBoundingClientRect(); return { x: e.clientX - r.left, y: e.clientY - r.top, r }; }

  hero.addEventListener('pointermove', (e) => {
    const p = local(e);
    const inside = p.x >= 0 && p.y >= 0 && p.x <= p.r.width && p.y <= p.r.height;
    if (inside && ptr.inside && ptr.t) {
      const dtm = Math.max(8, e.timeStamp - ptr.t) / 1000;
      ptr.vx = (p.x - ptr.x) / dtm; ptr.vy = (p.y - ptr.y) / dtm;
    }
    ptr.inside = inside; ptr.x = p.x; ptr.y = p.y; ptr.t = e.timeStamp;
    if (ptr.down && !ptr.drag && Math.hypot(p.x - ptr.sx, p.y - ptr.sy) > 6 && nearWave(ptr.sx, ptr.sy)) {
      ptr.drag = true; host.classList.add('grabbing');
    }
    if (!ptr.down) host.classList.toggle('grab', inside && nearWave(p.x, p.y));
    if (reduceMQ.matches) return;
  }, { passive: true });
  hero.addEventListener('pointerleave', () => { ptr.inside = false; host.classList.remove('grab'); });

  canvas.addEventListener('pointerdown', (e) => {
    const p = local(e);
    ptr.down = true; ptr.downAt = e.timeStamp; ptr.sx = p.x; ptr.sy = p.y; ptr.x = p.x; ptr.y = p.y;
    if (e.pointerType === 'mouse' && nearWave(p.x, p.y)) { try { canvas.setPointerCapture(e.pointerId); } catch (_) {} }
  });
  const up = (e) => {
    if (!ptr.down) return;
    const p = local(e);
    const quick = e.timeStamp - ptr.downAt < 350 && Math.hypot(p.x - ptr.sx, p.y - ptr.sy) < 8;
    if (!ptr.drag && quick && e.type === 'pointerup') { clap(p.x, p.y); if (reduceMQ.matches) { stepFor(1.2); } }
    if (ptr.drag) loud = Math.min(1.3, loud + 0.3);
    ptr.down = false; ptr.drag = false; host.classList.remove('grabbing');
  };
  canvas.addEventListener('pointerup', up);
  canvas.addEventListener('pointercancel', up);

  let lastScroll = window.scrollY;
  window.addEventListener('scroll', () => {
    const d = Math.abs(window.scrollY - lastScroll);
    lastScroll = window.scrollY;
    if (running) loud = Math.min(1.1, loud + Math.min(0.25, d * 0.002));
  }, { passive: true });

  if ('IntersectionObserver' in window) {
    new IntersectionObserver((ents) => {
      visible = ents[0].isIntersecting;
      if (visible) start(); else stop();
    }).observe(host);
  }
  document.addEventListener('visibilitychange', () => { if (document.hidden) stop(); else start(); });

  let rt = 0, lastW = 0;
  window.addEventListener('resize', () => {
    clearTimeout(rt);
    rt = setTimeout(() => {
      const w = host.getBoundingClientRect().width;
      if (Math.abs(w - lastW) < 2 && !wide === !wideMQ.matches) return; // ignore mobile URL-bar height changes
      lastW = w; layout(); render();
    }, 150);
  });
  const onPref = () => { stop(); layout(); render(); start(); };
  reduceMQ.addEventListener && reduceMQ.addEventListener('change', onPref);
  darkMQ.addEventListener && darkMQ.addEventListener('change', () => { readColours(); ictx.clearRect(0, 0, ink.width, ink.height); lastDep = null; render(); });

  function stepFor(sec) { const n = Math.round(sec * 60); for (let i = 0; i < n; i++) step(1 / 60); render(); }

  // review hook for headless screenshots
  window.__marginArt = {
    step: stepFor,
    clap: (x, y) => { clap(x, y); },
    pull: (x, y, sec) => { ptr.drag = true; ptr.x = x; ptr.y = y; stepFor(sec || 0.5); ptr.drag = false; },
    stir: (amount) => { loud = Math.min(1.3, loud + (amount || 1)); },
    info: () => ({ W, H, rx, baseY, running, lines: lineIdx, penX: pen && pen.X, alive: Array.from(pst).filter(Boolean).length }),
  };

  function boot() { layout(); lastW = W; render(); start(); }
  if (document.fonts && document.fonts.ready) {
    let booted = false;
    const go = () => { if (!booted) { booted = true; boot(); } };
    document.fonts.ready.then(go); setTimeout(go, 1200);
  } else boot();
})();
