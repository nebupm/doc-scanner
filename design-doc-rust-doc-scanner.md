# Design Document: Document-to-JSON Conversion Engine

**Status:** Draft v0.4 — per-line enrichment metadata added (§5.5); no outstanding open questions
**Author:** (fill in)
**Date:** 2026-09-22

---

## 1. Purpose & scope

Build a Rust application that ingests a single file — a PDF or an image — representing a **bank statement** or a **grocery/retail bill**, and emits a structured **JSON** representation of its contents. Output defaults to stdout; a flag allows writing to a file. Failures are also reported as JSON, never as a bare stack trace or plain-text error.

This tool is the **extraction layer** of a larger system: a web application that ingests statements/bills from users, stores structured data in Postgres, and renders spend/finance dashboards. This document also covers the feasibility and shape of that integration.

### 1.1 Goals

- Deterministic, scriptable CLI behavior (stdin/stdout friendly, correct exit codes, clean JSON on both success and failure).
- One clean core library, reusable by a CLI now and an HTTP service later — no logic duplication.
- Structured output that's stable enough to build a Postgres schema and dashboards on top of.
- Reasonable accuracy across varied real-world layouts, not just one bank's PDF format.
- Enrich each extracted line with categorization-assisting metadata (normalized merchant, advisory suggested category) — without the engine ever deciding the real category itself. See §5.5.

### 1.2 Non-goals (for v1)

- Multi-page batch ingestion in a single invocation (v1 is one input file per invocation; batching is a caller-side concern).
- Full double-entry accounting/reconciliation logic — this tool extracts data, it doesn't validate financial correctness beyond basic sanity checks.
- Cross-document reconciliation — e.g. detecting that an itemized grocery receipt and a single "Grocery Mart" line on a credit card statement represent the same purchase. In v1, avoiding that double-count is the user's responsibility at data-entry time (exclude one side manually). See §10.6.
- A UI. This is a backend/CLI component only.

---

## 2. High-level architecture

Two diagrams above frame this: the **processing pipeline** inside the engine, and how the engine sits inside the **broader system** (CLI + service both wrapping one core library, feeding a web backend, Postgres, and dashboards).

The important structural decision: **all extraction/parsing logic lives in a library crate (`doc_scanner_core`)**. The CLI binary (`main.rs`) is a thin adapter: parse args → call library → serialize result → write to stdout/file. This is what makes the later "plug into a web app" question easy to answer — see §9.

```text
crates/
  doc_scanner_core/     # library: all business logic, no I/O side effects beyond what's passed in
    src/
      lib.rs
      input.rs          # file type detection, validation
      extract/
        pdf.rs           # text-layer PDFs
        image.rs          # OCR path
      classify.rs        # bank_statement vs grocery_bill
      structure/
        bank_statement.rs
        grocery_bill.rs
        llm_backend.rs    # optional LLM-assisted structuring
      taxonomy.rs        # merchant normalization + starter-taxonomy matching, see §5.5 — shared by
                          #   every `convert` call (per-line enrichment) and `seed-report` (aggregation)
      schema.rs          # serde structs for output + JSON Schema validation
      error.rs           # typed error enum -> JSON error envelope
  doc_scanner_cli/       # binary: clap args, stdout/file writing, exit codes
                          #   subcommand `convert` (default) — the main pipeline above
                          #   subcommand `seed-report` — merchant/category bootstrapping, see §10.2.1
  doc_scanner_service/   # binary: axum HTTP wrapper (phase 2, see §9)
```

---

## 3. Processing pipeline (detail)

