# Brand: Gapless

_Status: set_

## Direction

**A flight recorder for Solana streams.** An instrument, not a dashboard: calm at rest, precise under stress, and honest about what it knows. The console is dark-first (a light theme exists for daylight projection).

Archetype: industrial/utilitarian, with Workstation Dense panels and Warm Monochrome restraint.

| | |
|---|---|
| Density | Compact inside panels (13–14px data), comfortable page frame |
| Surface | Flat graphite panels separated by 1px hairlines. No drop shadows on panels; elevation comes from background shade. Shadows only on overlays (menus, dialogs, command palette). |
| Type mood | Technical, quiet, exact |
| Motion | Crisp, never bouncy. 100ms feedback, 150–250ms enter/exit, entry slower than exit. The slot tape scrolls continuously; everything else moves only to report a change. |

## Color

Neutrals are cool graphite (hue 255). The only chromatic colors are the four slot states. They are never decorative: if something is green, it is live.

| Token | Dark | Light | Means |
|---|---|---|---|
| `--live` | `oklch(0.80 0.15 162)` | `oklch(0.56 0.13 160)` | Received live from the stream |
| `--replay` | `oklch(0.83 0.14 78)` | `oklch(0.64 0.14 68)` | Recovered by `from_slot` replay or the handoff patch |
| `--gap` | `oklch(0.69 0.19 22)` | `oklch(0.56 0.20 22)` | Missing: outage, disconnect, or a verified miss |
| `--verified` | `oklch(0.75 0.12 255)` | `oklch(0.55 0.16 258)` | Checked against RPC ground truth; also the focus ring |

Surfaces (dark): background `oklch(0.155 0.006 255)`, panel `0.185`, raised `0.215`. Text `oklch(0.935 …)`, secondary `0.72`, faint `0.60` (large or non-essential text only). Hairlines are white at 6.5–10% opacity. No pure black, and no gradients.

State colors always come with a second cue: a shape on the tape (gaps are hollow outlines, verified cells carry a tick), or a text label. Color alone never carries meaning.

## Typography

- **Geist** (variable) for UI text. Headings use tight tracking (-0.02em to -0.035em) and weight 600.
- **Geist Mono** (variable) for every number, slot, signature and duration: `.num` = mono + tabular numerals + slashed zero.
- Panel headers use `.label`: 11px, uppercase, 0.08em tracking, muted. These are instrument labels, not headings.
- Three weights: 400, 500, 600. Five sizes in the app: 11 / 13 / 14 / 16 / 20, plus display sizes on the landing page.

## Shape

Radius by role: panels 8px, controls 6px, tape cells 2px, status pills fully round. Borders are 1px.

## Voice

Specific and factual, like a flight log: "11,015 of 11,015 transactions verified against getTransactionsForAddress", not "blazing-fast reliability". Numbers over adjectives. Active voice. When something went wrong, say what happened and what Gapless did about it.
