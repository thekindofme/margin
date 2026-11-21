/* Milkrice film.
   One inline SVG, one time value. seek(t) sets every animated property from
   scratch, so a given t always renders the same frame (frame-by-frame capture). */
(() => {
'use strict';

const DUR = 58.5, FPS = 30, POSTER_T = 56.5;
const C = {
  cream: '#fbf5e6', cream2: '#f5ecd7', cream3: '#ebdfc4', rice: '#fffdf6',
  ink: '#1f1a14', ink2: '#5b5145', ink3: '#776c5e',
  leaf: '#2e6b34', leafDeep: '#1d4526', leafSoft: '#e3ead0', leafMid: '#4a8a4f',
  clay: '#c4602f', chili: '#d2381f',
  disc: '#f2e7cd', desk: '#e3d1a8', alu: '#c9bea8', aluDeep: '#a99d85',
  band: '#1d4526', bandInk: '#f6f0df', bandInk2: '#c6d3bd', term: '#1c1914', termInk: '#ece5d5',
};
const svg = document.getElementById('film');
const $ = (s) => svg.querySelector(s);
const $$ = (s) => [...svg.querySelectorAll(s)];

/* ---------- maths */
const clamp = (x, a = 0, b = 1) => Math.min(b, Math.max(a, x));
const sstep = (a, b, x) => { const p = clamp((x - a) / (b - a)); return p * p * (3 - 2 * p); };
const hash = (a, b = 0) => { const s = Math.sin(a * 127.1 + b * 311.7) * 43758.5453; return s - Math.floor(s); };
const EASE = {
  lin: (x) => x,
  io: (x) => (x < 0.5 ? 4 * x * x * x : 1 - Math.pow(-2 * x + 2, 3) / 2),
  io5: (x) => (x < 0.5 ? 16 * x ** 5 : 1 - Math.pow(-2 * x + 2, 5) / 2),
  out: (x) => 1 - Math.pow(1 - x, 3),
  o5: (x) => 1 - Math.pow(1 - x, 5),
  in: (x) => x * x * x,
  back: (x) => { const c = 1.6; return 1 + (c + 1) * Math.pow(x - 1, 3) + c * Math.pow(x - 1, 2); },
};
/* keyframes: [[t, value, easeIntoThisKey], …] */
function K(kfs) {
  return (t) => {
    if (t <= kfs[0][0]) return kfs[0][1];
    for (let i = 1; i < kfs.length; i++) {
      if (t <= kfs[i][0]) {
        const a = kfs[i - 1], b = kfs[i];
        const p = (t - a[0]) / (b[0] - a[0] || 1);
        return a[1] + (b[1] - a[1]) * EASE[b[2] || 'io'](p);
      }
    }
    return kfs[kfs.length - 1][1];
  };
}
const hex = (h) => (Array.isArray(h) ? h : [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16)));
const mixN = (a, b, p) => { const A = hex(a), B = hex(b); return A.map((v, i) => v + (B[i] - v) * p); };
const mix = (a, b, p) => `rgb(${mixN(a, b, p).map(Math.round).join(',')})`;

/* ---------- markup helpers */
const dia = (x, y, s, fill, extra = '') => `<rect x="${-s / 2}" y="${-s / 2}" width="${s}" height="${s}" rx="${s * 0.14}" fill="${fill}" transform="translate(${x} ${y}) rotate(45)" ${extra}/>`;
const lockGlyph = (stroke, s = 1) => `<g transform="scale(${s})"><rect x="-10" y="-3" width="20" height="16" rx="4" fill="${stroke}"/><path d="M-6 -3v-5a6 6 0 0 1 12 0v5" fill="none" stroke="${stroke}" stroke-width="3.4" stroke-linecap="round"/></g>`;
const sparkPath = 'M0 -15 L3.4 -3.4 15 0 3.4 3.4 0 15 -3.4 3.4 -15 0 -3.4 -3.4z';
/* a clip-revealed group: animate #<id>-c width */
const rev = (id, x, y, h, inner) => `<clipPath id="cp-${id}"><rect id="${id}-c" x="${x}" y="${y}" width="0" height="${h}"/></clipPath><g id="${id}" clip-path="url(#cp-${id})">${inner}</g>`;
/* a transcript line: sound bars that get wiped into text */
const barSets = [];
function tline(id, x, y, text, fs, W, col) {
  const bw = fs * 0.2, gap = fs * 0.2, n = Math.floor(W / (bw + gap));
  let bars = '';
  for (let i = 0; i < n; i++) bars += `<rect x="${x + i * (bw + gap)}" width="${bw}" rx="${bw / 2}" fill="${col}" opacity=".5"/>`;
  barSets.push({ id, y: y - fs * 0.33, h: fs * 0.95, seed: barSets.length * 13.7 });
  return `<clipPath id="cp-${id}-b"><rect id="${id}-bc" x="${x}" y="${y - fs * 1.1}" width="0" height="${fs * 1.6}"/></clipPath>
    <g id="${id}-bars" clip-path="url(#cp-${id}-b)">${bars}</g>
    ${rev(id, x - 2, y - fs * 1.1, fs * 1.6, `<text class="ui" x="${x}" y="${y}" font-size="${fs}">${text}</text>`)}`;
}

/* ---------- content */
const MAC_LINES = [
  ['Let’s ship the pilot Friday.', C.clay],
  ['I’ll draft the rollout plan.', C.leafMid],
  ['Who owns the checklist?', C.clay],
  ['Me. Done by Wednesday.', C.leafMid],
];
const MAC_NOTES = ['Ship the pilot on Friday', 'Jonas drafts the rollout plan', 'Checklist due Wednesday'];
const PH_LINES = [['Budget cap, 18k?', C.clay], ['Benchtop: six weeks.', C.leafMid], ['Plumber on Monday.', C.clay]];
const PH_NOTES = ['Budget capped at 18k', 'Benchtop in six weeks', 'Plumber on Monday'];
const LY = [-72, -8, 56, 120], NY = [8, 66, 124];
const PY = [8, 58, 108], PNY = [-88, -40, 8];
const ML_W = 440, PL_W = 206;

function bust(cx, cy, skin, hair, top, hairStyle) {
  const h = hairStyle === 'bob'
    ? `<circle cx="${cx}" cy="${cy - 26}" r="50" fill="${hair}"/><rect x="${cx - 50}" y="${cy - 26}" width="100" height="46" rx="14" fill="${hair}"/>`
    : `<path d="M${cx - 44} ${cy - 22}a44 46 0 0 1 88 0z" fill="${hair}"/>`;
  return `${hairStyle === 'bob' ? h : ''}<path d="M${cx - 104} ${cy + 124}q0 -92 104 -92q104 0 104 92z" fill="${top}"/>
    <rect x="${cx - 15}" y="${cy + 6}" width="30" height="34" rx="12" fill="${skin}"/>
    <circle cx="${cx}" cy="${cy - 18}" r="40" fill="${skin}"/>${hairStyle === 'bob' ? '' : h}`;
}
function tile(id, x, fill, inner) {
  return `<g id="${id}"><clipPath id="cp-${id}"><rect x="${x}" y="-148" width="284" height="246" rx="20"/></clipPath>
    <rect x="${x}" y="-148" width="284" height="246" rx="20" fill="${fill}"/>
    <g clip-path="url(#cp-${id})">${inner}</g>
    <rect id="${id}-ring" x="${x + 3}" y="-145" width="278" height="240" rx="17" fill="none" stroke="${C.leaf}" stroke-width="6"/>
    <g id="${id}-bars" transform="translate(${x + 26} 70)">${[0, 1, 2, 3].map((i) => `<rect x="${i * 9}" width="5" rx="2.5" fill="${C.rice}"/>`).join('')}</g></g>`;
}

function qr() {
  const n = 21, m = 12, o = -(n * m) / 2;
  let s = `<rect x="${o - 18}" y="${o - 18}" width="${n * m + 36}" height="${n * m + 36}" rx="18" fill="${C.rice}" stroke="rgba(31,26,20,.14)" stroke-width="2"/>`;
  const finder = (i, j) => (i < 8 && j < 8) || (i > 12 && j < 8) || (i < 8 && j > 12);
  for (let i = 0; i < n; i++) for (let j = 0; j < n; j++) {
    if (finder(i, j)) continue;
    if (hash(i * 3.1 + 7, j * 1.7 + 3) > 0.52) s += `<rect x="${o + i * m}" y="${o + j * m}" width="${m}" height="${m}" rx="2.5" fill="${C.ink}"/>`;
  }
  [[0, 0], [14, 0], [0, 14]].forEach(([i, j]) => {
    const x = o + i * m, y = o + j * m;
    s += `<rect x="${x}" y="${y}" width="84" height="84" rx="16" fill="${C.ink}"/><rect x="${x + 12}" y="${y + 12}" width="60" height="60" rx="9" fill="${C.rice}"/><rect x="${x + 24}" y="${y + 24}" width="36" height="36" rx="6" fill="${C.leaf}"/>`;
  });
  return s;
}

const mac = () => `
<g id="mac">
  <rect x="-330" y="-215" width="660" height="430" rx="26" fill="${C.ink}"/>
  <rect x="-312" y="-197" width="624" height="394" rx="10" fill="${C.rice}"/>
  <g clip-path="url(#macClip)">
    <g id="mCall">
      ${tile('tileA', -292, C.leafSoft, bust(-150, -8, '#c98a5e', '#2b211a', C.leaf, 'bob'))}
      ${tile('tileB', 8, C.cream3, bust(150, -8, '#8c5a3b', '#2b211a', C.clay, 'crop'))}
      <g id="mCtl"><circle cx="-56" cy="148" r="19" fill="${C.cream3}"/><circle cx="0" cy="148" r="19" fill="${C.cream3}"/><circle cx="56" cy="148" r="19" fill="${C.ink}"/>
        <rect x="-60" y="140" width="8" height="16" rx="4" fill="${C.ink2}"/><rect x="-9" y="142" width="18" height="12" rx="3" fill="${C.ink2}"/><rect x="47" y="145" width="18" height="6" rx="3" fill="${C.cream}"/></g>
    </g>
    <g id="mPill"><rect width="170" height="34" rx="17" fill="${C.ink}"/><g id="mPillDia">${dia(0, 0, 11, C.chili)}</g><text x="40" y="23" font-size="17" font-weight="600" fill="${C.cream}" style="fill:${C.cream}">Listening</text></g>
    ${MAC_LINES.map((l, i) => tline('ml' + i, -252, LY[i], l[0], 30, ML_W, l[1])).join('')}
    <g id="mNotes">
      ${rev('mEy', -264, -149, 30, `<text class="eyebrow" x="-262" y="-128" font-size="15" style="fill:${C.ink3}">AI NOTES · CLAUDE CODE</text>`)}
      ${rev('mTitle', -296, -106, 62, `<text class="serif" x="-292" y="-58" font-size="46">Pilot rollout</text>`)}
      ${MAC_NOTES.map((s, i) => rev('mb' + i, -258, NY[i] - 30, 44, `<text class="ui" x="-254" y="${NY[i]}" font-size="28">${s}</text>`)).join('')}
    </g>
    ${MAC_LINES.map((l, i) => `<rect id="md${i}" x="-9" y="-9" width="18" height="18" rx="9" fill="${l[1]}"/>`).join('')}
    <rect id="mFlash" x="-312" y="-197" width="624" height="394" fill="${C.leafSoft}"/>
    <g id="mQR"><rect x="-312" y="-197" width="624" height="394" fill="${C.cream2}"/><g transform="scale(1.08)">${qr()}</g></g>
  </g>
  <path d="M-400 215h800v6a14 14 0 0 1 -14 14h-772a14 14 0 0 1 -14 -14z" fill="${C.alu}"/>
  <rect x="-62" y="215" width="124" height="8" rx="4" fill="${C.aluDeep}"/>
</g>`;

const phone = () => `
<g id="phone">
  <rect x="-145" y="-295" width="290" height="590" rx="56" fill="${C.ink}"/>
  <rect x="-131" y="-281" width="262" height="562" rx="43" fill="${C.cream}"/>
  <g clip-path="url(#phoneClip)">
    <g id="pEyDia">${dia(0, 0, 10, C.chili)}</g>
    <text id="pEyA" class="eyebrow" x="-94" y="-207" font-size="12.5" style="fill:${C.ink3}">IN THE ROOM</text>
    <text id="pEyB" class="eyebrow" x="-94" y="-207" font-size="12.5" style="fill:${C.ink3}">AI NOTES</text>
    <text id="pTitle" class="serif" x="-112" y="-158" font-size="33">Kitchen quote</text>
    <text id="pTimer" class="serif num" x="-114" y="-84" font-size="66">0:00</text>
    <g id="pMeter">${Array.from({ length: 9 }, (_, i) => `<rect class="pm" x="-6" y="-6" width="12" height="12" rx="1.6" transform="translate(${-104 + i * 23} -48) rotate(45)"/>`).join('')}</g>
    ${PH_LINES.map((l, i) => tline('pl' + i, -90, PY[i], l[0], 21, PL_W, l[1])).join('')}
    ${PH_NOTES.map((s, i) => rev('pb' + i, -94, PNY[i] - 24, 34, `<text class="ui" x="-90" y="${PNY[i]}" font-size="20">${s}</text>`)).join('')}
    ${PH_LINES.map((l, i) => `<rect id="pd${i}" x="-7" y="-7" width="14" height="14" rx="7" fill="${l[1]}"/>`).join('')}
    <g id="pStart"><rect x="-100" y="194" width="200" height="54" rx="27" fill="${C.leaf}"/><text x="0" y="228" font-size="18" font-weight="600" text-anchor="middle" style="fill:${C.rice}">Start listening</text></g>
    <g id="pStop"><rect x="-106" y="194" width="212" height="54" rx="27" fill="${C.chili}"/><rect x="-86" y="214" width="14" height="14" rx="3.5" fill="${C.rice}"/><text x="-62" y="227" font-size="16.5" font-weight="600" style="fill:${C.rice}">Stop &amp; write notes</text></g>
    <circle id="pTap" cx="0" cy="221" r="30" fill="${C.rice}"/>
    <rect id="pFlash" x="-131" y="-281" width="262" height="562" fill="${C.leafSoft}"/>
    <g id="pCode"><rect x="-131" y="-281" width="262" height="562" fill="${C.cream2}"/>
      <text class="eyebrow" x="0" y="-70" font-size="13" text-anchor="middle" style="fill:${C.ink3}">CODES MATCH?</text>
      <text class="serif num" x="0" y="-8" font-size="50" text-anchor="middle">482 913</text>
      <g id="pCheck" ><circle r="30" fill="${C.leaf}"/><path d="M-12 1l8 8 16-17" fill="none" stroke="${C.rice}" stroke-width="5" stroke-linecap="round" stroke-linejoin="round"/></g>
    </g>
  </g>
  <rect id="island" x="-42" y="-268" width="84" height="26" rx="13" fill="#0e0c09"/>
  <g id="islandIn">${dia(-48, -255, 9, C.chili)}${[0, 1, 2, 3].map((i) => `<rect class="ib" x="${32 + i * 6}" width="3.4" rx="1.7" fill="${C.cream}"/>`).join('')}</g>
</g>`;

const bubble = (id, fill, flip) => `<g id="${id}"><path d="M-70 -46h140a22 22 0 0 1 22 22v48a22 22 0 0 1 -22 22h${flip ? -12 : -128}l${flip ? 30 : -30} 30l${flip ? -52 : 6} -30h${flip ? -106 : -18}a22 22 0 0 1 -22 -22v-48a22 22 0 0 1 22 -22z" fill="${fill}"/>
  <g class="bb">${[0, 1, 2, 3, 4, 5, 6].map((i) => `<rect x="${-45 + i * 14}" width="7" rx="3.5" fill="${C.rice}"/>`).join('')}</g></g>`;

const chip = (id, x, y, w, label) => `<g id="${id}" data-x="${x}" data-y="${y}">
  <rect x="${-w / 2}" y="-27" width="${w}" height="54" rx="27" fill="${C.rice}" stroke="rgba(31,26,20,.2)" stroke-width="2"/>
  <text x="0" y="8.5" font-size="24" font-weight="500" text-anchor="middle">${label}</text>
  <g id="${id}-on"><rect x="${-w / 2}" y="-27" width="${w}" height="54" rx="27" fill="${C.leaf}"/><text x="0" y="8.5" font-size="24" font-weight="500" text-anchor="middle" style="fill:${C.rice}">${label}</text></g></g>`;

const vignette = (side, inner, props) => `
<g id="vig${side}">
  <circle r="360" fill="${C.disc}"/>
  <circle id="tint${side}" r="360" fill="${C.leafSoft}"/>
  <g clip-path="url(#discClip)"><g id="props${side}"><rect x="-400" y="150" width="800" height="260" fill="${C.desk}"/><rect x="-400" y="150" width="800" height="12" fill="#d3bf92"/>${props}</g>
    <ellipse id="shad${side}" cx="0" cy="${side === 'L' ? 190 : 292}" rx="${side === 'L' ? 290 : 130}" ry="11" fill="${C.ink}" fill-opacity=".1"/></g>
  ${inner}
  <g id="ringG${side}"><g mask="url(#ringMask${side})"><circle r="360" fill="none" stroke="${C.leaf}" stroke-width="5" stroke-dasharray="3 15" stroke-linecap="round"/></g>
  <g id="lock${side}" ><circle r="31" fill="${C.leaf}"/>${lockGlyph(C.rice, 1.05)}</g></g>
</g>`;

const CODE = [
  [0, `<tspan fill="#8f8573">// the whole pipeline, in the open</tspan>`],
  [0, `<tspan fill="#d97a4c">fn</tspan> transcribe(audio: &amp;<tspan fill="#8cc084">Audio</tspan>) -&gt; <tspan fill="#8cc084">Transcript</tspan> {`],
  [1, `<tspan fill="#d97a4c">let</tspan> model = <tspan fill="#8cc084">Speech</tspan>::on_this_device();`],
  [1, `model.run(audio) <tspan fill="#8f8573">// nothing uploaded</tspan>`],
  [0, `}`],
  [0, ``],
  [0, `<tspan fill="#d97a4c">fn</tspan> write_notes(t: &amp;<tspan fill="#8cc084">Transcript</tspan>, ai: <tspan fill="#8cc084">YourAi</tspan>) {`],
  [1, `ai.summarise(t) <tspan fill="#8f8573">// your pick</tspan>`],
  [0, `}`],
  [0, `<tspan fill="#8f8573">// no account. no telemetry. MIT.</tspan>`],
];
const STATEMENTS = ['No account.', 'No server of ours.', 'You choose the AI.'];
const CAPTIONS = [
  [4.9, 8.2, 'On a call. <tspan class="em">In the room.</tspan>'],
  [8.5, 12.2, 'Milkrice listens. No bot joins.'],
  [13.0, 17.0, 'Sound becomes words, on the device.'],
  [17.3, 21.2, 'The audio <tspan class="em">never</tspan> leaves it.'],
  [21.7, 26.0, 'Then <tspan class="em">your</tspan> AI writes the notes.'],
  [26.3, 30.2, 'Cut into diamonds, ready to read.'],
  [31.3, 34.4, 'Pair once with a QR code.'],
  [34.7, 39.3, '<tspan class="em">Encrypted</tspan> before it leaves.'],
  [39.6, 43.5, 'Your drive sees only blobs.'],
];
const DRIVE_ROWS = ['a3f9c2e1-77d0.rec', '9f2c41a7-3be0.rec', '0b7d5c92-e4a8.rec'];
const ROW_Y = [436, 482, 528];

/* plate mark for the end card */
const LAT = [];
for (let i = -1; i <= 1; i++) for (let j = -1; j <= 1; j++) LAT.push([(i - j) * 62 * 0.7071, (i + j) * 62 * 0.7071]);

svg.innerHTML = `
<defs>
  <clipPath id="discClip"><circle r="360"/></clipPath>
  <clipPath id="macClip"><rect x="-312" y="-197" width="624" height="394" rx="10"/></clipPath>
  <clipPath id="phoneClip"><rect x="-131" y="-281" width="262" height="562" rx="43"/></clipPath>
  ${['L', 'R'].map((s) => `<mask id="ringMask${s}" maskUnits="userSpaceOnUse" x="-400" y="-400" width="800" height="800"><circle id="ringDraw${s}" r="360" fill="none" stroke="#fff" stroke-width="26" pathLength="1" stroke-dasharray="1 1" transform="rotate(-90)"/></mask>`).join('')}
  <clipPath id="plateClip"><circle r="150"/></clipPath>
</defs>
<rect width="1920" height="1080" fill="${C.cream}"/>

<g id="s0">${['Your meetings.', 'Your devices.', 'Your AI.'].map((s, i) => `<text id="in${i}" class="serif ${i === 2 ? 'em' : ''}" x="960" y="0" font-size="132" text-anchor="middle">${s}</text>`).join('')}</g>

<g id="world">
${vignette('L', mac(), `
  <g id="mug"><path d="M-23 -54h46v42a12 12 0 0 1 -12 12h-22a12 12 0 0 1 -12 -12z" fill="${C.rice}"/><path d="M23 -42q22 0 22 15t-22 15" fill="none" stroke="${C.rice}" stroke-width="8" stroke-linecap="round"/>
    <path class="steam" fill="none" stroke="${C.ink3}" stroke-width="3.5" stroke-linecap="round" opacity=".35"/><path class="steam" fill="none" stroke="${C.ink3}" stroke-width="3.5" stroke-linecap="round" opacity=".35"/></g>
  <g id="plant"><g class="lf"><ellipse cx="0" cy="-58" rx="19" ry="58" fill="${C.leafDeep}"/></g><g class="lf"><ellipse cx="0" cy="-50" rx="17" ry="50" fill="${C.leaf}"/></g><g class="lf"><ellipse cx="0" cy="-46" rx="16" ry="46" fill="${C.leafMid}"/></g>
    <path d="M-34 -52h68l-9 52h-50z" fill="${C.clay}"/><rect x="-38" y="-58" width="76" height="14" rx="5" fill="#b3541f"/></g>`)}
${vignette('R', phone(), `
  <g id="cup"><rect x="-40" y="-6" width="80" height="6" rx="3" fill="${C.rice}"/><path d="M-27 -50h54q0 44 -27 44q-27 0 -27 -44z" fill="${C.leaf}"/><path d="M26 -42q20 0 20 12t-22 12" fill="none" stroke="${C.leaf}" stroke-width="7" stroke-linecap="round"/>
    <path class="steam" fill="none" stroke="${C.ink3}" stroke-width="3.5" stroke-linecap="round" opacity=".35"/><path class="steam" fill="none" stroke="${C.ink3}" stroke-width="3.5" stroke-linecap="round" opacity=".35"/></g>
  <g id="plate"><rect x="-56" y="-8" width="112" height="8" rx="4" fill="${C.clay}"/><rect x="-44" y="-13" width="88" height="6" rx="3" fill="${C.leaf}"/>
    ${dia(-24, -27, 22, C.rice)}${dia(24, -27, 22, C.rice)}${dia(0, -27, 22, C.rice)}${dia(-12, -43, 22, C.rice)}${dia(12, -43, 22, C.rice)}<circle cx="0" cy="-62" r="6" fill="${C.chili}"/></g>`)}
${bubble('bubA', C.clay, false)}${bubble('bubB', C.leaf, true)}

<g id="labels12">
  <text id="labL" x="540" y="826" font-size="25" font-weight="600" text-anchor="middle" style="fill:${C.ink2}">Mac</text>
  <text id="labR" x="1380" y="826" font-size="25" font-weight="600" text-anchor="middle" style="fill:${C.ink2}">iPhone <tspan font-weight="400" style="fill:${C.ink3}">· in beta</tspan></text>
</g>
<g id="chips">
  ${chip('chip0', 345, 838, 196, 'Claude Code')}${chip('chip1', 522, 838, 126, 'Codex')}${chip('chip2', 717, 838, 232, 'Ollama · offline')}
  ${chip('chip3', 1380, 838, 400, 'Apple Intelligence · on-device')}
</g>
<path id="sparkL" d="${sparkPath}" fill="${C.clay}"/><path id="sparkR" d="${sparkPath}" fill="${C.clay}"/>

<g id="s4">
  <g id="soon"><rect x="-196" y="-30" width="392" height="60" rx="30" fill="${C.clay}"/>${dia(-162, 0, 13, C.rice)}<text x="14" y="9" font-size="25" font-weight="600" text-anchor="middle" style="fill:${C.rice}">Optional sync · coming soon</text></g>
  <path id="trackL" d="M574 434H690" fill="none" stroke="${C.ink3}" stroke-width="3" stroke-dasharray="2 12" stroke-linecap="round" stroke-opacity=".6"/>
  <path id="trackR" d="M1230 434H1346" fill="none" stroke="${C.ink3}" stroke-width="3" stroke-dasharray="2 12" stroke-linecap="round" stroke-opacity=".6"/>
  <g id="drive">
    <rect x="740" y="238" width="440" height="336" rx="30" fill="${C.rice}" stroke="rgba(31,26,20,.14)" stroke-width="2"/>
    <path d="M776 272h22l7 8h29a6 6 0 0 1 6 6v24a6 6 0 0 1 -6 6h-58a6 6 0 0 1 -6 -6v-32a6 6 0 0 1 6 -6z" fill="${C.leaf}"/>
    <text class="serif" x="856" y="303" font-size="31">Your iCloud Drive</text>
    <text x="776" y="350" font-size="18.5" style="fill:${C.ink3}">or Google Drive, or any folder you pick</text>
    <path d="M776 374H1144" stroke="rgba(31,26,20,.12)" stroke-width="2"/>
    <text class="eyebrow" x="776" y="404" font-size="13" style="fill:${C.ink3}">WHAT THE DRIVE SEES</text>
    ${DRIVE_ROWS.map((n, i) => `<g id="row${i}"><rect id="row${i}-hl" x="762" y="${ROW_Y[i] - 27}" width="396" height="40" rx="12" fill="${C.leafSoft}"/><g transform="translate(786 ${ROW_Y[i] - 9})">${lockGlyph(C.ink2, 0.68)}</g><text class="mono" x="808" y="${ROW_Y[i]}" font-size="19.5" style="fill:${C.ink2}">${n}</text><text x="1144" y="${ROW_Y[i]}" font-size="16.5" text-anchor="end" style="fill:${C.ink3}">4 KB</text></g>`).join('')}
  </g>
  <g id="labels4">
    ${[[300, 'Mac', ''], [1620, 'iPhone', ' · in beta']].map(([x, a, b]) => `<text x="${x}" y="762" font-size="27" font-weight="600" text-anchor="middle" style="fill:${C.ink2}">${a}<tspan font-weight="400" style="fill:${C.ink3}">${b}</tspan></text>
      <g transform="translate(${x - 92} 802)"><path d="M0 0m-8 0a8 8 0 1 0 16 0a8 8 0 1 0 -16 0M8 0h20M21 0v7M28 0v6" fill="none" stroke="${C.leaf}" stroke-width="3" stroke-linecap="round"/><text x="42" y="7.5" font-size="21" style="fill:${C.leaf}" font-weight="500">key stays here</text></g>`).join('')}
  </g>
  <g id="parcel">
    <g id="pcCard"><rect x="-38" y="-46" width="76" height="92" rx="12" fill="${C.rice}" stroke="rgba(31,26,20,.2)" stroke-width="2"/>${[-22, -2, 18].map((y) => `${dia(-20, y, 9, C.leaf)}<rect x="-8" y="${y - 3.5}" width="${y === 18 ? 22 : 32}" height="7" rx="3.5" fill="${C.cream3}"/>`).join('')}</g>
    <g id="pcDia">${dia(0, 0, 62, C.leafDeep)}<g id="pcLock" >${lockGlyph(C.rice, 1.05)}</g></g>
  </g>
  <g id="pcLabel"><rect x="-128" y="-21" width="256" height="42" rx="21" fill="${C.rice}" stroke="rgba(31,26,20,.18)" stroke-width="2"/><text id="pcText" class="mono" x="0" y="7" font-size="19.5" text-anchor="middle"></text></g>
  <g id="parcel2">${dia(0, 0, 50, C.leafDeep)}${lockGlyph(C.rice, 0.85)}</g>
</g>
</g>

<g id="caps">${CAPTIONS.map((c, i) => `<text id="cap${i}" class="serif" x="960" y="0" font-size="64" text-anchor="middle">${c[2]}</text>`).join('')}</g>

<circle id="iris1" cx="960" cy="540" r="0" fill="${C.band}"/>
<g id="s5">
  <g id="lattice" opacity=".1">${(() => { let s = ''; for (let i = 0; i < 5; i++) for (let j = 0; j < 5; j++) s += `<rect x="-52" y="-52" width="104" height="104" rx="12" fill="none" stroke="${C.bandInk}" stroke-width="3" transform="translate(${1380 + (i - j) * 84} ${540 + (i + j - 4) * 84}) rotate(45)"/>`; return s; })()}</g>
  ${STATEMENTS.map((s, i) => `<clipPath id="cp-st${i}"><rect x="200" y="${404 + i * 150 - 118}" width="1300" height="150"/></clipPath><g clip-path="url(#cp-st${i})"><g id="st${i}">${dia(250, 404 + i * 150 - 36, 26, i === 2 ? C.chili : C.bandInk2)}<text class="serif" x="300" y="${404 + i * 150}" font-size="112" style="fill:${C.bandInk}">${s}</text></g></g>`).join('')}
  <g id="os">
    <text id="os0" class="serif" x="170" y="470" font-size="108" style="fill:${C.bandInk}">Open source.</text>
    <text id="os1" class="serif" x="170" y="596" font-size="108" style="fill:${C.bandInk}; font-style: italic">Read every line.</text>
    <text id="os2" x="176" y="680" font-size="27" style="fill:${C.bandInk2}">MIT licensed · github.com/thekindofme/milkrice</text>
  </g>
  <g id="term">
    <rect x="0" y="0" width="760" height="600" rx="26" fill="${C.term}"/>
    <circle cx="38" cy="38" r="7" fill="#3a352c"/><circle cx="62" cy="38" r="7" fill="#3a352c"/><circle cx="86" cy="38" r="7" fill="#3a352c"/>
    ${CODE.map((l, i) => `<clipPath id="cp-cl${i}"><rect id="cl${i}-c" x="40" y="${92 + i * 48}" width="0" height="46"/></clipPath><g clip-path="url(#cp-cl${i})"><text id="cl${i}" class="mono" x="${46 + l[0] * 56}" y="${124 + i * 48}" font-size="23.5" style="fill:${C.termInk}">${l[1]}</text></g>`).join('')}
    <rect id="caret" x="0" y="0" width="12" height="30" rx="2" fill="${C.clay}"/>
  </g>
</g>

<circle id="iris2" cx="960" cy="540" r="0" fill="${C.cream}"/>
<g id="s6">
  <g id="mark">
    <circle id="mkPlate" r="170" fill="${C.clay}"/><circle id="mkRim" r="158" fill="#b3541f"/>
    <g id="mkLeaf"><circle r="146" fill="${C.leaf}"/><g clip-path="url(#plateClip)" stroke="${C.leafDeep}" stroke-width="3" opacity=".45" stroke-linecap="round">${Array.from({ length: 13 }, (_, i) => `<path d="M${-220 + i * 36} 150L${-140 + i * 36} -150"/>`).join('')}</g></g>
    ${LAT.map((p, i) => `<rect id="mk${i}" x="-27" y="-27" width="54" height="54" rx="7" fill="${C.rice}"/>`).join('')}
    <circle id="mkChili" r="15" fill="${C.chili}"/>
  </g>
  <text id="e0" class="serif" x="960" y="0" font-size="136" text-anchor="middle">Milkrice</text>
  <text id="e1" class="serif" x="960" y="0" font-size="46" text-anchor="middle" style="fill:${C.ink2}">Your meetings, your devices, <tspan class="em">your AI.</tspan></text>
  <text id="e2" x="960" y="0" font-size="29" font-weight="500" text-anchor="middle" style="fill:${C.leaf}; letter-spacing:.01em">milkrice.yasithfernando.com</text>
  <text id="e3" class="eyebrow" x="960" y="0" font-size="17" text-anchor="middle" style="fill:${C.ink3}">OPEN SOURCE · MAC · IPHONE IN BETA</text>
</g>
<rect id="fade" width="1920" height="1080" fill="${C.cream}"/>
`;

/* ---------- animation registry */
const reg = new Map();
const fxs = [];
function A(sel, props) {
  const list = typeof sel === 'string' ? $$(sel) : [sel];
  if (!list.length) console.warn('no match', sel);
  list.forEach((el) => {
    let st = reg.get(el);
    if (!st) { st = { el, p: {} }; reg.set(el, st); }
    for (const k in props) {
      const v = props[k];
      st.p[k] = Array.isArray(v) ? K(v) : typeof v === 'function' ? v : () => v;
    }
  });
}
const fx = (fn) => fxs.push(fn);
const ATTR = { w: 'width', h: 'height', rx: 'rx', rad: 'r', ax: 'x', ay: 'y' };
function apply(t) {
  reg.forEach(({ el, p }) => {
    if (p.o) {
      const o = p.o(t);
      if (o <= 0.001) { if (el.style.display !== 'none') el.style.display = 'none'; return; }
      if (el.style.display === 'none') el.style.display = '';
      el.style.opacity = o >= 0.999 ? '' : o.toFixed(3);
    }
    if (p.x || p.y || p.s || p.r || p.sx || p.sy) {
      const x = p.x ? p.x(t) : 0, y = p.y ? p.y(t) : 0, r = p.r ? p.r(t) : 0;
      const s = p.s ? p.s(t) : 1, sx = p.sx ? p.sx(t) : s, sy = p.sy ? p.sy(t) : s;
      el.setAttribute('transform', `translate(${x.toFixed(2)} ${y.toFixed(2)})${r ? ` rotate(${r.toFixed(2)})` : ''}${sx !== 1 || sy !== 1 ? ` scale(${sx.toFixed(4)} ${sy.toFixed(4)})` : ''}`);
    }
    for (const k in ATTR) if (p[k]) { const v = p[k](t); el.setAttribute(ATTR[k], (k === 'ax' || k === 'ay' ? v : Math.max(0, v)).toFixed(2)); }
    if (p.draw) el.setAttribute('stroke-dashoffset', (1 - p.draw(t)).toFixed(4));
    if (p.fill) el.setAttribute('fill', p.fill(t));
  });
  for (const f of fxs) f(t);
}
/* helpers */
const pop = (sel, t, x, y, d = 0.5, s1 = 1) => A(sel, { x, y, s: [[t, 0], [t + d, s1, 'back']], o: [[t - 0.01, 0], [t, 1]] });
const riseIn = (t, y, dy = 30, d = 0.8) => [[t, y + dy], [t + d, y, 'o5']];
const fadeIn = (t, d = 0.4) => [[t, 0], [t + d, 1, 'out']];

/* ================= SCENE 0: the line ================= */
[0, 1, 2].forEach((i) => {
  const t = 0.35 + i * 0.6, y = 400 + i * 150, out = 3.55 + i * 0.07;
  A('#in' + i, { y: [[t, y + 60], [t + 1.0, y, 'o5'], [out, y], [out + 0.6, y - 50, 'in']], o: [[t, 0], [t + 0.5, 1, 'out'], [out, 1], [out + 0.5, 0, 'in']] });
});

/* ================= vignettes: placement through scenes 1–4 ================= */
const VL = { x: 540, y: 410 }, VR = { x: 1380, y: 410 };
const T4 = 30.6;   // pull back for sync
A('#vigL', { x: [[T4, VL.x], [T4 + 1.4, 300, 'io5']], y: [[T4, VL.y], [T4 + 1.4, 440, 'io5']], s: [[4.0, 0], [5.1, 1, 'o5'], [T4, 1], [T4 + 1.4, 0.72, 'io5']], o: [[3.99, 0], [4.0, 1], [44.95, 1], [45, 0]] });
A('#vigR', { x: [[T4, VR.x], [T4 + 1.4, 1620, 'io5']], y: [[T4, VR.y], [T4 + 1.4, 440, 'io5']], s: [[4.18, 0], [5.28, 1, 'o5'], [T4, 1], [T4 + 1.4, 0.72, 'io5']], o: [[4.17, 0], [4.18, 1], [44.95, 1], [45, 0]] });

const T2 = 12.4;   // props clear, devices grow
A('#mac', { x: 0, y: [[4.4, 70], [5.4, 0, 'o5'], [T2 + 0.1, 0], [T2 + 1.3, -8, 'io']], s: [[T2 + 0.1, 0.64], [T2 + 1.3, 0.78, 'io']], o: fadeIn(4.4, 0.45) });
A('#phone', { x: 0, y: [[4.6, 0], [5.6, -68, 'o5'], [T2 + 0.2, -68], [T2 + 1.4, 0, 'io']], s: [[T2 + 0.2, 0.74], [T2 + 1.4, 0.95, 'io']], o: fadeIn(4.6, 0.45) });
A('#propsL', { y: [[T2, 0], [T2 + 0.7, 40, 'in']], o: [[T2, 1], [T2 + 0.6, 0, 'in']] });
A('#propsR', { y: [[T2 + 0.1, 0], [T2 + 0.8, 40, 'in']], o: [[T2 + 0.1, 1], [T2 + 0.7, 0, 'in']] });
A('#shadL', { o: [[T2 + 0.9, 0], [T2 + 1.5, 1]] });
A('#shadR', { o: [[T2 + 1.0, 0], [T2 + 1.6, 1]] });
pop('#mug', 5.0, -292, 150); pop('#plant', 5.15, 292, 150);
pop('#cup', 5.2, -218, 150); pop('#plate', 5.35, 222, 150);
A('.lf', {}); // registered below through fx
fx((t) => {
  if (t < 4 || t > 13.2) return;
  $$('#plant .lf').forEach((el, i) => el.setAttribute('transform', `rotate(${[-30, 4, 34][i] + Math.sin(t * 1.3 + i * 1.9) * 2.4})`));
  $$('.steam').forEach((el, i) => {
    const k = i % 2, w = Math.sin(t * 2.1 + i * 2.3) * 5, x = k ? 8 : -8;
    el.setAttribute('d', `M${x} -64q${7 + w} -9 0 -18t${-w * 0.6} -18`);
    el.setAttribute('opacity', (0.22 + 0.13 * Math.sin(t * 1.7 + i)).toFixed(3));
  });
});

/* who is speaking (two voices alternating) */
const spA = (t) => sstep(-0.15, 0.45, Math.sin((t - 4.7) * Math.PI / 1.85));
const spB = (t) => sstep(-0.15, 0.45, -Math.sin((t - 4.7) * Math.PI / 1.85));
const wob = (i, t, seed = 0) => 0.25 + 0.75 * Math.abs(Math.sin(i * 1.73 + seed + t * 7.3) * Math.sin(i * 0.61 + seed * 0.7 + t * 4.1));

/* speech bubbles lean in from outside the café disc */
[['#bubA', 1380 - 262, 410 - 196, spA, 5.5], ['#bubB', 1380 + 258, 410 - 250, spB, 5.8]].forEach(([sel, x, y, sp, t0]) => {
  A(sel, { x, y: (t) => y + Math.sin(t * 1.1 + x) * 4, s: (t) => K([[t0, 0], [t0 + 0.5, 1, 'back'], [12.3, 1], [12.75, 0, 'in']])(t) * (0.94 + 0.06 * sp(t)), o: [[t0 - 0.01, 0], [t0, 1], [12.74, 1], [12.75, 0]] });
  fx((t) => { if (t < 5 || t > 12.8) return; $$(sel + ' .bb rect').forEach((r, i) => { const h = 8 + 44 * sp(t) * wob(i, t, x); r.setAttribute('height', h.toFixed(1)); r.setAttribute('y', (-h / 2).toFixed(1)); }); });
});

/* ---------- Mac screen */
const TL = 7.0;     // both start listening
const T3 = 21.5;    // AI writes the notes
A('#mCall', { x: [[T2 + 0.2, 0], [T2 + 1.2, 236, 'io']], y: [[T2 + 0.2, 0], [T2 + 1.2, -160, 'io']], s: [[T2 + 0.2, 1], [T2 + 1.2, 0.2, 'io']], o: [[T3 + 0.1, 1], [T3 + 0.5, 0]] });
A('#mCtl', { o: [[T2 + 0.2, 1], [T2 + 0.6, 0]] });
A('#tileA-ring', { o: (t) => (t > 4.6 ? spA(t) : 0) });
A('#tileB-ring', { o: (t) => (t > 4.6 ? spB(t) : 0) });
fx((t) => {
  if (t < 4 || t > T3 + 0.6) return;
  [['#tileA-bars', spA], ['#tileB-bars', spB]].forEach(([sel, sp]) => $$(sel + ' rect').forEach((r, i) => { const h = 5 + 24 * sp(t) * wob(i, t, 3); r.setAttribute('height', h.toFixed(1)); r.setAttribute('y', (-h / 2).toFixed(1)); }));
});
A('#mPill', { x: -292, y: [[TL, -250], [TL + 0.7, -191, 'o5']], o: [[TL - 0.01, 0], [TL, 1], [T3 + 0.1, 1], [T3 + 0.5, 0]] });
A('#mPillDia', { x: 21, y: 17, s: (t) => 1 + 0.18 * Math.sin(t * 5) });

/* transcript lines: bars arrive, then are wiped into words; later the words fold into the dot */
const M_T = [13.9, 15.05, 16.2, 17.35], P_T = [14.3, 15.75, 17.2];
const M_FOLD = (i) => T3 + 0.7 + i * 0.16, P_FOLD = (i) => T3 + 0.85 + i * 0.16;
const T_SPARK = 24.9, T_NOTE = 25.6, T_GATH = 23.75;
function lines(prefix, dots, times, W, fold, xs, ys, target, big, lat) {
  times.forEach((t0, i) => {
    const id = `#${prefix}${i}`;
    const p = K([[t0 + 0.45, 0], [t0 + 1.3, 1, 'io'], [fold(i), 1], [fold(i) + 0.55, 0, 'io']]);
    const q = K([[t0, 0], [t0 + 0.55, 1, 'out']]);
    A(id + '-c', { w: (t) => p(t) * (W + 6) });
    fx((t) => {
      const bc = $(id + '-bc'); if (!bc) return;
      const a = p(t) * W, b = q(t) * W;
      bc.setAttribute('x', (xs + a).toFixed(1)); bc.setAttribute('width', Math.max(0, t > fold(i) ? 0 : b - a).toFixed(1));
    });
    // the speaker dot → diamond → note bullet
    const f = fold(i) + 0.35, tg = target[i], y0 = ys[i] - (big ? 10 : 7), xd = xs - (big ? 28 : 21);
    const col = (prefix === 'ml' ? MAC_LINES : PH_LINES)[i][1];
    A(`#${dots}${i}`, {
      x: [[T_GATH + i * 0.06, xd], [T_GATH + 0.7 + i * 0.06, lat[i][0], 'io'], [T_NOTE, lat[i][0]], [T_NOTE + 0.75, tg[0], 'io']],
      y: [[T_GATH + i * 0.06, y0], [T_GATH + 0.7 + i * 0.06, lat[i][1], 'io'], [T_NOTE, lat[i][1]], [T_NOTE + 0.75, tg[1], 'io']],
      s: [[t0, 0], [t0 + 0.35, 1, 'back'], [f, 1], [f + 0.3, 1.25, 'out'], [f + 0.7, 1, 'io'], [T_GATH + i * 0.06, 1], [T_GATH + 0.7 + i * 0.06, 3, 'io'], [T_NOTE - 0.02, 3], [T_NOTE + 0.18, 3.5, 'out'], [T_NOTE + 0.75, tg[3] || 1, 'io']],
      r: [[f, 0], [f + 0.7, 135, 'io']], rx: [[f, big ? 9 : 7], [f + 0.5, 2.5, 'io']],
      o: [[t0 - 0.01, 0], [t0, 1]],
      fill: (t) => mix(col, tg[2], sstep(f, f + 0.6, t)),
    });
  });
}
lines('ml', 'md', M_T, ML_W, M_FOLD, -252, LY, [[-282, -134, C.leaf, 0.7], [-280, NY[0] - 10, C.leaf], [-280, NY[1] - 10, C.leaf], [-280, NY[2] - 10, C.leaf]], true, [[0, -50], [-50, 0], [50, 0], [0, 50]]);
lines('pl', 'pd', P_T, PL_W, P_FOLD, -90, PY, PNY.map((y) => [-108, y - 7, C.leaf]), false, [[-39, -10], [39, -10], [0, 29]]);
A('#md0', { fill: (t) => mix(mixN(C.clay, C.leaf, sstep(M_FOLD(0) + 0.35, M_FOLD(0) + 0.95, t)), C.chili, sstep(T_GATH, T_GATH + 0.5, t)) });
fx((t) => {
  if (t < 13.5 || t > T3 + 2.5) return;
  barSets.forEach((b) => $$(`#${b.id}-bars rect`).forEach((r, i) => { const h = b.h * wob(i, t, b.seed); r.setAttribute('height', h.toFixed(1)); r.setAttribute('y', (b.y - h / 2).toFixed(1)); }));
});
/* notes write in */
const T5 = 44.0;
A('#mNotes', { o: [[T_NOTE - 0.01, 0], [T_NOTE, 1]] });
A('#mEy-c', { w: [[T_NOTE + 0.15, 0], [T_NOTE + 0.75, 260, 'io']] });
A('#mTitle-c', { w: [[T_NOTE + 0.3, 0], [T_NOTE + 1.0, 320, 'io']] });
[0, 1, 2].forEach((i) => A(`#mb${i}-c`, { w: [[T_NOTE + 0.75 + i * 0.32, 0], [T_NOTE + 1.5 + i * 0.32, 450, 'io']] }));
[0, 1, 2].forEach((i) => A(`#pb${i}-c`, { w: [[T_NOTE + 0.95 + i * 0.32, 0], [T_NOTE + 1.65 + i * 0.32, 230, 'io']] }));

/* ---------- iPhone screen */
const eyS = K([[T_GATH, 1], [T_GATH + 0.7, 4.2, 'io'], [T_NOTE - 0.02, 4.2], [T_NOTE + 0.18, 4.9, 'out'], [T_NOTE + 0.75, 1, 'io']]);
A('#pEyDia', { x: [[T_GATH, -108], [T_GATH + 0.7, 0, 'io'], [T_NOTE, 0], [T_NOTE + 0.75, -108, 'io']], y: [[T_GATH, -211.5], [T_GATH + 0.7, -49, 'io'], [T_NOTE, -49], [T_NOTE + 0.75, -211.5, 'io']], s: (t) => eyS(t) * (t > TL && t < T3 ? 1 + 0.2 * Math.sin(t * 5 + 1) : 1) });
A('#pEyA', { o: [[T_NOTE, 1], [T_NOTE + 0.25, 0]] });
A('#pEyB', { o: [[T_NOTE + 0.2, 0], [T_NOTE + 0.5, 1]] });
A('#pTimer', { o: [[T3 + 0.1, 1], [T3 + 0.6, 0]], y: [[T3 + 0.1, 0], [T3 + 0.6, -10, 'in']] });
A('#pMeter', { o: [[T3 + 0.1, 1], [T3 + 0.6, 0]] });
A('#pStart', { o: [[TL, 1], [TL + 0.25, 0]] });
A('#pStop', { o: [[TL + 0.05, 0], [TL + 0.3, 1], [T3 + 0.1, 1], [T3 + 0.6, 0]], y: [[T3 + 0.1, 0], [T3 + 0.6, 30, 'in']] });
A('#pTap', { rad: [[TL - 0.25, 10], [TL + 0.35, 46, 'out']], o: [[TL - 0.26, 0], [TL - 0.25, 0.55], [TL + 0.35, 0, 'out']] });
const islandW = K([[TL + 0.1, 84], [TL + 0.7, 136, 'back'], [T3 + 0.2, 136], [T3 + 0.7, 84, 'io']]);
A('#island', { w: islandW, ax: (t) => -islandW(t) / 2 });
A('#islandIn', { o: [[TL + 0.4, 0], [TL + 0.7, 1], [T3 + 0.1, 1], [T3 + 0.3, 0]] });
const timerEl = $('#pTimer'), meter = $$('.pm'), ibars = $$('.ib');
let lastTimer = '';
fx((t) => {
  const s = Math.max(0, Math.floor(t - TL)), txt = `0:${String(Math.min(s, 59)).padStart(2, '0')}`;
  if (txt !== lastTimer) { timerEl.textContent = txt; lastTimer = txt; }
  const on = t > TL + 0.3 && t < T3 + 0.7;
  const lvl = on ? 2 + Math.round(6 * Math.abs(Math.sin(t * 2.9) * Math.sin(t * 1.3 + 1))) : 0;
  meter.forEach((m, i) => m.setAttribute('fill', i < lvl ? C.leaf : C.cream3));
  ibars.forEach((r, i) => { const h = 4 + 10 * wob(i, t, 9); r.setAttribute('height', h.toFixed(1)); r.setAttribute('y', (-255 - h / 2).toFixed(1)); });
});

/* ================= SCENE 2: the boundary ================= */
[['L', 13.4], ['R', 13.65]].forEach(([s, t0]) => {
  A('#ringDraw' + s, { draw: [[t0, 0], [t0 + 2.3, 1, 'io']] });
  A('#ringG' + s, { s: [[17.3, 1], [17.65, 1.035, 'out'], [18.3, 1, 'io']], o: [[t0 - 0.01, 0], [t0, 1]] });
  A('#lock' + s, { x: 0, y: -360, s: [[t0 + 2.2, 0], [t0 + 2.7, 1, 'back'], [17.3, 1], [17.6, 1.2, 'out'], [18.2, 1, 'io']], o: [[t0 + 2.19, 0], [t0 + 2.2, 1]] });
  A('#tint' + s, { o: [[t0 + 2.2, 0], [t0 + 3.6, 1, 'io']] });
});
A('#labels12', { o: [[5.3, 0], [5.9, 1], [T3 - 0.1, 1], [T3 + 0.3, 0]] });

/* ================= SCENE 3: your AI ================= */
const SEL = [[22.5, 23.3], [23.3, 24.1], [24.1, 24.9]];
[0, 1, 2, 3].forEach((i) => {
  const el = $('#chip' + i), x = +el.dataset.x, y = +el.dataset.y, t0 = T3 + 0.25 + i * 0.12;
  A(el, { x, y: [[t0, y + 26], [t0 + 0.7, y, 'o5'], [T4 - 0.3, y], [T4 + 0.2, y + 20, 'in']], o: [[t0, 0], [t0 + 0.4, 1], [T4 - 0.3, 1], [T4 + 0.15, 0]] });
});
A('#chip0-on', { o: [[SEL[0][0], 0], [SEL[0][0] + 0.2, 1], [SEL[0][1], 1], [SEL[0][1] + 0.2, 0], [24.9, 0], [25.1, 1]] });
A('#chip1-on', { o: [[SEL[1][0], 0], [SEL[1][0] + 0.2, 1], [SEL[1][1], 1], [SEL[1][1] + 0.2, 0]] });
A('#chip2-on', { o: [[SEL[2][0], 0], [SEL[2][0] + 0.2, 1], [SEL[2][1], 1], [SEL[2][1] + 0.2, 0]] });
A('#chip3-on', { o: [[22.5, 0], [22.8, 1]] });
/* a spark rises from the chosen chip into the note */
[['#sparkL', 345, 800, 540, 402, 0], ['#sparkR', 1380, 800, 1380, 400, 0.08]].forEach(([sel, x0, y0, x1, y1, d]) => {
  const a = T_SPARK + d, b = T_NOTE + d;
  A(sel, { x: [[a, x0], [b, x1, 'io']], y: [[a, y0], [b, y1, 'io']], r: [[a, 0], [b, 180, 'io']], s: [[a, 0], [a + 0.25, 1.6, 'out'], [b, 1.1, 'io'], [b + 0.35, 3.2, 'out']], o: [[a - 0.01, 0], [a, 1], [b, 1], [b + 0.35, 0, 'out']] });
});

/* ================= SCENE 4: sync, coming soon ================= */
A('#s4', { o: [[T4, 0], [T4 + 0.01, 1], [44.95, 1], [45, 0]] });
A('#soon', { x: 960, y: [[T4 + 0.5, 70], [T4 + 1.3, 106, 'o5']], o: fadeIn(T4 + 0.5, 0.5) });
const DS = 1.16, DCX = 960, DCY = 406, DY = 28;   // drive card scale about its centre, nudged down
const dX = (x) => DCX + (x - DCX) * DS, dY = (y) => DCY + (y - DCY) * DS + DY;
A('#drive', { x: DCX * (1 - DS), y: [[T4 + 0.7, DCY * (1 - DS) + DY + 50], [T4 + 1.6, DCY * (1 - DS) + DY, 'o5']], s: DS, o: fadeIn(T4 + 0.7, 0.5) });
A('#labels4', { o: fadeIn(T4 + 1.2, 0.5) });
A('#trackL', { o: fadeIn(T4 + 1.4, 0.5) }); A('#trackR', { o: fadeIn(T4 + 1.55, 0.5) });
A('#row0', { o: fadeIn(T4 + 1.3, 0.4) }); A('#row0-hl', { o: 0 });
/* pairing */
A('#mQR', { o: [[31.9, 0], [32.3, 1], [34.2, 1], [34.6, 0]] });
A('#pCode', { o: [[32.0, 0], [32.4, 1], [34.2, 1], [34.6, 0]] });
pop('#pCheck', 33.3, 0, 80, 0.45);
/* trip 1: Mac → drive → iPhone */
const PX = K([[35.8, 300], [37.45, dX(740) - 4, 'io'], [37.9, dX(786), 'in'], [38.2, dX(1180) - 6], [38.5, dX(1180) + 22, 'out'], [39.6, 1620, 'io']]);
const PY0 = 434;
const PYy = K([[37.45, PY0], [37.9, dY(ROW_Y[1] - 9), 'in'], [38.19, dY(ROW_Y[1] - 9)], [38.2, PY0]]);
const PS = K([[34.8, 0], [35.25, 1, 'back'], [37.45, 1], [37.9, 0.3, 'in'], [38.19, 0.3], [38.2, 0], [38.55, 1, 'back'], [39.95, 1], [40.35, 0, 'in']]);
A('#parcel', { x: PX, y: PYy, s: PS, o: [[34.79, 0], [34.8, 1], [37.8, 1], [37.9, 0], [38.19, 0], [38.2, 1], [40.34, 1], [40.35, 0]] });
A('#pcCard', { o: [[35.3, 1], [35.7, 0], [39.45, 0], [39.8, 1]], r: [[35.25, 0], [35.8, 45, 'io'], [39.4, 45], [39.85, 0, 'io']], s: [[35.25, 1], [35.8, 0.7, 'io'], [39.4, 0.7], [39.85, 1, 'io']] });
A('#pcDia', { o: [[35.35, 0], [35.7, 1], [39.45, 1], [39.8, 0]], s: [[35.3, 0.6], [35.8, 1, 'back'], [39.4, 1], [39.85, 0.6, 'io']] });
A('#pcLock', { x: 0, y: -2, s: [[36.35, 0], [36.75, 1, 'back'], [39.3, 1], [39.5, 0, 'in']], o: [[36.34, 0], [36.35, 1], [39.49, 1], [39.5, 0]] });
A('#pcLabel', { x: PX, y: (t) => PYy(Math.min(t, 37.45)) - 78, o: [[35.2, 0], [35.5, 1], [36.85, 1], [37.05, 0], [38.95, 0], [39.15, 1], [39.9, 1], [40.1, 0]] });
const PLAIN = 'Pilot rollout.md', BLOB = DRIVE_ROWS[1], HEXC = '0123456789abcdef';
const scr = K([[35.95, 0], [36.55, 1, 'lin'], [39.2, 1], [39.7, 0, 'lin']]);
const pcText = $('#pcText'); let lastLabel = '';
fx((t) => {
  if (t < 35 || t > 40.2) return;
  const p = scr(t), n = Math.max(PLAIN.length, BLOB.length), f = Math.floor(t * 15);
  let s = '';
  for (let i = 0; i < n; i++) {
    const a = (i / n) * 0.6;
    s += p <= a ? (PLAIN[i] || '') : p >= a + 0.4 ? BLOB[i] : HEXC[Math.floor(hash(i + 1, f) * 16)];
  }
  if (s !== lastLabel) { pcText.textContent = s; lastLabel = s; }
});
A('#row1', { o: fadeIn(37.8, 0.3) }); A('#row1-hl', { o: [[37.8, 0], [38.0, 1], [38.9, 1], [39.6, 0]] });
A('#pFlash', { o: [[39.2, 0], [39.55, 0.92, 'out'], [40.4, 0.92], [41.0, 0]] });
/* trip 2: iPhone → drive → Mac, quicker */
const QX = K([[40.9, 1620], [41.8, dX(1180) + 4, 'io'], [41.81, 1140], [42.1, 1140], [42.11, dX(740) - 4], [43.0, 300, 'io']]);
A('#parcel2', { x: QX, y: PY0, s: [[40.6, 0], [40.95, 1, 'back'], [41.7, 1], [41.82, 0.4], [42.1, 0.4], [42.2, 1, 'out'], [43.0, 1], [43.3, 0, 'in']], o: [[40.59, 0], [40.6, 1], [41.78, 1], [41.82, 0], [42.1, 0], [42.14, 1], [43.29, 1], [43.3, 0]] });
A('#row2', { o: fadeIn(41.8, 0.3) }); A('#row2-hl', { o: [[41.8, 0], [42.0, 1], [42.8, 1], [43.4, 0]] });
A('#mFlash', { o: [[34.65, 0], [35.0, 0.92, 'out'], [36.3, 0.92], [36.9, 0], [43.05, 0], [43.35, 0.9, 'out'], [43.95, 0]] });
A('#lockL', {}); A('#lockR', {});

/* ================= captions ================= */
CAPTIONS.forEach((c, i) => {
  const [a, b] = c;
  A('#cap' + i, { y: [[a, 1000], [a + 0.8, 978, 'o5'], [b - 0.35, 978], [b, 968, 'in']], o: [[a, 0], [a + 0.45, 1, 'out'], [b - 0.35, 1], [b, 0, 'in']] });
});

/* ================= SCENE 5: the promises, then the source ================= */
const T6 = 52.5;
A('#iris1', { rad: [[T5, 0], [T5 + 1.0, 1160, 'io']], o: [[T5 - 0.01, 0], [T5, 1], [T6 + 1.0, 1], [T6 + 1.05, 0]] });
A('#s5', { o: [[T5, 0], [T5 + 0.01, 1], [T6 + 1.0, 1], [T6 + 1.05, 0]] });
A('#lattice', { x: (t) => (t - T5) * -6, y: (t) => (t - T5) * 3, o: [[T5 + 0.5, 0], [T5 + 1.6, 0.1], [48.3, 0.1], [48.8, 0]] });
STATEMENTS.forEach((_, i) => {
  const t = T5 + 0.65 + i * 0.75, out = 48.25 + i * 0.07;
  A('#st' + i, { x: 0, y: [[t, 150], [t + 0.9, 0, 'o5'], [out, 0], [out + 0.5, -150, 'in']], o: [[t - 0.01, 0], [t, 1], [out + 0.5, 1], [out + 0.51, 0]] });
});
const TO = 48.7;
A('#os0', { x: 0, y: [[TO, 40], [TO + 0.9, 0, 'o5']], o: fadeIn(TO, 0.5) });
A('#os1', { x: 0, y: [[TO + 0.18, 40], [TO + 1.08, 0, 'o5']], o: fadeIn(TO + 0.18, 0.5) });
A('#os2', { o: fadeIn(TO + 0.7, 0.6) });
A('#term', { x: 1010, y: [[TO, 300], [TO + 1.0, 240, 'o5']], o: fadeIn(TO, 0.45) });
const CL_T = (i) => TO + 0.7 + i * 0.26;
const CL_W = [470, 640, 610, 560, 30, 0, 660, 440, 30, 470];
CODE.forEach((l, i) => {
  A(`#cl${i}-c`, { w: [[CL_T(i), 0], [CL_T(i) + 0.42, CL_W[i] + l[0] * 56 + 20, 'out']] });
  A(`#cl${i}`, { x: 0, y: [[CL_T(i), 18], [CL_T(i) + 0.5, 0, 'o5']] });
});
fx((t) => {
  const c = $('#caret');
  const i = clamp(Math.floor((t - (TO + 0.7)) / 0.26), 0, CODE.length - 1);
  const w = clamp((t - CL_T(i)) / 0.42) ; const e = EASE.out(w);
  const done = t > CL_T(CODE.length - 1) + 0.42;
  c.setAttribute('x', (46 + CODE[i][0] * 56 + e * CL_W[i] + 4).toFixed(1)); c.setAttribute('y', 100 + i * 48);
  c.setAttribute('opacity', t < TO + 0.7 ? 0 : done ? (Math.floor(t * 2.2) % 2 ? 0 : 1) : 1);
});

/* ================= SCENE 6: end card ================= */
A('#iris2', { rad: [[T6, 0], [T6 + 1.0, 1160, 'io']], o: [[T6 - 0.01, 0], [T6, 1]] });
A('#s6', { o: [[T6, 0], [T6 + 0.01, 1]] });
A('#mark', { x: 960, y: 318 });
A('#mkPlate', { rad: [[T6 + 0.45, 0], [T6 + 1.25, 170, 'o5']] });
A('#mkRim', { rad: [[T6 + 0.5, 0], [T6 + 1.3, 158, 'o5']] });
A('#mkLeaf', { s: [[T6 + 0.6, 0], [T6 + 1.45, 1, 'o5']], r: [[T6 + 0.6, -50], [T6 + 1.6, 0, 'o5']] });
LAT.forEach((p, i) => {
  const t = T6 + 0.95 + ((i * 5) % 9) * 0.07, ang = hash(i, 4) * Math.PI * 2, d = 620;
  A('#mk' + i, { x: [[t, p[0] + Math.cos(ang) * d], [t + 1.0, p[0], 'o5']], y: [[t, p[1] + Math.sin(ang) * d], [t + 1.0, p[1], 'o5']], r: [[t, 45 + (hash(i, 9) - 0.5) * 400], [t + 1.0, 45, 'o5']], s: [[t, 0.5], [t + 1.0, 1, 'o5']], o: [[t - 0.01, 0], [t, 1]] });
});
pop('#mkChili', T6 + 2.3, 0, 0, 0.5);
[['#e0', 54.5, 668], ['#e1', 54.85, 752], ['#e2', 55.2, 842], ['#e3', 55.5, 912]].forEach(([sel, t, y]) => A(sel, { y: riseIn(t, y, 34, 0.9), o: fadeIn(t, 0.5) }));
A('#fade', { o: [[DUR - 0.5, 0], [DUR - 0.02, 1, 'io']] });
A('#s0', { o: [[4.7, 1], [4.71, 0]] });

/* ---------- player */
let cur = 0;
function seek(t) { cur = clamp(t, 0, DUR); apply(cur); return cur; }
const qs = new URLSearchParams(location.search);
const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches;
let playing = false, t0 = 0, raf = 0;
function tick(now) { if (!playing) return; seek(((now - t0) / 1000) % DUR); raf = requestAnimationFrame(tick); }
function play() { if (playing) return; playing = true; t0 = performance.now() - cur * 1000; raf = requestAnimationFrame(tick); }
function pause() { playing = false; cancelAnimationFrame(raf); }
function fit() {
  const f = document.getElementById('frame'), s = Math.min(innerWidth / 1920, innerHeight / 1080);
  f.style.transform = `scale(${s}) translate(-50%, -50%)`;
}
if (!qs.has('capture')) { fit(); addEventListener('resize', fit); }
else { const f = document.getElementById('frame'); f.style.left = '0'; f.style.top = '0'; f.style.transform = 'none'; document.getElementById('stage').style.cursor = 'none'; }

let wanted = !qs.has('capture') && !qs.has('t') && !reduced;
seek(qs.has('t') ? parseFloat(qs.get('t')) : reduced ? POSTER_T : 0);
const ready = (document.fonts && document.fonts.ready ? document.fonts.ready : Promise.resolve()).then(() => { seek(cur); if (wanted) play(); return true; });
document.addEventListener('visibilitychange', () => { if (document.hidden) pause(); else if (wanted) play(); });
document.getElementById('stage').addEventListener('click', () => { wanted = !playing; wanted ? play() : pause(); });
addEventListener('keydown', (e) => {
  if (e.key === ' ') { e.preventDefault(); wanted = !playing; wanted ? play() : pause(); }
  if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { pause(); wanted = false; seek(cur + (e.key === 'ArrowRight' ? 1 : -1) * (e.shiftKey ? 1 : 1 / FPS)); }
});

window.__film = { duration: DUR, fps: FPS, posterTime: POSTER_T, seek, play, pause, ready, get time() { return cur; } };
})();