| Stage | Input | Output | Notes |
| --- | --- | --- | --- |
| 1. Input layer | file path | validated file handle + detected type (`pdf`/`image`) | Magic-byte sniffing (don't trust the extension), size limit, corrupt-file check |
| 2. Extraction | validated file | raw text (+ optional layout/position data) | PDF: try text-layer extraction first (cheap, exact). If text layer is empty/sparse (scanned PDF) or input is an image, rasterize/decode and run OCR |
| 3. Classification | raw text | `bank_statement` \| `grocery_bill` \| `unknown` | Skippable if caller passes `--type` explicitly. Otherwise keyword/heuristic classifier, or delegate to the same LLM call used for structuring |
| 4. Structuring | raw text + doc type | typed struct matching schema (§5) | The hard part — see §4 for strategy tradeoffs |
| 5. Validation | structured data | validated data or validation errors | Schema conformance + sanity checks (e.g. sum of line items ≈ stated total, dates parse, currency consistent) |
| 6. Output | validated data or error | JSON on stdout or to `--output` file | Always JSON; stdout reserved for the payload, stderr reserved for logs/diagnostics |

---

## 4. The hard problem: structuring raw text into schema

This is where most of the engineering risk lives, so it's worth stating the tradeoff explicitly rather than burying it in implementation.

**Option A — Rule-based (regex/heuristics per issuer format).**
Fast, free, fully offline, deterministic, easy to unit test. But bank statement layouts vary enormously by bank (and even by account type within a bank), and grocery receipts vary by retailer/POS vendor. Realistically this means writing and maintaining a parser *per format*, which doesn't scale past a handful of known formats.

**Option B — LLM-assisted structuring.**
Extract raw text (via PDF text layer or OCR), then send it to an LLM with the target JSON schema and a strict "output only valid JSON matching this schema" instruction. This generalizes across formats without per-issuer parser code, at the cost of: network dependency, per-document latency/cost, and sending potentially sensitive financial data to a third-party API (a real consideration if these are genuine bank statements — see §10).

**Option C — Cloud document-AI APIs** (AWS Textract *Analyze Expense*, Google Document AI, Azure Document Intelligence).
Purpose-built for receipts/invoices/statements, often with layout-aware field extraction (better than plain-text OCR for tables and columns). Same data-sensitivity consideration as Option B, plus vendor lock-in and per-page pricing.

**Decision: Option A only, fully local.** No data — statement text, receipt text, or images — leaves the host. This rules out Option B and C entirely for v1. Concretely:

- OCR: local Tesseract (`leptess`), not a cloud Document-AI API.
- Structuring: rule-based parsers per known format (regex/heuristics for `qty x price` line items, `date / description / amount / balance` transaction rows), not an LLM call.
- Consequence: format coverage grows only as fast as you write parsers for new bank/retailer layouts. This is the direct cost of staying local-only, and it should shape the roadmap — §12 now assumes an ongoing "add a parser for format X" workstream rather than a one-time LLM-fallback build.
- Stage-5 validation (schema conformance + sanity checks like line-item sum ≈ stated total) is what catches a parser silently producing wrong data, since there's no LLM safety net to fall back on — so it's not optional, it's load-bearing.
- If a document's format isn't recognized by any parser, the correct behavior is a clean `error.code: "UNRECOGNIZED_FORMAT"` JSON output, not a best-effort guess. Recognizing that you don't have a parser for something is safer than partially parsing it wrong.
- Worth tracking as the parser library grows: which formats it covers, so it's obvious when a new bank/retailer needs a new parser versus hitting an existing one's edge case.

---

## 5. JSON schemas

### 5.1 Common envelope

```json
{
  "status": "success",
  "meta": {
    "input_file": "statement.pdf",
    "input_type": "pdf",
    "document_type": "bank_statement",
    "extraction_method": "pdf_text_layer",
    "processing_time_ms": 842,
    "engine_version": "0.1.0"
  },
  "data": { }
}
```

### 5.2 Bank statement `data`

```json
{
  "account": {
    "bank_name": "Example Bank",
    "account_holder_name": "Jane Doe",
    "account_type": "checking",
    "currency": "USD",
    "statement_period": { "start": "2026-08-01", "end": "2026-08-31" },
    "opening_balance": 1000.00,
    "closing_balance": 1245.50
  },
  "transactions": [
    {
      "date": "2026-08-05",
      "raw_text": "05/08 GROCERY MART #204 LONDON GB   -54.32",
      "description": "Grocery Mart #204",
      "merchant": "Grocery Mart",
      "debit": 54.32,
      "credit": null,
      "balance": 945.68,
      "category": null,
      "category_suggestion": { "name": "Groceries", "match_confidence": 0.7, "matched_pattern": "grocery" }
    }
  ]
}
```

`account_type` is one of `checking` \| `savings` \| `credit_card` \| `unknown`. It's required for correct downstream interpretation of debit/credit — see §10.1. There is deliberately no account number, masked or otherwise, and no card details anywhere in this schema: the product has no use for them, and not extracting them in the first place is simpler and safer than extracting-then-discarding.

### 5.3 Grocery bill `data`

```json
{
  "merchant": { "name": "Example Grocery", "address": "123 Main St", "phone": null },
  "receipt": { "date": "2026-09-20", "receipt_number": "R-88213", "payment_method": "card" },
  "items": [
    {
      "name": "Whole milk 1gal",
      "raw_text": "WHOLE MILK 1GAL           3.50 T",
      "quantity": 2,
      "unit_price": 3.50,
      "total_price": 7.00,
      "category": null,
      "category_suggestion": { "name": "Groceries", "match_confidence": 0.6, "matched_pattern": "milk" }
    }
  ],
  "totals": { "subtotal": 45.00, "tax": 3.60, "discount": 0.00, "total": 48.60 },
  "currency": "USD"
}
```

### 5.4 Error envelope

```json
{
  "status": "error",
  "meta": { "input_file": "receipt.jpg", "input_type": "image" },
  "error": {
    "stage": "ocr_extraction",
    "code": "OCR_LOW_CONFIDENCE",
    "message": "OCR confidence 42% is below the 70% threshold",
    "details": { "confidence": 0.42, "threshold": 0.70 }
  }
}
```

`category` fields are left `null` at extraction time deliberately — the engine has no basis for guessing a category from raw text alone, and shouldn't try. Categorization is a downstream concern, handled by the ingestion layer's merchant-rule matching; see §10.2.

### 5.5 Per-line enrichment metadata

Every bank transaction and grocery item carries three fields beyond the raw extracted data, to make categorization easier for whoever handles it next — without the engine itself ever making that call:

- **`raw_text`** — the original line exactly as extracted, before any cleanup. Kept for audit/debugging: if `merchant` or `description` got mangled by OCR or normalization, this is the ground truth to check against.
- **`merchant`** — a normalized merchant name derived from the raw description (strip trailing digits/reference codes, uppercase, collapse whitespace — a simple heuristic, not trying to be perfect). `"TESCO STORES 2211 LONDON GB"` becomes `"Tesco Stores"`; good enough for clustering and matching, not a claim of precision. `null` if nothing recognizable was found. Grocery items don't repeat this per line — the merchant is already the document-level `merchant.name`.
- **`category_suggestion`** — `{ name, match_confidence, matched_pattern } | null`. Computed by matching `merchant` (or the item name) against a small starter-taxonomy config (the same one `seed-report` uses, see §10.2.1) — an external JSON/TOML file, not hardcoded, so it's swappable without a rebuild. `match_confidence` is a small fixed lookup by match type (e.g. `0.9` for an exact match, `0.6` for a substring match) — a static heuristic, not a statistical model, consistent with the local-only, no-ML decision in §4.

This is advisory only. `category` stays authoritatively `null` from the engine every time — nothing here changes that. What it does change is what the ingestion layer has to work with: `category_suggestion` becomes a second fallback tier in the category-resolution order (§10.2), sitting between the tenant's `merchant_category_rules` and the "Uncategorized" catch-all, so a transaction has a decent chance of landing somewhere sensible even before any tenant-specific rules exist.

Because this enrichment now happens on every single `convert` call, `seed-report` (§10.2.1) no longer needs its own normalization logic — it just aggregates the `merchant` and `category_suggestion` fields that are already present in each document's output.

---

## 6. CLI specification

```bash
doc-scanner convert --input <path> [--output <path>] [--pretty] [--log-file <path>]

  --input, -i     Path to the PDF or image file (required)
  --output, -o    Write JSON to this file instead of stdout. Default: stdout
  --pretty        Pretty-print JSON (default is compact, better for piping)
  --log-file      Write structured logs (§14) to this file instead of stderr. Global flag —
                   also applies to `seed-report`. Default: stderr

doc-scanner seed-report --input-dir <dir> [--format json|csv] [--taxonomy <path>] [--log-file <path>]

  --input-dir       Directory of sample PDFs/images to mine for merchant strings (required)
  --format          Output format for the merchant/category report. Default: json
  --taxonomy        Path to the starter taxonomy config used for suggested_category. Optional —
                     falls back to a small built-in default set if omitted
  --emit-sql <file>  Instead of a report, read back a reviewed/edited report from <file> and
                     emit `merchant_category_rules` INSERT statements (source = 'system_seed')

`convert` is the default subcommand if none is specified, for backward-compatible single-command invocation.
```

- **stdout**: reserved exclusively for the JSON payload (success or error, or the seed report) — always exactly one JSON document per invocation.
- **stderr**: reserved for human-readable logs/diagnostics (via `tracing`), so `doc-scanner ... 2>/dev/null` gives you clean JSON every time. This matters for scripting and for the future service wrapper, which will reuse the same logging convention. `--log-file` redirects this to a file instead, still never mixing with stdout.
- **Exit codes**: `0` success, `1` generic failure, `2` invalid arguments/input file, `3` unsupported format, `4` extraction failure, `5` classification failure, `6` output write failure. The JSON error envelope's `error.code` carries the specific reason; exit code is coarse-grained for shell scripting. `seed-report` and `--emit-sql` reuse the same exit code conventions.

---

## 7. Technology choices (crates)

| Concern | Crate(s) | Notes |
| --- | --- | --- |
| CLI parsing | `clap` (derive) | |
| Serialization | `serde`, `serde_json` | |
| Error handling | `thiserror` (typed errors in the lib), `anyhow` (glue in the binary) | Typed errors in the lib map cleanly to `error.code` |
| PDF text extraction | `pdf-extract` or `lopdf`, or shelling to `pdftotext` (poppler-utils) | Prefer a pure-Rust crate if it's reliable enough; poppler shell-out is a pragmatic fallback |
| PDF → image rasterization | `pdfium-render`, or shell to `pdftoppm` | Needed for scanned/image-only PDFs |
| Image decoding | `image` | |
| Local OCR | `leptess` (Tesseract bindings) or `rusty-tesseract` | Requires the Tesseract system library — an ops dependency worth flagging. No cloud OCR/LLM crates needed given the local-only decision in §4 |
| JSON Schema validation | `jsonschema` | Validates structured output before it's returned; carries extra weight here since there's no LLM fallback to catch bad structuring |
| Structured logging | `tracing`, `tracing-subscriber` (JSON formatter, to stderr) | Records are shaped to match the OpenTelemetry Log Data Model's field names (see §14) — no OTLP exporter crate, no network call from this engine |
| HTTP service (phase 2) | `axum`, `tokio`, `tower` | |

---

## 8. Error taxonomy & operational hardening

Since this ingests arbitrary user-supplied files, treat it with the same suspicion you'd apply to any file-upload-handling service in production:

- **Resource limits**: cap input file size (config, e.g. 25 MB), cap OCR/rasterization time with a timeout, cap page count for multi-page PDFs.
- **Malformed input**: PDF parsers and image decoders are historically a source of memory-safety and DoS bugs (malformed PDFs, decompression bombs). Rust's memory safety helps, but a hostile file can still hang a thread or consume unbounded memory — enforce timeouts and size limits regardless.
- **No sensitive data in logs**: mask account numbers, names, etc. before anything hits `tracing`/stderr. The JSON payload on stdout is the only place full data should appear.
- **Idempotency**: hash the input file content; the caller (web app) can use that hash to detect duplicate uploads before even invoking the engine.

---

## 9. Feasibility: plugging this into a web frontend

Short answer: **yes, and your instinct is right that it belongs on the backend** — a browser can't invoke a native Rust binary or access the local filesystem the way this tool needs to. The frontend uploads a file to your web backend; the backend is what talks to this engine.

There are three ways to wire that up, in increasing order of operational maturity:

1. **Subprocess per request.** Web backend (any language) spawns the CLI binary per upload, captures stdout, parses the JSON. Simplest to build, works today, but pays process-spawn overhead on every request and doesn't let you reuse connections (e.g. to a cloud OCR/LLM API) across requests. Fine for low volume or an MVP.
2. **In-process library, if your web backend is also Rust.** Add `doc_scanner_core` as a dependency directly. No IPC, no subprocess, no network hop — just a function call. This is the cleanest option if you're open to a Rust web backend (e.g. `axum`).
3. **Standalone microservice.** Wrap `doc_scanner_core` in a small `axum` HTTP service (`POST /v1/convert`, multipart upload, same JSON envelope as the response body). Any web backend, in any language, calls this over HTTP. This is the best fit if your main web app is *not* Rust, or if you want the conversion engine to scale/deploy independently from the rest of the app (which, given your SRE background, is probably the instinct you already have — separate blast radius, separate scaling, separate resource limits for something that does CPU/OCR-heavy work).

Because the CLI was built with the logic isolated in a library crate from the start, **moving from option 1 to option 3 is additive, not a rewrite** — you write a new thin binary (`doc_scanner_service`) alongside the existing CLI binary, both calling the same `doc_scanner_core`.

**Recommended path:** build the CLI first (fast to iterate, easy to test with golden files), validate accuracy on real sample documents, then add the `axum` service wrapper once you're ready to wire up the web app. Keep the CLI around after that — it stays useful for local debugging, batch backfills, and support tooling. Given current scope is occasional/personal use (§13), option 1 (subprocess-per-request) is a perfectly reasonable stopping point — there's no need to build the microservice wrapper until volume or deployment requirements actually call for it.

---

## 10. Postgres integration

This section reflects the actual deployed schema (multi-tenant, Drizzle-managed). One principle threads through all of it: **the Rust engine stays completely tenant-agnostic.** It never sees `tenant_id`, never resolves a category or currency, never decides what's a real expense. It extracts faithfully; every business rule below belongs to the ingestion layer (the web app backend) at insert time. This keeps the engine simple and reusable, and keeps tenant/business logic in one place — the layer that already owns it.

### 10.1 Account types and credit card handling

A checking or savings account and a credit card statement need different interpretation of the same `debit`/`credit` fields:

| `account.account_type` | `debit` present | `credit` present |
| --- | --- | --- |
| `checking` / `savings` | Outflow → expense (category type `expense`) | Inflow → income (category type `income`) |
| `credit_card` | New charge → expense (category type `expense`) | **Payment against last cycle's balance — not new spend.** Filed under a system category of type `transfer` (e.g. "Credit Card Payment"), so it never counts as spend in any expense-total query |

This needs no new column: the existing `categories.type` enum already has `transfer` for exactly this case. It does need one seeded system category per tenant, of type `transfer`, that credit-card payment rows get routed to automatically. Any dashboard that sums "spend" should filter to `categories.type = 'expense'` — which it should be doing anyway, since `income`/`savings`/`billing` rows shouldn't be counted as spend either.

Known simplification, worth stating rather than quietly hiding: a genuine merchant *refund* on a credit card also arrives as a `credit`, and this rule will file it the same way as a payment (as a `transfer`), not as a negative expense. Fine for v1 given the manual-correction workflow in §10.6; a smarter refund/payment distinction is a later-phase refinement, not a v1 requirement.

### 10.2 Category resolution strategy

The engine leaves `category` null; something has to turn merchant/description text into a real `category_id` before insert, since the column is `NOT NULL`. Proposed layered approach — no ML, no external calls, consistent with the local-only decision in §4:

1. **`merchant_category_rules` table** (new — see migration below): maps a merchant/description pattern to a category. Seeded platform-wide with common merchants (supermarkets → groceries, known airlines → travel, etc.) as `source = 'system_seed'`.
2. **Self-learning from corrections.** When a user re-files an expense from "Uncategorized" to something else in the UI, write a new rule capturing that merchant string → chosen category, scoped to the tenant, `source = 'user_correction'`. Tenant-scoped user rules take priority over global system seeds. This means accuracy on *that tenant's* real merchants improves automatically over time, with zero model training.
3. **Fall back to the engine's own `category_suggestion`** (§5.5), if `merchant_category_rules` produced no match and `match_confidence` clears a minimum threshold (e.g. exact matches only, not weak substring matches). This costs nothing — the field is already sitting in the document's JSON — and gives every transaction one more real chance before landing in "Uncategorized," even for a tenant with no custom rules yet.
4. **Final fallback.** Nothing matched → tenant's existing `is_system` "Uncategorized" category, exactly as today.
5. **Future refinement (not v1):** fuzzy/edit-distance matching for merchant-name variants, or a small local statistical classifier trained per tenant on their own correction history. Still fully local if it's ever built — flagged here so it's not forgotten, not because it's needed now.

This is inherently an 80/20 system that gets better with usage; "Uncategorized" staying nonzero for a while after launch is expected, not a bug.

#### 10.2.1 Bootstrapping the seed list from real documents

Step 1 above needs *something* to seed with. Rather than inventing a generic merchant list, doc-scanner gets a second subcommand purpose-built to mine it from the real sample documents already available (§13):

```
doc-scanner seed-report --input-dir <dir> [--format json|csv] [--taxonomy <path>]
```

- Runs the same extraction/structuring pipeline as the main conversion path (reuses `doc_scanner_core` — no new parsing or normalization logic needed here) over every file in the directory.
- Since every document's JSON already includes a normalized `merchant` field and a `category_suggestion` (§5.5) — computed once, by the same `taxonomy.rs` module, regardless of which subcommand triggered it — `seed-report` just **aggregates those already-computed fields** across all files: no separate normalization step of its own.
- Produces a ranked list: `{ merchant, occurrences, example_raw_text: [...], suggested_category }`, sorted by frequency.
- Default output is JSON, consistent with the rest of the tool; `--format csv` is worth supporting here specifically, since — unlike the main conversion output — this is meant to be opened, reviewed, and hand-edited before anything reaches the database.

This is explicitly a **human-in-the-loop, offline bootstrapping tool** — it runs once (or occasionally, as new samples arrive), it's outside the production ingestion path, and nothing here writes to Postgres automatically. The workflow is: run `seed-report` → review/edit the list (merge merchants the heuristic split apart, fix a wrong suggested category, delete ones you don't want seeded) → feed the finalized list back in with `--emit-sql`, which converts it into ready-to-run `INSERT INTO merchant_category_rules (..., source) VALUES (..., 'system_seed')` statements for §10.7's migration. This lives in `doc_scanner_cli` (as a second subcommand alongside the existing conversion command), not in `doc_scanner_core` — it's a batch/reporting driver over the core library's per-document output, not a separate implementation of the enrichment logic itself.

