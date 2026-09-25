# doc-scanner

A local Rust CLI that converts bank statements and grocery/retail bills (PDF or image) into
structured JSON — the extraction layer for a larger expense-tracking app.

Everything runs on the local machine. PDF text extraction is local, OCR is local Tesseract, and structuring is rule-based parsers.

- **Build and run it**: see [`RUNBOOK.md`](RUNBOOK.md).
- **Architecture, JSON contract, and design rationale**: see [`AGENTS.md`](AGENTS.md) (quick
  reference) and [`design-doc-rust-doc-scanner.md`](design-doc-rust-doc-scanner.md) (full design).
