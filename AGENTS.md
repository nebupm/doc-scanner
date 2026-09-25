# AGENTS.md

Rust CLI that converts bank statements / grocery bills (PDF or image) into JSON.
Extraction layer for a larger multi-tenant, Postgres-backed expense app.
Full design: `design-doc-rust-doc-scanner.md`. This file is the fast-reference version.

## Architecture (non-negotiable)

- All business logic lives in `doc_scanner_core` (lib crate). Binaries are thin wrappers only.
- `doc_scanner_cli`: subcommands `convert` (default) and `seed-report`.
- `doc_scanner_service` (axum HTTP wrapper): deferred — occasional-use scope, don't build yet.
- Never put parsing/extraction/business logic directly in a binary's `main.rs`.

## Hard constraints

- **Local only.** No network calls, no LLM/cloud OCR/Document-AI APIs, ever. OCR = local Tesseract
  (`leptess`). Structuring = rule-based parsers. If a change would make a network call, stop and flag it.
- **stdout = JSON only.** Exactly one JSON document per invocation, success or error. All logs/
  diagnostics go to stderr via `tracing`.
- **Engine is tenant-agnostic.** Never reference `tenant_id`, Postgres, or any DB concept inside
  `doc_scanner_core`. That belongs to the ingestion layer, not this engine.
- **Engine never decides a category.** `category` is always `null` from the engine.
  `category_suggestion` is a separate, advisory-only field — never conflate the two.
- **No card/account numbers.** Don't extract or persist card numbers or account numbers, masked or
  not. Bank name, account holder name, account type, and statement period are fine.
- **Logging is local-only and OTEL-shaped.** Every `convert` invocation emits one structured log
  record to stderr, shaped like the OpenTelemetry Log Data Model (timestamp, severity, body,
  attributes, resource) — never an OTLP network export from this engine, consistent with the
  local-only constraint above. Required attributes: `file_name`, `status` (`success`|`failed`),
  `bytes`, `processing_time_ms`, `document_type`, `extraction_method`, `content_hash`, and on
  failure `error.stage` + `error.code`. Design doc §14 has the full shape. This is what lets a
  local agent (e.g. Promtail → Loki) scrape logs today, and lets the same log shape keep working
  once this engine runs inside a container behind a frontend/backend.

## JSON contract (design doc §5 has the full schema)

- Envelope: `{ status, meta, data }` on success; `{ status: "error", meta, error }` on failure.
- Bank statement `data.account`: `bank_name`, `account_holder_name`,
  `account_type` (`checking`|`savings`|`credit_card`|`unknown`), `currency`, `statement_period`,
  `opening_balance`, `closing_balance`.
- Bank statement `data.transactions[]`: `date`, `raw_text`, `description`, `merchant`, `debit`,
  `credit`, `balance`, `category` (null), `category_suggestion`.
- Grocery bill `data.items[]`: `name`, `raw_text`, `quantity`, `unit_price`, `total_price`,
  `category` (null), `category_suggestion`.
- `category_suggestion`: `{ name, match_confidence, matched_pattern } | null` — computed by
  `taxonomy.rs` against an external taxonomy config file. Never hardcode taxonomy data in Rust source.
- Error envelope: `{ error: { stage, code, message, details } }`. Always this shape — never a bare
  panic or stack trace.

## CLI surface

```bash
doc-scanner convert --input <path> [--output <path>] [--pretty] [--taxonomy <path>] [--log-file <path>]
doc-scanner seed-report --input-dir <dir> [--format json|csv] [--taxonomy <path>] [--emit-sql <file>] [--log-file <path>]
```

`--log-file` is a global flag: it redirects the structured conversion log (see hard constraints
above) from stderr to the given file. Never affects stdout, which stays JSON-only.

Exit codes: `0` success, `1` generic, `2` bad args/input, `3` unsupported format, `4` extraction
failure, `5` classification failure, `6` output write failure.

## Crate layout

```text
doc_scanner_core/src/
  input.rs         # file type detection, validation
  extract/          # pdf.rs (text layer), image.rs (OCR)
  classify.rs       # bank_statement vs grocery_bill
  structure/        # bank_statement.rs, grocery_bill.rs
  taxonomy.rs       # merchant normalization + category_suggestion — shared by convert & seed-report
  schema.rs         # serde structs + JSON Schema validation
  error.rs          # typed errors -> JSON error envelope
  observability.rs  # OTEL-shaped structured log record for every convert call, see design doc §14
doc_scanner_cli/     # convert + seed-report subcommands
  src/logging.rs      # picks the tracing-subscriber writer (stderr or --log-file) for the process
doc_scanner_service/ # axum wrapper (deferred phase)
```

## Testing expectations

- Golden-file tests per sample document; every success output validates against its JSON Schema.
- Fuzz malformed PDFs/images — must always return a clean JSON error, never panic or hang.
- Stage-5 validation (line items sum to stated total, dates within range, etc.) is load-bearing —
  there's no LLM fallback here to catch a bad parse, so this isn't optional.

## Not this engine's job

Postgres schema, currency/category resolution against `merchant_category_rules`, tenant/user
attribution, audit logging, dashboards — all owned by the web app's ingestion layer. See design
doc §10 for the full mapping.