### 10.3 Currency: make it a global reference table, not per-tenant

Today `currencies` is tenant-scoped (`tenant_id NOT NULL`, unique on `(tenant_id, code)`), which is exactly the problem you flagged — nothing stops one tenant from having a row where `code = 'USD'` and `name = 'USDollar'`. Currencies should be a single platform-owned lookup seeded from ISO 4217, where tenants only *select* a code, never author one.

This is a **breaking migration**, not additive — worth treating with the same care as any production schema change with live FKs pointing at the table being restructured:

1. **Consolidate duplicates.** For every distinct `code` across all tenants' existing `currencies` rows, keep exactly one canonical row (prefer an existing `is_system = true` row; otherwise the oldest). For any ISO code with no existing row at all, insert it fresh from the standard ISO 4217 list.
2. **Repoint foreign keys.** Update `expenses.currency_id`, `budgets.currency_id`, and `expense_recurrences.currency_id` on every row currently pointing at a soon-to-be-deleted duplicate, so they point at the canonical row for that code instead.
3. **Delete the now-orphaned duplicate rows.**
4. **Drop tenant scoping:**
   ```sql
   ALTER TABLE public.currencies DROP CONSTRAINT currencies_tenant_id_tenants_id_fk;
   DROP INDEX public.uq_currencies_code_per_tenant;
   DROP INDEX public.idx_currencies_tenant_id;
   DROP INDEX public.idx_currencies_active;
   ALTER TABLE public.currencies DROP COLUMN tenant_id;
   ALTER TABLE public.currencies ADD CONSTRAINT uq_currencies_code UNIQUE (code);
   CREATE INDEX idx_currencies_active ON public.currencies USING btree (is_active);
   ```
