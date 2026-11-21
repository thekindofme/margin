/* Milkrice — the hero demo. One function, render(t), draws the whole scripted story for a
   moment in time, so playing, pausing, seeking (the step pills) and the static
   reduced-motion frame are all the same code path. The markup is the finished frame. */
(() => {
  'use strict';
  const demo = document.getElementById('demo');
  if (!demo) return;
  const $ = (k) => demo.querySelector(`[data-d="${k}"]`);
  const el = {};
  ['banner', 'bStart', 'vHome', 'vNote', 'curSub', 'curDot', 'dur', 'tNotes', 'tTx', 'txN', 'pTx', 'pNotes', 'cue',
    'raw', 'done', 'typed', 'caret', 'busy', 'ai', 'yours', 'dock', 'timer', 'stop', 'stopL', 'term', 'q', 'qCaret', 'tool', 'ans']
    .forEach((k) => { el[k] = $(k); });
  const pills = [...demo.querySelectorAll('.dsteps li')];
  const lines = [...demo.querySelectorAll('.dline')].map((n) => ({ n, p: n.querySelector('p'), words: n.querySelector('p').textContent.split(' ') }));
  const aiRows = [...el.ai.children];
  const typedFull = el.typed.textContent;
  const qFull = el.q.textContent;

  // The script, in seconds.
  const STEP = [0, 2.4, 9.2, 14.6, 20.2];
  const END = 28.5, FADE = 27.9;
  const LINE_AT = [3.0, 4.6, 6.1, 7.7];
  const TYPE_AT = 9.9, TYPE_CPS = 15;
  const WRITTEN_AT = 16.9, TERM_AT = 20.7, Q_AT = 21.3, Q_CPS = 20;
  const TOOL_AT = Q_AT + qFull.length / Q_CPS + 0.5, ANS_AT = TOOL_AT + 0.9;
  const REC_RATE = 37;   // the call runs in fast-forward

  const last = new Map();
  const set = (key, v, apply) => { if (last.get(key) !== v) { last.set(key, v); apply(v); } };
  const cls = (node, name, on) => set(`${name}@${idOf(node)}`, !!on, (b) => node.classList.toggle(name, b));
  const ids = new WeakMap(); let seq = 0;
  const idOf = (n) => { if (!ids.has(n)) ids.set(n, ++seq); return ids.get(n); };
  const txt = (node, s) => set(`t@${idOf(node)}`, s, (v) => { node.textContent = v; });
  const hide = (node, h) => set(`h@${idOf(node)}`, !!h, (b) => { node.hidden = b; });
  const mmss = (s) => `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
  const recEnd = Math.floor((STEP[3] - STEP[1]) * REC_RATE);

  function render(t) {
    const step = t < STEP[1] ? 1 : t < STEP[2] ? 2 : t < STEP[3] ? 3 : t < STEP[4] ? 4 : 5;
    set('step', step, (v) => { demo.dataset.step = v; });
    pills.forEach((li, i) => { cls(li, 'on', i + 1 === step); cls(li, 'done', i + 1 < step); });
    cls(demo, 'out', t >= FADE);

    // 1. the nudge
    cls(el.banner, 'on', t >= 0.5 && t < STEP[1]);
    cls(el.bStart, 'press', t >= 1.9 && t < STEP[1]);
    cls(el.vHome, 'on', step === 1);
    cls(el.vNote, 'on', step > 1);

    // recording state
    const rec = step === 2 || step === 3;
    const secs = Math.min(recEnd, Math.max(0, Math.floor((t - STEP[1]) * REC_RATE)));
    cls(el.dock, 'on', step > 1);
    cls(el.dock, 'rec', rec);
    txt(el.timer, mmss(secs));
    txt(el.stopL, rec ? 'Stop' : 'Resume');
    cls(el.stop, 'press', t >= STEP[3] - 0.25 && t < STEP[3]);
    hide(el.curDot, !rec);
    txt(el.curSub, rec ? 'Recording…' : `10:00 · ${mmss(recEnd).replace(/^0/, '')} · Maya, Jonas`);
    hide(el.dur, rec);
    txt(el.dur, mmss(recEnd).replace(/^0/, ''));

    // 2. live transcript
    const onTx = step === 2;
    cls(el.tTx, 'on', onTx); cls(el.tNotes, 'on', !onTx);
    cls(el.pTx, 'on', onTx); cls(el.pNotes, 'on', !onTx);
    let shown = 0;
    lines.forEach((l, i) => {
      const dt = t - LINE_AT[i];
      cls(l.n, 'on', dt >= 0);
      if (dt >= 0) shown++;
      const n = Math.max(0, Math.min(l.words.length, Math.ceil(dt * 9)));
      txt(l.p, l.words.slice(0, n).join(' '));
    });
    cls(el.cue, 'on', rec && t >= STEP[1] + 0.3);
    txt(el.txN, String(shown + (t > 10.6) + (t > 12.1) + (t > 13.6)));

    // 3. rough notes
    const typedN = Math.max(0, Math.min(typedFull.length, Math.floor((t - TYPE_AT) * TYPE_CPS)));
    txt(el.typed, typedFull.slice(0, typedN));
    cls(el.typed.parentNode, 'ph', typedN === 0);
    cls(el.caret, 'on', step === 3);

    // 4. the write-up
    const written = t >= WRITTEN_AT;
    hide(el.raw, written); hide(el.done, !written);
    cls(el.busy, 'on', t >= STEP[3] + 0.3 && !written);
    aiRows.forEach((row, i) => cls(row, 'on', t >= WRITTEN_AT + 0.15 + i * 0.32));
    cls(el.yours, 'on', t >= WRITTEN_AT + 0.15 + aiRows.length * 0.32);

    // 5. the agent
    cls(el.term, 'on', t >= TERM_AT);
    const qN = Math.max(0, Math.min(qFull.length, Math.floor((t - Q_AT) * Q_CPS)));
    txt(el.q, qFull.slice(0, qN));
    cls(el.qCaret, 'on', t >= TERM_AT && t < TOOL_AT);
    cls(el.tool, 'on', t >= TOOL_AT);
    cls(el.ans, 'on', t >= ANS_AT);
  }

  const reduced = matchMedia('(prefers-reduced-motion: reduce)');
  const fixed = /[?&]demo_t=([\d.]+)/.exec(location.search);
  let t = 0, prev = 0, raf = 0, visible = true;

  function frame(now) {
    raf = 0;
    t += Math.min(0.1, (now - prev) / 1000); prev = now;
    if (t >= END) t = 0;
    render(t);
    tick();
  }
  function tick() {
    const run = visible && !document.hidden && !reduced.matches && !fixed;
    demo.classList.toggle('play', run);
    if (run && !raf) { prev = performance.now(); raf = requestAnimationFrame(frame); }
    if (!run && raf) { cancelAnimationFrame(raf); raf = 0; }
  }

  demo.querySelectorAll('[data-go]').forEach((b) => b.addEventListener('click', () => {
    const i = +b.dataset.go;
    // With motion off, a pill shows that step's finished frame instead of playing it.
    t = reduced.matches ? (i < 4 ? STEP[i + 1] - 0.3 : FADE - 0.1) : STEP[i] + (i ? 0 : 0.3);
    render(t);
  }));

  if (fixed) { t = +fixed[1]; render(t); }
  else if (reduced.matches) render(FADE - 0.1);
  else render(0);
  demo.classList.add('ready');

  if ('IntersectionObserver' in window) {
    new IntersectionObserver((es) => { visible = es[es.length - 1].isIntersecting; tick(); }, { threshold: 0.05 }).observe(demo);
  }
  document.addEventListener('visibilitychange', tick);
  reduced.addEventListener && reduced.addEventListener('change', () => { if (reduced.matches) render(FADE - 0.1); tick(); });
  tick();
})();
