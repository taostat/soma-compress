# soma-compress

Extractive compression for oversized agent tool results, in pure Rust.

A text-level port of the compression core of
[SOMA-OpenClaw-compressor](https://github.com/DendriteHQ/SOMA-OpenClaw-compressor)
by [Dendrite](https://github.com/DendriteHQ) (MIT). Built for LLM gateways
and agent runtimes that want to shrink bulky tool output — logs, test
runs, file dumps — without touching anything load-bearing.

## What it does

```rust
match soma_compress::compress(&tool_result_text) {
    Some(smaller) => forward(smaller), // extractive summary in [[CMP]]…[[/CMP]] markers
    None => forward(tool_result_text), // small, already compressed, or not worth it
}
```

- Results at or below **16 KB pass through untouched**; larger ones keep a
  proportional share (60%, floored at 16 KB, capped at 32 KB).
- **Load-bearing lines survive**: tracebacks, failing-test names, assertion
  lines, file paths, imports, function/class signatures, diff hunks. Lines
  are otherwise ranked by TF-IDF informativeness; elisions are marked with
  `…` and the compressed region is wrapped in `[[CMP]]…[[/CMP]]` so the
  model can see shortening happened.
- Content that is *all* load-bearing (e.g. grep output, where every line
  carries a path) is left alone entirely — when unsure, don't compress.

## Invariants

The properties gateways rely on, each pinned by tests:

- **Pure and message-local** — output depends only on the input text's own
  bytes. Never on conversation length, position, or any global state. A
  client replaying history raw recompresses byte-identically, so upstream
  provider prompt-caches stay warm.
- **Deterministic** — byte-identical across calls, threads, and processes.
- **Idempotent** — output contains `[[CMP]]`; re-compressing returns `None`.
- **Never inflating** — emitted only when strictly smaller than the input.
- **Panic-free** — UTF-8-boundary-safe on arbitrary input (property-tested).

## Measured ratios (real artifacts)

| Input | Saved |
|---|---|
| `cargo test` log (128 KB) | 74.8% |
| Structured JSON service logs (235 KB) | 85.9% |
| Large JSON dump (250 KB) | 86.9% |
| Source-file read (67 KB) | 51.0% |
| `rg` output, 200 KB (every line path-pinned) | 0% — untouched by design |

Ratios measure token reduction only; end-task quality is workload-dependent —
evaluate on your own traffic before enabling anywhere quality-sensitive.

## Differences from the Python original

Byte lengths instead of character counts; one native TF-IDF scorer instead
of the sklearn-with-fallback pair (removes an environment-dependent
determinism seam). The connector-side features (message walking, thinking-block
sanitizing, loop guard) are intentionally out of scope — callers own message
structure.

## License

MIT, as a derived work of
[SOMA-OpenClaw-compressor](https://github.com/DendriteHQ/SOMA-OpenClaw-compressor)
— see [LICENSE](LICENSE).