5. **Seed the full ISO 4217 list** (~180 currencies), not just the ones already in use — load once from a standard ISO 4217 dataset rather than hand-typing it. Sample shape:
   ```sql
   INSERT INTO public.currencies (code, symbol, name, is_active, is_system) VALUES
     ('USD', '$', 'US Dollar', true, true),
     ('GBP', '£', 'British Pound', true, true),
     ('EUR', '€', 'Euro', true, true),
     ('INR', '₹', 'Indian Rupee', true, true)
     -- ... remaining ISO 4217 currencies
   ON CONFLICT (code) DO NOTHING;
   ```
6. `is_system` no longer means much once tenants can't author currencies at all — repurpose it (or drop it) as a simple "shown in the picker by default" flag if you want a curated subset surfaced before "show all currencies."

Since steps 1–3 depend on what's actually in the live data (how many duplicate rows exist, whether any tenant has non-standard codes), this needs a real migration script run and reviewed against production data, not just the DDL above — flagging that explicitly given how this class of change tends to bite in production.

### 10.4 Audit trail

The existing `audit_logs` table is an HTTP-request log (method, path, status code) — the right thing for API observability, the wrong shape for "what happened to this person's financial data." Given money data, a dedicated ingestion audit trail is worth having as its own table, separate from both `audit_logs` and `source_documents` (which is a data snapshot, not an event log):

