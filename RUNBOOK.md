# RUNBOOK

Run instructions for the `doc-scanner` CLI. See `AGENTS.md` / `design-doc-rust-doc-scanner.md`
for architecture and design rationale.

## Prerequisites

- Rust toolchain (`cargo`, `rustc`) — install via [rustup](https://rustup.rs) if not already present.
- No network access or external services required; the tool runs fully local.

## Build

```bash
cargo build --release
```

The binary is produced at `target/release/doc-scanner` (use `target/debug/doc-scanner` for a
debug build via plain `cargo build`).

## Usage

`convert` is the default subcommand, so `--input` can be passed directly:

```bash
doc-scanner --input statement.pdf
doc-scanner convert --input statement.pdf --pretty
doc-scanner convert --input statement.pdf --output result.json
doc-scanner convert --input statement.pdf --taxonomy taxonomy.json
```

- stdout carries exactly one JSON document (the success or error envelope) — safe to pipe.
- stderr carries structured logs only; redirect it away (`2>/dev/null`) for clean JSON, or pass
  `--log-file <path>` to send logs to a file instead (e.g. for a local Promtail/Loki setup):
  ```bash
  doc-scanner convert --input statement.pdf --log-file /var/log/doc-scanner/conversions.log
  ```
  Each log line reports `file_name`, `status` (`success`/`failed`), `bytes`, `processing_time_ms`,
  `document_type`, `extraction_method`, `content_hash`, and on failure `error.stage`/`error.code`.
  Logging is always local — no network call is made regardless of this flag.
- Only PDF input is currently supported. Image input (`.jpg`/`.png`) is accepted by the file-type
  detector but returns an `OCR_NOT_IMPLEMENTED` error — local OCR is a later phase, not yet built.
- No format-specific structuring parser is registered yet, so `convert` currently returns a clean
  `UNRECOGNIZED_FORMAT` error for every PDF. This is expected until a rule-based parser is added
  for a real bank statement / receipt layout.

### Exit codes

| Code | Meaning |
| --- | --- |
| 0 | Success |
| 1 | Generic failure |
| 2 | Invalid arguments or input file |
| 3 | Unsupported format |
| 4 | Extraction failure |
| 5 | Classification or structuring failure |
| 6 | Output write failure |

### seed-report

Mines a directory of sample documents for merchant strings, to bootstrap the category-rule seed
list (see design doc §10.2.1):

```bash
doc-scanner seed-report --input-dir ./samples
doc-scanner seed-report --input-dir ./samples --format csv
doc-scanner seed-report --input-dir ./samples --taxonomy taxonomy.json
doc-scanner seed-report --emit-sql reviewed-report.json
```

Files that fail to convert (including the current `UNRECOGNIZED_FORMAT` case above) are skipped
and logged to stderr rather than aborting the whole run.
