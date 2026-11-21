# Milkrice film: storyboard

**Line:** Your meetings, your devices, your AI.
**Format:** 16:9, 1920×1080, 30 fps, 58.5 s, silent. Every beat carries a short caption so it reads muted.
**Look:** flat editorial illustration on coconut cream. Two round "spot" vignettes (one per device) carry the story; the disc is the stage in beat 1 and becomes the privacy boundary in beat 2. Fraunces (SOFT 100) for captions and display, system sans for device UI, mono for file names and code. Banana-leaf green for "safe / yours", clay for warmth, chili only for the recording diamond. One recurring shape: the diamond (kiribath cut). One recurring transition: the circle iris.

All meeting content is fictional ("Pilot rollout", "Kitchen quote", Maya, Jonas).

| # | Time | Picture | Motion | Caption |
|---|------|---------|--------|---------|
| 0 | 0.0–4.4 | Cream field, three stacked serif lines. | Lines rise in one by one, then lift away. | Your meetings. / Your devices. / Your AI. |
| 1a | 4.0–8.3 | Two discs. Left: a Mac on a desk, mug and plant, a two-person video call on screen. Right: an iPhone on a café table, a cup and a plate of kiribath, two speech bubbles leaning in from outside. | Discs bloom, props pop in. Call tiles and bubbles take turns speaking. | On a call. *In the room.* |
| 1b | 7.0–12.4 | Both devices start listening: a "Listening" pill drops into the Mac; the iPhone's Start becomes "Stop & write notes", the timer runs, the diamond meter and Dynamic Island wake. | Tap ripple, chili diamonds pulse. | Milkrice listens. No bot joins. |
| 2a | 12.4–17.2 | Props clear; the devices grow inside their discs. Sound bars run along each line and are wiped into words. A dashed green ring draws around each disc. | Bars-to-text wipe per line; ring draws clockwise. | Sound becomes words, on the device. |
| 2b | 17.2–21.5 | Ring closed, lock badge on top, disc tints leaf-green. Sound motes drift inside and never cross. | Lock pops; soft tint. | The audio never leaves it. |
| 3a | 21.5–26.2 | AI chips under each device. Mac: Claude Code, Codex, Ollama · offline (selection walks across them, settles). iPhone: Apple Intelligence · on-device. Each transcript line folds into its speaker dot, which turns into a diamond. | Text collapses into dots; dots square off and rotate into diamonds. A spark rises from the chosen chip. | Then *your* AI writes the notes. |
| 3b | 26.2–30.5 | Diamonds become the note's bullets; title and bullet text write in. | Diamonds glide to bullet positions; text reveals. | Cut into diamonds, ready to read. |
| 4a | 30.5–34.5 | Pull back: discs shrink to the sides, a "Your iCloud Drive" card sits between them. Pill on top: "Optional sync · coming soon". Mac shows a QR code, iPhone a six-digit check. Labels: "Mac", "iPhone · beta", "key stays here". | Camera pull-back; QR fades up; check pops. | Pair once with a QR code. |
| 4b | 34.5–39.5 | A note folds into a locked diamond parcel, leaves the Mac's ring (its name scrambles as it crosses), drops into the drive as an opaque row, comes out and is unlocked inside the iPhone's ring. | Fold, travel, scramble, land. | Encrypted before it leaves. |
| 4c | 39.5–44.0 | The drive card lists only random `.rec` names at 4 KB each. A second parcel makes the return trip. | Return trip; rows pulse. | Your drive sees only blobs. |
| 5a | 44.0–48.4 | Iris to the banana-leaf band. Three big lines. | Circle iris; lines rise from a mask. | No account. / No server of ours. / You choose the AI. |
| 5b | 48.4–52.5 | Headline left, a dark code card right; code lines unfold one by one. | Lines unfold with stagger. | Open source. Read every line. (MIT) |
| 6 | 52.5–58.5 | Iris back to cream. Nine rice diamonds fly in and settle on a leaf-lined clay plate, a pinch of chili in the middle. Wordmark, line, URL. | Diamonds assemble; type rises. | Milkrice — Your meetings, your devices, your AI. — milkrice.yasithfernando.com |

## Truthfulness guard-rails
- The iPhone only records the room (speech bubbles outside the phone), never another app's call.
- "iPhone · beta" label is on screen whenever the iPhone is named in beats 1, 2 and 4.
- Sync is on screen only under the "Optional sync · coming soon" pill.
- The cloud card shows random file names and padded sizes only; keys are drawn on the devices.
- AI names are plain text chips, no third-party logos.

## Technical
- `film.js` builds one inline SVG and drives every property from a single time value. `window.__film = { duration, fps, seek(t), play(), pause() }`.
- `?t=12.5` freezes on a frame; `?capture` disables autoplay. Reduced motion shows the poster frame.
- `capture.mjs` (Node 22+, no dependencies) steps headless Chrome frame by frame over CDP.
