# Sample prompts (copy/paste)

Starting-point recipes for the `imagegen` skill. They are complete examples, not the default
amount of augmentation to add to every request — keep the user's requirements and add detail
only per the specificity policy in `prompting.md`. The labeled lines are prompt scaffolding,
not a rigid schema; use the ones that help.

Pass the assembled block as `--prompt`. For edits, attach inputs with `--image` and label
each one's role in the block (`Input images: Image 1: ...`).

## Generate

### photorealistic-natural
```
Use case: photorealistic-natural
Primary request: candid photo of an elderly sailor on a small fishing boat adjusting a net
Scene/backdrop: coastal water with soft haze
Subject: weathered skin with wrinkles and sun texture
Style/medium: photorealistic candid photo
Composition/framing: medium close-up, eye-level
Lighting/mood: soft coastal daylight, shallow depth of field, subtle film grain
Materials/textures: real skin texture, worn fabric, salt-worn wood
Constraints: natural color balance; no heavy retouching; no watermark
Avoid: studio polish; staged look
```

### product-mockup
```
Use case: product-mockup
Primary request: premium product photo of a matte black shampoo bottle with a minimal label
Scene/backdrop: clean studio gradient from light gray to white
Subject: single bottle centered with subtle reflection
Style/medium: premium product photography
Composition/framing: centered, slight three-quarter angle, generous padding
Lighting/mood: softbox lighting, clean highlights, controlled shadows
Materials/textures: matte plastic, crisp label printing
Constraints: no logos or trademarks; no watermark
```

### ui-mockup
```
Use case: ui-mockup
Primary request: mobile app home screen for a local farmers market with vendors and daily specials
Asset type: mobile app screen
Style/medium: realistic product UI, not concept art
Composition/framing: clean vertical mobile layout with clear hierarchy
Constraints: practical layout, clear typography, no logos or trademarks, no watermark
```

### infographic-diagram
```
Use case: infographic-diagram
Primary request: detailed infographic of an automatic coffee machine flow
Scene/backdrop: clean, light neutral background
Subject: bean hopper -> grinder -> brew group -> boiler -> water tank -> drip tray
Style/medium: clean vector-like infographic with clear callouts and arrows
Composition/framing: vertical poster layout, top-to-bottom flow
Text (verbatim): "Bean Hopper", "Grinder", "Brew Group", "Boiler", "Water Tank", "Drip Tray"
Constraints: clear labels, strong contrast, no logos or trademarks, no watermark
```

### scientific-educational
```
Use case: scientific-educational
Primary request: biology diagram titled "Cellular Respiration at a Glance" for high school students
Scene/backdrop: clean white classroom handout background
Subject: glucose turns into energy inside a cell; include glycolysis, Krebs cycle, and electron transport chain
Style/medium: flat scientific diagram with consistent icons, arrows, and readable labels
Composition/framing: landscape slide-style layout with clear hierarchy and generous whitespace
Text (verbatim): "Cellular Respiration at a Glance", "Glucose", "Pyruvate", "ATP", "NADH", "FADH2", "CO2", "O2", "H2O"
Constraints: scientifically plausible; avoid tiny text; no extra decoration; no watermark
```

### logo-brand
```
Use case: logo-brand
Primary request: original logo for "Field & Flour", a local bakery
Style/medium: vector logo mark; flat colors; minimal
Composition/framing: single centered logo on a plain background with generous padding
Constraints: strong silhouette, balanced negative space; original design only; no gradients unless essential; no trademarks; no watermark
```

### illustration-story
```
Use case: illustration-story
Primary request: 4-panel comic about a pet left alone at home
Scene/backdrop: cozy living room across panels
Subject: pet reacting to the owner leaving, then relaxing, then returning to a composed pose
Style/medium: comic illustration with clear panels
Composition/framing: 4 equal-sized vertical panels, readable actions per panel
Constraints: no text; no logos or trademarks; no watermark
```

### stylized-concept
```
Use case: stylized-concept
Primary request: cavernous hangar interior with tall support beams and drifting fog
Scene/backdrop: industrial hangar interior, deep scale, light haze
Subject: compact shuttle parked near the center
Style/medium: cinematic concept art, industrial realism
Composition/framing: wide-angle, low-angle
Lighting/mood: volumetric light rays cutting through fog
Constraints: no logos or trademarks; no watermark
```

### ads-marketing
```
Use case: ads-marketing
Primary request: campaign image for a streetwear brand called Thread
Subject: group of friends hanging out together in a stylish urban setting
Style/medium: polished youth streetwear campaign photography
Composition/framing: vertical ad layout with natural poses and integrated headline space
Lighting/mood: contemporary, energetic, tasteful
Text (verbatim): "Yours to Create."
Constraints: render the tagline exactly once; clean legible typography; no extra text; no watermarks; no unrelated logos
```

### productivity-visual
```
Use case: productivity-visual
Primary request: one pitch-deck slide titled "Market Opportunity"
Asset type: fundraising slide image
Style/medium: clean modern deck slide, white background, crisp sans-serif typography
Subject: TAM/SAM/SOM concentric-circle diagram plus a small growth bar chart from 2021 to 2026
Composition/framing: 16:9 landscape slide, clear data hierarchy, polished spacing
Text (verbatim): "Market Opportunity", "TAM: $42B", "SAM: $8.7B", "SOM: $340M"
Constraints: readable labels, no clip art, no stock photography, no decorative clutter, no watermark
```

