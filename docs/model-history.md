# Journal versus provider continuation state

A conversation journal is application data. It is not an archive of API packets
to re-submit verbatim to every subsequent model. This separation fixes the failure
when returning from DeepSeek through OpenRouter to the Codex subscription backend.

## The persistent journal

`[albert tool journal v2]` records are constructed when a completed/interrupted
round is journaled. They contain ordinary messages with explicitly labelled
historical tool invocations and results: identifiers, tool names, arguments,
complete outputs, images, and the existing completed/UNKNOWN/not-executed outcomes.
These are reference data, not pending calls to execute again.

Reasoning, encrypted state, signatures, provider-specific tool fields and assistant
message IDs are not part of this durable journal. This is the same representation
for all models, without family detection or repeated model-to-model conversion.
The usual SDK serialization into the destination API remains necessary, as for
any text/image conversation.

Old `[albert tool trace v1]` records have a compatibility reader. It presents the
same journal view without editing old records or clearing the conversation. New
v2 records are read directly. Quoting either storage marker in a model's response
cannot inject host-authored records. Images retain their content and media type.

Interrupt checkpoints and automatic hearing append journal records to the next
turn's working context, too. Provider changes or interruptions start a new model
attempt with the existing journal; they do not resume another provider's native
reasoning session. No completed tool action is automatically re-executed.

## The active tool loop

Inside one attempt, provider state can be mandatory. It must remain intact until
the loop completes or is cancelled:

| Interface | Continuation state |
|-----------|--------------------|
| OpenAI Responses / Codex | Native reasoning items, IDs and encrypted content |
| DeepSeek through OpenRouter | Plain reasoning and/or gateway reasoning details |
| Claude through OpenRouter | Thinking blocks, opaque signatures, ordering and format fields |
| Gemini through OpenRouter | Thought state associated with the appropriate function-call part |

These are not interchangeable. Anthropic requires complete thinking blocks during
tool use; Google validates function-call signatures within the current turn;
OpenRouter documents preserving reasoning details. Sources:
[Anthropic](https://platform.claude.com/docs/en/build-with-claude/extended-thinking),
[Gemini](https://ai.google.dev/gemini-api/docs/generate-content/thought-signatures),
[OpenRouter](https://openrouter.ai/docs/guides/best-practices/reasoning-tokens).

Albert's Codex path keeps the native streaming adapter. The API-key path uses a
per-attempt `OpenRouterHttp` transport: it retains original assistant tool-call
messages and returns them unchanged on subsequent requests in that loop. This
preserves opaque fields that rig's generic representation may otherwise omit or
reorder, without reconstructing them by model family. The capture is private to
that client/attempt, is not printed by Debug, and is dropped with the attempt.
Repeated call IDs retain their occurrence order rather than overwriting a round.

The API-key path currently uses non-streaming completions. This transport explicitly
rejects streaming calls rather than pretending to capture SSE continuation state.
Codex continues to stream through its existing separate adapter.

## Checks and limits

Local HTTP fixtures cover gateway shapes for DeepSeek, Claude, Gemini and OpenAI,
including exact second-request replay. Journal tests cover mixed legacy history,
images, complete action results, marker escaping and both Codex entry points.
These are protocol regression tests, not live certification of every hosted model
or a new implementation of the native Anthropic/Gemini clients. Models still use
the two supported backend paths: API-key chat completions and Codex subscription.