```sql
CREATE TABLE public.document_ingestion_audit (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    tenant_id uuid NOT NULL,
    user_id uuid,
    source_document_id uuid,
    expense_id uuid,
    event_type character varying(50) NOT NULL,  -- 'uploaded' | 'extraction_succeeded' | 'extraction_failed'
                                                  -- | 'expenses_inserted' | 'expense_recategorized'
                                                  -- | 'expense_excluded' | 'expense_reprocessed'
    details jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT document_ingestion_audit_pkey PRIMARY KEY (id),
    CONSTRAINT document_ingestion_audit_tenant_id_tenants_id_fk
        FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE,
    CONSTRAINT document_ingestion_audit_user_id_users_id_fk
        FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL,
    CONSTRAINT document_ingestion_audit_source_document_id_fk
        FOREIGN KEY (source_document_id) REFERENCES public.source_documents(id) ON DELETE SET NULL,
    CONSTRAINT document_ingestion_audit_expense_id_fk
        FOREIGN KEY (expense_id) REFERENCES public.expenses(id) ON DELETE SET NULL
);
CREATE INDEX idx_ingestion_audit_tenant_created ON public.document_ingestion_audit USING btree (tenant_id, created_at);
CREATE INDEX idx_ingestion_audit_source_document ON public.document_ingestion_audit USING btree (source_document_id);
```

Append-only — rows are never updated or deleted. This is what lets you answer "who changed this transaction's category, and when" or "did this upload actually produce the expenses I'm looking at," which neither `audit_logs` nor a mutable `expenses` row can answer on its own.

### 10.5 Statement header data

