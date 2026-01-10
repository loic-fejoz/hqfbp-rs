# Agent Guidance System: hqfbp-rs

## Mission
To provide a high-performance, robust Rust implementation of the **Hamradio Quick File Broadcasting Protocol (HQFBP)**. This project aims for full bit-accuracy with the [Python reference implementation](https://github.com/loic-fejoz/py-hqfbp/) to ensure seamless interoperability.

## Critical Commands
- **Build:** `cargo build`
- **Test:** `cargo test` (Run this ALWAYS before submitting changes)
- **Check/Lint:** `cargo clippy`
- **Format:** `cargo fmt`

## Project Map
- `src/lib.rs`: Atomic types (`Header`, `EncodingList`) and `unpack` primitive.
- `src/codec.rs`: Registry of all physical encodings (CRC, FEC, Compression).
- `src/generator.rs`: `PDUGenerator` logic for turning messages into chunks/PDUs.
- `src/deframer.rs`: `Deframer` logic for reassembly, quality tracking, and multi-PDU FEC.
- `src/bin/`: CLI tools for packing, unpacking, and noise simulation.
- `tests/`: Comprehensive test suite including regressions and performance power-tests.

## Documentation Index
Read these specialized docs in `agent_docs/` before starting specific tasks:
1. **[Architecture](file:///home/loic/projets/hqfbp-rs/agent_docs/architecture.md):** Data flow from serialized message to radio-ready PDUs and back. Read when modifying core logic.
2. **[Testing Guidelines](file:///home/loic/projets/hqfbp-rs/agent_docs/testing_guidelines.md):** How to use the simulation tools and regression suite. Read when adding features or fixing bugs.
3. **[Conventions](file:///home/loic/projets/hqfbp-rs/agent_docs/conventions.md):** Protocol-specific patterns (Boundary markers, quality metrics). Read before any implementation work.

> [!IMPORTANT]
> **Verification Requirement:**
> - You MUST verify all changes. Passing `cargo test` and maintaining high recovery rates in `cargo run --bin simulate` is required for all PRs.
> - **Zero Regression Rule:** EVERY bug fix MUST include a corresponding unit test (in `tests/` or `tests/test_regressions.rs`) that fails without the fix and passes with it.
