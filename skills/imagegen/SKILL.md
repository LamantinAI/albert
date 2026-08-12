---
name: imagegen
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

Image generation runs through Codex's built-in `image_gen` tool, which works on the
shared **ChatGPT subscription** (an `auth.json` in `chatgpt` mode) — no OpenAI key, no
per-image charge. We drive `codex exec` from the sandbox; it renders and saves a PNG, and
the script copies it into the workspace.

## How to run

The backend is a bash script, run through **forkd**: `dispatch_to_connector` target
`"forkd"`, kind `"forkd.run"`, payload:

```
{ "skill_path": "imagegen/scripts/imagegen.sh",
  "interpreter": "bash",
  "args": ["--prompt", "<the assembled spec, see below>",
           "--out", "image.png",
           "--size", "1024x1024"],
  "timeout_secs": 300 }
```

- `--prompt` (required) — the spec you assemble (see "Building the prompt").
- `--out` — the workspace filename (default `image_<time>.png`).
- `--size` — gpt-image-2 size: `1024x1024` (square, fastest), `1536x1024` (landscape),
  `1024x1536` (portrait), `2048x1152` (2K), `auto`. Both edges multiples of 16, ratio no
  steeper than 3:1.
- `--image <path>` — an input image (a reference or an edit target), repeatable. Describe
  each one's role in words inside `--prompt` ("Image 1 — style reference", "Image 2 — edit
  target").

The script prints a summary to stdout: `[out] <path>`, `[size]`, `[src]`. Set the timeout
to ~300s (one image is usually 1-2 minutes); for a batch, call the script once per image
and raise the timeout.

## Delivering the result to the user

The image sits in the workspace at the path from `[out]`. Send it to the chat with
**`chat.send_file`**: `dispatch_to_connector` target — the telegram connector's id, kind
`"chat.send_file"`, payload `{ "path": "image.png", "caption": "<caption>" }`. It goes out
as a **photo with a preview** (not a document); `caption` is a short caption, optional.
Don't paste the image bytes into the reply text — deliver it only by reference via
`chat.send_file`.

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

The built-in tool gives no true transparency. Ask for a flat solid chroma-key background
(`#00ff00`, or `#ff00ff` for green subjects), with no shadows or gradients and generous
padding — the background can then be removed locally. For hard edges (hair, fur, glass,
smoke) a clean cutout won't happen — tell the user honestly.

## Iterating and checking

Once generated, look with your eyes: subject, style, composition, text accuracy, whether
the invariants and `Avoid` held. Fix one change at a time and re-check. Don't grind out
dozens of variants — the model is paid via the subscription, but not free of the user's
time.

## If it fails

The script returns a non-zero code and the reason on `stderr`:
- "codex binary not found" — the Codex CLI isn't installed on this stand (needed for
  subscription image generation). Tell the user honestly, don't invent an image.
- "auth_mode is not chatgpt" — codex isn't logged in on the subscription; the script
  refuses so it doesn't bill an API key.
- "image was not produced" — the model didn't save a file; retry with a clearer prompt.
Report the `stderr` error as-is.