### historical-scene
```
Use case: historical-scene
Primary request: outdoor crowd scene in Bethel, New York on August 16, 1969
Scene/backdrop: open field with period-appropriate staging
Subject: crowd in period-accurate clothing, authentic environment
Style/medium: photorealistic photo
Composition/framing: wide shot, eye-level
Constraints: period-accurate details; no modern objects; no logos or trademarks; no watermark
```

## Asset templates

### Website hero background
```
Use case: stylized-concept
Asset type: landing page hero background
Primary request: minimal abstract background with a soft gradient and subtle texture
Style/medium: matte illustration / soft-rendered abstract background
Composition/framing: wide composition with usable negative space for page copy
Lighting/mood: gentle studio glow
Color palette: restrained neutral palette
Constraints: no text; no logos; no watermark
```

### Game environment concept art
```
Use case: stylized-concept
Asset type: game environment concept art
Primary request: cavernous hangar interior with tall support beams and drifting fog
Scene/backdrop: industrial hangar interior, deep scale, light haze
Subject: compact shuttle parked near the center
Style/medium: cinematic concept art, industrial realism
Composition/framing: wide-angle, low-angle
Lighting/mood: volumetric light rays cutting through fog
Constraints: no logos or trademarks; no watermark
```

### Game UI icon
```
Use case: stylized-concept
Asset type: game UI icon
Primary request: round shield icon with a subtle rune pattern
Style/medium: painted game UI icon
Composition/framing: centered icon; generous padding; clear silhouette
Constraints: no text; no background scene elements; no logos or trademarks; no watermark
```

### Tileable texture
```
Use case: stylized-concept
Asset type: tileable game texture
Primary request: worn sandstone blocks
Style/medium: seamless tileable texture; PBR-ish look
Scene/backdrop: neutral lighting reference only
Constraints: seamless edges; no obvious focal elements; no text; no logos or trademarks; no watermark
```

### Wireframe (desktop homepage)
```
Use case: ui-mockup
Asset type: website wireframe
Primary request: SaaS homepage layout with clear hierarchy
Style/medium: low-fi grayscale wireframe
Subject: top nav; hero with headline and CTA; three feature cards; testimonial strip; pricing preview; footer
Composition/framing: landscape desktop layout
Constraints: label major blocks; no color; no logos; no real photos; no watermark
```

### Logo (abstract symbol)
```
Use case: logo-brand
Asset type: logo concept
Primary request: geometric leaf symbol suggesting sustainability and growth
Style/medium: vector logo mark; flat colors; minimal
Composition/framing: centered mark; clear silhouette
Color palette: deep green and off-white
Constraints: no text unless requested; no gradients; no mockups; no 3D; no watermark
```

## Edit

### text-localization
```
Use case: text-localization
Input images: Image 1: original infographic
Primary request: replace "Bean Hopper", "Grinder", "Brew Group" with "Tolva", "Molino", "Grupo de infusion"
Constraints: change only the text; preserve layout, typography, spacing, and hierarchy; no extra words; do not alter logos or imagery
```

### identity-preserve
```
Use case: identity-preserve
Input images: Image 1: person photo; Image 2..N: clothing references
Primary request: replace only the clothing with the provided garments
Constraints: preserve face, body shape, pose, hair, expression, and identity; match lighting and shadows; keep the background unchanged; no accessories or text
```

### precise-object-edit
```
Use case: precise-object-edit
Input images: Image 1: room photo
Primary request: replace only the white chairs with wooden chairs
Constraints: preserve camera angle, room lighting, floor shadows, and surrounding objects; keep all other aspects unchanged
```

### lighting-weather
```
Use case: lighting-weather
Input images: Image 1: original photo
Primary request: make it look like a winter evening with gentle snowfall
Constraints: preserve subject identity, geometry, camera angle, and composition; change only lighting, atmosphere, and weather
```

### background-extraction
```
Use case: background-extraction
Input images: Image 1: product photo
Primary request: isolate the product on a flat solid #00ff00 chroma-key background for local removal
Constraints: background must be one uniform color with no shadows, gradients, texture, reflections, floor plane, or lighting variation; crisp silhouette; generous padding; no halos or fringing; preserve label text exactly; do not use #00ff00 anywhere in the subject
```

### style-transfer
```
Use case: style-transfer
Input images: Image 1: style reference
Primary request: apply Image 1's visual style to a man riding a motorcycle on a plain white backdrop
Constraints: preserve palette, texture, and brushwork; no extra elements
```

### compositing
```
Use case: compositing
Input images: Image 1: base scene; Image 2: subject to insert
Primary request: place the subject from Image 2 next to the person in Image 1
Constraints: match lighting, perspective, and scale; keep the base framing unchanged; no extra elements
```

### character consistency
```
Use case: identity-preserve
Input images: Image 1: previous character anchor illustration
Primary request: continue the story with the same character in a new scene and action
Scene/backdrop: snowy forest after a winter storm
Subject: same young forest hero gently helping a frightened squirrel out of a fallen tree
Style/medium: same children's book watercolor illustration style as Image 1
Constraints: do not redesign the character; preserve facial features, proportions, outfit, color palette, and personality; no text; no watermark
```

### sketch-to-render
```
Use case: sketch-to-render
Input images: Image 1: drawing
Primary request: turn the drawing into a photorealistic image
Constraints: preserve layout, proportions, and perspective; choose realistic materials and lighting; do not add new elements or text
```
