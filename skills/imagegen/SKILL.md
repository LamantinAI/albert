---
name: imagegen
command: draw
command_about: Draw or edit a picture: /draw <what>
requires: imagegen
description: >-
  Generates and edits raster images on the ChatGPT subscription — no API keys, no
  per-image billing. Photos, illustrations, concept art, logos, mockups, sprites,
  covers, infographics; editing an existing picture (background swap, lighting, an
  object), variants from a reference. Activate when asked to "draw", "generate an
  image/picture", "make art/an illustration/a logo/a cover", "make a mockup",
  "transform this photo", "remove/replace the background". Not for vector icons and
  diagrams that are better built in SVG/HTML/CSS code.
---

Image generation runs natively on the shared **ChatGPT subscription**: the `imagegen`
connector sends your spec to `gpt-image-2` through the subscription Images endpoint — no
OpenAI key, no per-image charge, no scripts. One call, usually 20-60 seconds.

## How to run

`dispatch_to_connector` target `"imagegen"`, kind `"imagegen.run"`, payload:

```
{ "prompt": "<the assembled spec, see below>",
  "size": "1024x1024",
  "quality": "auto",
  "background": "auto",
  "images": ["<workspace path>", ...] }
```

- `prompt` (required) — the spec you assemble (see "Building the prompt").
- `size` — `1024x1024` (square, fastest), `1536x1024` (landscape), `1024x1536` (portrait),
  `2048x1152` (2K), `auto`. Both edges multiples of 16, ratio no steeper than 3:1. Omitted,
  the endpoint picks.
- `quality` — `low` (fast drafts), `medium`, `high` (slowest), `auto`.
- `background` — `transparent` for a cutout (logos, stickers, sprites), `opaque`, `auto`.
- `images` — up to 5 workspace paths (a reference or an edit target). Present, the call
  **edits** them. Describe each one's role in words inside `prompt` ("Image 1 — style
  reference", "Image 2 — edit target").

The result is `{ "path": "image-<id>.png" }` in the workspace (or `{ "error": ... }` — read
it: a 400 usually means the content policy or a bad size).

## Delivering the result to the user

Send the image with **`chat.send_file`**: `dispatch_to_connector` target — the telegram
connector's id, kind `"chat.send_file"`, payload `{ "path": "<the path>", "caption":
"<caption>" }`. It goes out as a **photo with a preview** (not a document); `caption` is
optional. Don't paste image bytes into the reply text — deliver it only by reference.

## Building the prompt (this is half the battle)

A diffusion model listens to structure, not a stream of consciousness. Assemble the spec
from labeled lines — take only the ones you need, order "scene -> subject -> details ->
constraints":

```
Use case: <slug from the taxonomy below>
Primary request: <the core of the user's request>
Subject: <the main object>
Scene/backdrop: <environment, background>
Style/medium: <photo / illustration / 3D / flat-vector / watercolour ...>
Composition/framing: <wide/close/top-down; placement, negative space>
Lighting/mood: <light and mood>
Color palette: <palette>
Text (verbatim): "<exact on-image text, if any>"
Constraints: <what must be kept>
Avoid: <what must not appear: logos, watermarks, extra text ...>
```

How much to add:
- **Prompt already detailed** — normalise it into structure, invent nothing of your own.
- **Prompt generic** — add, tastefully, only what genuinely improves the result (framing,
  polish level, scene concreteness). Don't slip in extra characters, brands, slogans, or
  palettes the user didn't ask for.

Small things that help a lot:
- State the intended use (cover, app mockup, ad, infographic) — it sets the mode and level
  of detail.
- For photorealism, use camera language (lens, angle, depth of field, light).
- Put exact text in quotes and demand it verbatim; spell tricky words letter by letter.
  The model still drifts on long text — keep captions short.

Taxonomy (the `Use case:` slug): `photorealistic-natural`, `product-mockup`, `ui-mockup`,
`infographic-diagram`, `scientific-educational`, `ads-marketing`, `productivity-visual`,
`logo-brand`, `illustration-story`, `stylized-concept`, `historical-scene`. For edits:
`text-localization`, `identity-preserve`, `precise-object-edit`, `lighting-weather`,
`background-extraction`, `style-transfer`, `compositing`, `sketch-to-render`.

More principles and ready recipes are in `references/prompting.md` and
`references/sample-prompts.md`. Read them when the task is non-trivial.

## Generate or edit

- No input image, or inputs given only as a **reference** for style/mood -> this is
  **generation**.
- Asked to change an existing image while keeping parts of it -> **edit**: pass the file
  via `--image` and, in the spec, list the invariants hard ("change only the background;
  don't touch the subject or its edges") and repeat them every iteration.
- **Starting fresh often beats editing.** If the edit would rewrite most of the frame (new
  angle, style, composition), don't force an edit — regenerate from a good prompt. Editing
  is worth it when you must preserve identity (a face, a specific product, an exact layout).

## Transparent background

Pass `"background": "transparent"` and the image comes back with a real alpha channel —
right for logos, stickers, sprites and cutouts. Keep generous padding around the subject.
Very fine edges (hair, fur, smoke, glass) may still come out imperfect — say so honestly
if it matters.

## Iterating and checking

Once generated, look with your eyes: subject, style, composition, text accuracy, whether
the invariants and `Avoid` held. Fix one change at a time and re-check. Don't grind out
dozens of variants — the model is paid via the subscription, but not free of the user's
time.

## If it fails

The connector answers `{ "error": "..." }` with the reason:
- `HTTP 400` — the request was rejected: usually the content policy, sometimes a bad size.
  Rephrase the prompt (or fix the size) rather than repeating it.
- `HTTP 429` — the subscription's usage window is exhausted; tell the user to try later.
- `HTTP 401` / "sign in again" — the subscription token is gone; the owner needs to re-run
  the login.
Report the error honestly; never describe an image you did not get.
