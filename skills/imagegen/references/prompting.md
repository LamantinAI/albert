# Prompting best practices

Depth material for the `imagegen` skill. These are principles of prompt structure,
specificity, and iteration for a diffusion image model (driven through Codex's built-in
`image_gen` tool on the ChatGPT subscription). Copy/paste recipes live in
`sample-prompts.md`.

## Structure
- Keep a consistent order: scene/backdrop -> subject -> key details -> constraints ->
  output intent.
- Include the intended use (ad, UI mock, infographic) to set the level of polish.
- For complex requests, use short labeled lines instead of one long paragraph.

## Specificity policy
- If the user's prompt is already specific and detailed, normalize it into a clean spec
  without adding creative requirements.
- If the prompt is generic, add tasteful detail only when it materially improves output.
- Treat the recipes in `sample-prompts.md` as fully-authored examples, not as the default
  amount of augmentation to add to every request.
- For photorealism, include the word `photorealistic` when that is the goal, plus concrete
  real-world texture: pores, wrinkles, fabric wear, material grain, imperfect everyday
  detail.

## Allowed and disallowed augmentation
Allowed for generic prompts:
- composition and framing cues
- intended-use or polish-level hints
- practical layout guidance
- reasonable scene concreteness that supports the request

Do not add:
- extra characters, props, or objects that are not implied
- brand palettes, slogans, or story beats that are not implied
- arbitrary left/right placement unless the surrounding layout supports it

## Composition and layout
- Specify framing and viewpoint (close-up, wide, top-down) only when it materially helps.
- Call out negative space when the asset needs room for UI or copy.
- For people, describe body framing, scale, gaze, and object interactions when they matter
  (`full body visible`, `looking down at the book`, `hands gripping the handlebars`).

## Constraints and invariants
- State what must not change (`keep the background unchanged`).
- For edits, say `change only X; keep Y unchanged` and repeat invariants on every iteration
  to reduce drift.

## Text in images
- Put literal text in quotes or ALL CAPS and specify typography (style, size, color,
  placement).
- Spell uncommon words letter-by-letter when accuracy matters.
- Require verbatim rendering and no extra characters. The model still drifts on long text,
  so keep in-image copy short and check it afterwards.

## Input images and references
- Do not assume every provided image is an edit target.
- Label each image by index and role (`Image 1: edit target`, `Image 2: style reference`).
- Images given only for style/composition/mood guidance mean generation with references,
  not an edit.
- Only when the user wants an existing image preserved with specific parts changed is it an
  edit.
- For compositing, describe how the images interact (`place the subject from Image 2 into
  Image 1`).

## Generate vs edit: often start fresh
- Editing is for keeping identity: a face, a specific product, an exact layout.
- If the change touches most of the frame (new angle, new style, new composition), a fresh
  generation from a good prompt usually beats fighting an edit. Do not force `--image` when
  a clean prompt gets there faster.

## Iterate deliberately
- Start from a clean base prompt, then make small single-change edits.
- Re-specify critical constraints when you iterate.
- Prefer one targeted follow-up at a time over rewriting the whole prompt.

## Transparent backgrounds
- The built-in tool has no true transparency control. Prompt for a perfectly flat solid
  chroma-key background (usually `#00ff00`; `#ff00ff` when the subject is green; avoid a key
  color that appears in the subject).
- Explicitly forbid shadows, gradients, floor planes, reflections, texture, and lighting
  variation in the background; ask for crisp edges and generous padding.
- Clean removal needs a separate step and is unreliable for hair, fur, glass, smoke,
  liquids, or soft shadows. For those, tell the user true transparency is not something this
  path does well rather than shipping a fringed cutout.

## Size and quality
- `1024x1024` (square) is the fastest default. `1536x1024` landscape, `1024x1536` portrait,
  `2048x1152` for 2K, `3840x2160` / `2160x3840` for 4K.
- gpt-image-2 sizes: max edge <= 3840px, both edges multiples of 16, ratio <= 3:1.
- Reach for larger sizes for final assets, dense text, diagrams, and detailed scenes; keep
  drafts square and small.

## Use-case tips
Generate:
- photorealistic-natural: prompt as if capturing a real photo in the moment; use photography
  language (lens, lighting, framing); call for real texture; avoid over-stylized polish
  unless requested.
- product-mockup: describe the product/packaging and materials; clean silhouette and label
  clarity; require verbatim in-image text and specify typography.
- ui-mockup: state target fidelity first (shippable mockup vs low-fi wireframe), then layout,
  hierarchy, practical UI elements; avoid concept-art language.
- infographic-diagram: define audience and layout flow; label parts explicitly; require
  verbatim text; use a larger size for dense labels.
- logo-brand: simple and scalable; strong silhouette, balanced negative space; avoid
  decorative flourishes unless requested.
- ads-marketing: write like a creative brief; brand positioning, audience, vibe, scene, and
  the exact tagline if text must appear.
- productivity-visual: name the exact artifact (slide, chart, workflow), define canvas and
  hierarchy, provide real labels/data, ask for readable typography and polished spacing.
- scientific-educational: define audience, lesson objective, required labels, scientific
  constraints, arrows, scan-friendly whitespace.
- illustration-story: define panels or scene beats; keep each action concrete.
- stylized-concept: specify style cues, material finish, and rendering approach (3D,
  painterly, clay) without inventing new story elements.
- historical-scene: state location/date and required period accuracy; constrain clothing,
  props, and environment to the era.

Edit:
- text-localization: change only the text; preserve layout, typography, spacing, hierarchy.
- identity-preserve: lock identity (face, body, pose, hair, expression); change only the
  specified elements; match lighting and shadows.
- precise-object-edit: specify exactly what to remove/replace; preserve surrounding texture
  and lighting; keep everything else unchanged.
- lighting-weather: change only environmental conditions; keep geometry, framing, and subject
  identity.
- background-extraction: request a clean cutout on a flat chroma-key background; crisp
  silhouette, generous padding, no shadows or halos; preserve label text exactly.
- style-transfer: specify style cues to preserve (palette, texture, brushwork) and what must
  change; add `no extra elements` to prevent drift.
- compositing: reference inputs by index; specify what moves where; match lighting,
  perspective, scale; keep base framing unchanged.
- sketch-to-render: preserve layout, proportions, perspective; choose materials and lighting
  that support the sketch without adding new elements.