Confirmed relevant: bank name, account holder name, and (needed for §10.1's logic even though not explicitly requested) account type. Not relevant: account/card numbers. One new table, one row per bank-statement upload:

```sql
CREATE TABLE public.bank_statement_headers (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    tenant_id uuid NOT NULL,
    source_document_id uuid NOT NULL,
    bank_name character varying(150),
    account_holder_name character varying(150),
    account_type character varying(20) NOT NULL,  -- 'checking' | 'savings' | 'credit_card' | 'unknown'
    statement_period_start date,
    statement_period_end date,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT bank_statement_headers_pkey PRIMARY KEY (id),
    CONSTRAINT chk_bank_statement_headers_account_type
        CHECK (account_type IN ('checking','savings','credit_card','unknown')),
    CONSTRAINT bank_statement_headers_tenant_id_tenants_id_fk
        FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE,
    CONSTRAINT bank_statement_headers_source_document_id_fk
        FOREIGN KEY (source_document_id) REFERENCES public.source_documents(id) ON DELETE CASCADE,
    CONSTRAINT uq_bank_statement_headers_source_document UNIQUE (source_document_id)
);
```

Grocery bills have no header row — this table only populates when `source_documents.document_type = 'bank_statement'`.

### 10.6 Line items and the manual-correction workflow

Confirmed: **one `expenses` row per line item**, never one row per receipt/statement total — matches the default already in this doc. Merchant name goes in `expenses.tags`.

The known gap this creates: an itemized grocery receipt (50 individual items) and the corresponding single "Grocery Mart" line on a credit card statement both describe the same real-world purchase, and nothing here detects that overlap — it's an explicit v1 non-goal (§1.2). What the schema *should* provide is a clean way for the user to resolve it themselves without losing history, which argues for a soft-exclude rather than a hard delete:

```sql
ALTER TABLE public.expenses
    ADD COLUMN status character varying(10) DEFAULT 'active' NOT NULL;
ALTER TABLE public.expenses
    ADD CONSTRAINT chk_expenses_status CHECK (status IN ('active','excluded','superseded'));
CREATE INDEX idx_expenses_status ON public.expenses USING btree (tenant_id, status);
```

`'excluded'` covers the double-count case above (e.g. the user excludes the one card-statement line once they've entered the itemized receipt separately) and any other "this shouldn't count, but don't erase it" correction. `'superseded'` covers "this row was replaced by a corrected one." Every status change should also write a `document_ingestion_audit` row (§10.4) — the two features are meant to work together: soft-delete for reversibility, audit log for "who did this and why."

Automated cross-document reconciliation (matching a receipt total to a statement line by amount + date + merchant) is a reasonable phase-2+ idea once there's real usage data to see how often it'd actually help — not a v1 requirement.

### 10.7 Full migration

Combining everything additive from this section (currency migration in §10.3 is separate and should run on its own, given its data-dependent steps):

```sql
-- Document/JSON audit trail (data snapshot)
CREATE TABLE public.source_documents (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    tenant_id uuid NOT NULL,
    uploaded_by uuid,
    document_type character varying(30) NOT NULL,
    input_type character varying(10) NOT NULL,
    status character varying(10) NOT NULL,
    content_hash character varying(64) NOT NULL,
    engine_version character varying(20),
    raw_json jsonb NOT NULL,
    error_code character varying(100),
    error_message text,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT source_documents_pkey PRIMARY KEY (id),
    CONSTRAINT chk_source_documents_status CHECK (status IN ('success','error')),
    CONSTRAINT chk_source_documents_document_type CHECK (document_type IN ('bank_statement','grocery_bill')),
    CONSTRAINT chk_source_documents_input_type CHECK (input_type IN ('pdf','image')),
    CONSTRAINT source_documents_tenant_id_tenants_id_fk
        FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE,
    CONSTRAINT source_documents_uploaded_by_users_id_fk
        FOREIGN KEY (uploaded_by) REFERENCES public.users(id) ON DELETE SET NULL
);
CREATE INDEX idx_source_documents_tenant_id ON public.source_documents USING btree (tenant_id);
CREATE INDEX idx_source_documents_status ON public.source_documents USING btree (tenant_id, status);
CREATE UNIQUE INDEX uq_source_documents_tenant_hash
    ON public.source_documents USING btree (tenant_id, content_hash);

-- Statement header metadata (§10.5)
CREATE TABLE public.bank_statement_headers (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    tenant_id uuid NOT NULL,
    source_document_id uuid NOT NULL,
    bank_name character varying(150),
    account_holder_name character varying(150),
    account_type character varying(20) NOT NULL,
    statement_period_start date,
    statement_period_end date,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT bank_statement_headers_pkey PRIMARY KEY (id),
    CONSTRAINT chk_bank_statement_headers_account_type
        CHECK (account_type IN ('checking','savings','credit_card','unknown')),
    CONSTRAINT bank_statement_headers_tenant_id_tenants_id_fk
        FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE,
    CONSTRAINT bank_statement_headers_source_document_id_fk
        FOREIGN KEY (source_document_id) REFERENCES public.source_documents(id) ON DELETE CASCADE,
    CONSTRAINT uq_bank_statement_headers_source_document UNIQUE (source_document_id)
);

-- Merchant → category rules (§10.2)
CREATE TABLE public.merchant_category_rules (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    tenant_id uuid,  -- NULL = global system seed, applies to all tenants
    match_type character varying(10) NOT NULL,  -- 'exact' | 'contains' | 'regex'
    pattern character varying(255) NOT NULL,
    category_id uuid NOT NULL,
    priority integer DEFAULT 100 NOT NULL,       -- lower = higher priority; user_correction rules should be lower than system_seed
    source character varying(20) NOT NULL,        -- 'system_seed' | 'user_correction'
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT merchant_category_rules_pkey PRIMARY KEY (id),
    CONSTRAINT chk_merchant_rules_match_type CHECK (match_type IN ('exact','contains','regex')),
    CONSTRAINT chk_merchant_rules_source CHECK (source IN ('system_seed','user_correction')),
    CONSTRAINT merchant_category_rules_tenant_id_tenants_id_fk
        FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE,
    CONSTRAINT merchant_category_rules_category_id_categories_id_fk
        FOREIGN KEY (category_id) REFERENCES public.categories(id) ON DELETE CASCADE
);
CREATE INDEX idx_merchant_rules_tenant ON public.merchant_category_rules USING btree (tenant_id, priority);

-- Dedicated ingestion event log (§10.4)
CREATE TABLE public.document_ingestion_audit (
    id uuid DEFAULT gen_random_uuid() NOT NULL,
    tenant_id uuid NOT NULL,
    user_id uuid,
    source_document_id uuid,
    expense_id uuid,
    event_type character varying(50) NOT NULL,
    details jsonb,
    created_at timestamp with time zone DEFAULT now() NOT NULL,
    CONSTRAINT document_ingestion_audit_pkey PRIMARY KEY (id),
    CONSTRAINT document_ingestion_audit_tenant_id_tenants_id_fk
        FOREIGN KEY (tenant_id) REFERENCES public.tenants(id) ON DELETE CASCADE,
    CONSTRAINT document_ingestion_audit_user_id_users_id_fk
        FOREIGN KEY (user_id) REFERENCES public.users(id) ON DELETE SET NULL,
    CONSTRAINT document_ingestion_audit_source_document_id_fk
        FOREIGN KEY (source_document_id) REFERENCES public.source_documents(id) ON DELETE SET NULL,
    CONSTRAINT document_ingestion_audit_expense_id_fk
        FOREIGN KEY (expense_id) REFERENCES public.expenses(id) ON DELETE SET NULL
);
CREATE INDEX idx_ingestion_audit_tenant_created ON public.document_ingestion_audit USING btree (tenant_id, created_at);
CREATE INDEX idx_ingestion_audit_source_document ON public.document_ingestion_audit USING btree (source_document_id);

-- Traceability + soft-correction on expenses (§10.6)
ALTER TABLE public.expenses ADD COLUMN source_document_id uuid;
ALTER TABLE public.expenses
    ADD CONSTRAINT expenses_source_document_id_source_documents_id_fk
    FOREIGN KEY (source_document_id) REFERENCES public.source_documents(id) ON DELETE SET NULL;
CREATE INDEX idx_expenses_source_document_id ON public.expenses USING btree (source_document_id);

ALTER TABLE public.expenses ADD COLUMN status character varying(10) DEFAULT 'active' NOT NULL;
ALTER TABLE public.expenses
    ADD CONSTRAINT chk_expenses_status CHECK (status IN ('active','excluded','superseded'));
CREATE INDEX idx_expenses_status ON public.expenses USING btree (tenant_id, status);
```

Run the currency migration (§10.3) as a separate change — it touches existing FK data, whereas everything above is purely additive (new tables, nullable/defaulted new columns) and safe to ship without a maintenance window.

### 10.8 Ingestion mapping (engine JSON → tables)

| Engine JSON field | Destination | Resolution needed |
| --- | --- | --- |
| Whole envelope | `source_documents` row (1 per upload) | Attach `tenant_id`, `uploaded_by` from the authenticated request |
| `account.bank_name`, `account.account_holder_name`, `account.account_type`, `account.statement_period` | `bank_statement_headers` (bank statements only) | 1:1 with the `source_documents` row |
| Bank txn `date` / grocery `receipt.date` | `expenses.date` | Direct copy |
| Bank txn `description` / grocery item `name` (+ merchant) | `expenses.description`, `expenses.tags` | Merchant/store name goes in `tags` |
| Bank txn `debit`/`credit` / grocery item `total_price` | `expenses.amount` | Always positive; direction and "does this count as spend" come from category type, per §10.1 |
| `currency` (ISO code) | `expenses.currency_id` | Look up `code` in the now-global `currencies` table (§10.3) — no tenant scoping |
| `category` (nullable) + `merchant`/`category_suggestion` (§5.5) | `expenses.category_id` | Run `merchant_category_rules` (§10.2) first; if unmatched, fall back to `category_suggestion` if confident enough; otherwise tenant's `is_system` "Uncategorized". Credit-card payment rows are forced to the "Credit Card Payment" transfer-type system category regardless of any of the above |
| — | `expenses.source_document_id`, `expenses.status = 'active'` | FK for traceability; status flips to `'excluded'`/`'superseded'` only on later manual correction |
| Grocery item `quantity`, `unit_price` | *(no dedicated columns)* | Folded into `description`, e.g. `"Whole milk 1gal x2 @ $3.50"` |
| Every insert/correction | `document_ingestion_audit` row | Append-only event log, per §10.4 |

Every ingested row from one document shares the same `source_document_id`; combined with `status`, "undo this import" becomes `UPDATE expenses SET status = 'excluded' WHERE source_document_id = ...` — reversible, and it leaves a trail, rather than a hard `DELETE`.

### 10.9 Dashboards

Two reasonable paths, not mutually exclusive:
- **Off-the-shelf BI** (Grafana or Metabase) pointed at Postgres — fastest to stand up, good for internal/ops-style dashboards. Filter `status = 'active'` and `categories.type = 'expense'` for any "spend" view.
- **Custom web dashboard** — your web app's own API layer runs aggregate SQL (grouped by `categories.type`, `category_id`, month) and a JS charting library (Chart.js, Recharts) renders it. Better fit if the dashboard needs to be user-facing and tightly integrated with your app's auth/UX, and it already follows the tenant-scoping pattern the rest of the schema uses.

---

## 11. Testing strategy

- **Golden-file tests**: a corpus of sample PDFs/images with hand-verified expected JSON output; run on every change.
- **Schema/contract tests**: every success output validates against the JSON Schema for its document type; every error output validates against the error schema.
- **Fuzz testing**: malformed/truncated PDFs and images fed to the input layer, asserting it always returns a clean JSON error and never panics or hangs.
- **Sanity-check tests on structuring**: line items sum to subtotal within rounding tolerance, dates fall within the stated statement period, etc.

---

## 12. Phased roadmap

| Phase | Deliverable |
| --- | --- |
| 0 | This design doc, JSON schemas finalized, sample documents collected |
| 1 | CLI MVP: PDF text-layer extraction + rule-based parsing for one bank format and one receipt format, JSON to stdout |
| 2 | OCR path for scanned PDFs and images (local Tesseract) |
| 3 | Expand rule-based parser coverage (additional bank/retailer formats) as new samples are collected |
| 4 (defer) | Extract into `doc_scanner_core` lib (recommended from the start regardless) and build the `axum` microservice wrapper — low priority at occasional-use volume (§13); the CLI called via subprocess is sufficient until that changes |
| 5 | Postgres migration: additive tables (§10.7), then the currency global-reference migration (§10.3) as its own reviewed change; run `doc-scanner seed-report` (§10.2.1) over the sample documents, review/edit the result, and `--emit-sql` it into the `merchant_category_rules` seed data alongside per-tenant "Uncategorized"/"Credit Card Payment" system categories |
| 6 | Ingestion API in the web app (mapping in §10.8), wired to the engine |
| 7 | Dashboard/reporting layer |
| 8 (later) | Automated cross-document reconciliation (grocery receipt ↔ card statement line) — only once real usage data shows how often it'd help; see §1.2, §10.6 |

---

## 13. Open questions for you

None remaining that need your input right now — the last three are resolved below. This doc is otherwise ready to move on from "design" into "build."

**Decided this round:**
- **Sample documents**: confirmed available. Use them for two things at once: seed the golden-file test corpus (§11), and derive the initial `merchant_category_rules` seed rows (§10.2) from the merchant/description strings that actually appear in them — real data beats a generic starter list, and it's no extra work since the samples are being processed anyway.
- **Volume/scale**: occasional/personal use for now; production-scale concerns explicitly deferred. Practical effect: no rush to the `axum` microservice (§9 option 3) — the subprocess-per-request approach (§9 option 1) is perfectly adequate at this volume, and phase 4 in the roadmap can move later without cost. Revisit if/when usage grows.
- **Merchant seed list**: not a separate taxonomy decision — it's just the initial rows in `merchant_category_rules`, generated from your own sample documents rather than invented generically (see above).

**Previously decided:**
- Structuring is local-only — Tesseract OCR + rule-based parsers, no LLM/cloud calls (§4).
- Document type is auto-detected by the engine's classifier, consistent with keeping ingestion agnostic to anything the web app knows ahead of time.
- Credit card payments are filed under a `transfer`-type system category, not counted as expense (§10.1).
- Category resolution is rule-table + self-learning from user corrections, no ML (§10.2).
- Currencies become a single global ISO 4217 reference table; tenants select, never author (§10.3).
- A dedicated `document_ingestion_audit` event log is added, separate from the HTTP-level `audit_logs` (§10.4).
- Bank name, account holder name, account type, and statement period are persisted structurally; no account/card numbers (§10.5).
- One `expenses` row per line item, with a soft `status` column (`active`/`excluded`/`superseded`) for manual corrections instead of hard deletes (§10.6).

---

## 14. Observability: local, OTEL-compliant conversion logging

A cross-cutting concern from phase 1 onward (not deferred): every `convert` invocation must emit
exactly one structured log record describing what happened, in addition to the JSON payload on
stdout. This is what lets an operator answer "how many conversions failed today, and why" without
grepping stdout payloads or wiring up anything beyond what already runs on the local machine.

### 14.1 Local-only, still OTEL-shaped

This stays inside the local-only hard constraint (§4): the engine never makes a network call, so
there is no OTLP exporter and no collector endpoint configured here. What "OTEL-compliant" means
concretely is that the log record's *semantic fields* — `file_name`, `status`, `bytes`,
`processing_time_ms`, `document_type`, `extraction_method`, `content_hash`, `error.stage`,
`error.code` — mirror the attribute-style content of the [OpenTelemetry Log Data
Model](https://opentelemetry.io/docs/specs/otel/logs/data-model/), rather than an ad hoc field
layout. **Decided:** the outer envelope key names use `tracing-subscriber`'s own JSON formatter
(`timestamp`, `level`, `target`, `fields`) as-is, not the OTEL spec's literal
`Timestamp`/`SeverityText`/`Body`/`Attributes`/`Resource` key names — building a custom serializer
to match those exactly was considered and rejected as not worth the extra code for a local-only
tool. Any log-shipping agent (Promtail, Vector, the Grafana Agent) parses `tracing-subscriber`'s
JSON shape natively, and forwards to a log store (e.g. Loki) with no changes needed on this
engine's side.

### 14.2 Record shape

One record per `convert` call, emitted via `tracing::info!`/`tracing::error!` and rendered by
`tracing-subscriber`'s JSON formatter (consistent with §7), written to stderr by default or to
`--log-file <path>` (§6) when given — never to stdout, which stays reserved for the JSON payload:

```json
{
  "timestamp": "2026-09-25T10:04:12.841Z",
  "level": "INFO",
  "target": "doc_scanner_core::observability",
  "fields": {
    "message": "conversion completed",
    "file_name": "statement.pdf",
    "status": "success",
    "bytes": 184320,
    "processing_time_ms": 842,
    "document_type": "bank_statement",
    "extraction_method": "pdf_text_layer",
    "content_hash": "b1946ac92492d2347c6235b4d2611184a1b2c3d4e5f60718293a4b5c6d7e8f9"
  }
}
```

On failure, `status` is `"failed"`, `level` is `"ERROR"`, `message` is `"conversion failed"`, and
`fields` additionally carries `error.stage` and `error.code` — the same values that land in the
JSON error envelope's `error.stage`/`error.code` (§5.4), so a log line and its corresponding stdout
payload can be cross-checked without duplicating the error taxonomy.

| Attribute | Always present? | Notes |
| --- | --- | --- |
| `file_name` | Yes | Base name of the input file, not the full path — avoid leaking local filesystem layout into logs |
| `status` | Yes | `success` \| `failed` |
| `bytes` | Best-effort | Size of the input file. Present whenever the file could be `stat`'d, even if validation failed before a full read (e.g. `INPUT_TOO_LARGE`); absent only when the path itself doesn't resolve (e.g. `INPUT_NOT_FOUND`) |
| `processing_time_ms` | Yes | Same value as the JSON envelope's `meta.processing_time_ms` |
| `document_type` | Only if classification succeeded | `bank_statement` \| `grocery_bill` |
| `extraction_method` | Only if extraction succeeded | `pdf_text_layer` \| `ocr` |
| `content_hash` | Only if the file was fully read | SHA-256 hex digest of the input file bytes — reuses §8's idempotency hash, so a log line can be correlated with a specific upload. Used for correlation/dedup only, not integrity |
| `error.stage` | Only on failure | One of the pipeline stages, §3 |
| `error.code` | Only on failure | Matches `error.code` in the JSON error envelope |

### 14.3 Where this lives in the crate layout

`doc_scanner_core/src/observability.rs` owns building and emitting this record from the pipeline
result — the same `EngineError`/`Envelope` values already produced at the end of `convert()`, so no
pipeline stage needs to know logging exists. `doc_scanner_cli` and any future `doc_scanner_service`
binary both get this for free, consistent with the "no logic duplication" goal in §1.1: the record
is emitted once, from inside the library, not re-derived by each binary.

`doc_scanner_cli/src/logging.rs` is the CLI-only piece: it picks the `tracing-subscriber` writer
(stderr, or the `--log-file` path) based on CLI args before `observability.rs`'s calls ever fire.
This split keeps the library ignorant of "CLI flag" as a concept — the library only knows how to
build and emit a record; the binary decides where the configured writer sends it.

### 14.4 Forward compatibility with a containerized/service deployment

Nothing about this design assumes a CLI invocation specifically — `file_name`, `bytes`, and
`content_hash` describe the *input*, not the process. When this engine eventually runs inside a
container behind a frontend/backend (§9 option 2 or 3), the same `observability.rs` code emits the
same shape to stdout/stderr of that container, where it's picked up by whatever the container
platform already uses for log collection (e.g. a Fluent Bit/Vector sidecar, or the platform's
native log driver) — no code change required in this engine to support that move, only a
change in how the surrounding infrastructure collects stderr.
