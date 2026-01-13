# Testing Guidelines

This project uses a multi-tier testing strategy.

## 1. Unit Tests
Location: `tests/*.rs`.
Used for verifying individual components (e.g., [tests/test_rs.rs](file:///home/loic/projets/hqfbp-rs/tests/test_rs.rs)).

## 2. Regression Suite
Location: [tests/test_regressions.rs](file:///home/loic/projets/hqfbp-rs/tests/test_regressions.rs).
**CRITICAL:** EVERY bug fix MUST be accompanied by a new unit test. Protocol-level bugs should be added to the regression suite here, while component-specific bugs can be added to their respective unit tests in `tests/*.rs`. This ensures the bug never returns and documents the fix.

## 3. Simulation (Power Testing)
Location: [src/bin/simulate.rs](file:///home/loic/projets/hqfbp-rs/src/bin/simulate.rs).
Used to verify high-level performance metrics.
```bash
# Run a 100-iteration simulation at 10⁻³ BER
cargo run --bin simulate -- --ber 1e-3 --limit 100 --encodings gzip,h,rs(120,100) --ann-encodings h,crc16 --file-size 1024
```
Refer to [tests/test_rs_power.rs](file:///home/loic/projets/hqfbp-rs/tests/test_rs_power.rs) for an example of automated power-testing.

## 4. Cross-Implementation & Interoperability
This is the most critical verification layer for ensuring bit-accuracy with the Python reference implementation.

- **Automated Benchmarking:** `make test-py-bench` runs a comprehensive suite against Python-generated samples. See [Cross-Implementation Testing](file:///home/loic/projets/hqfbp-rs/agent_docs/cross_implementation_testing.md) for details.
- **Round-Trip Shell Scripts:**
    - **`test_cross_compat_py_rs.sh`**: Python Pack -> Rust Unpack.
    - **`test_cross_compat_rs_py.sh`**: Rust Pack -> Python Unpack.

Requires the Python reference implementation to be available in the adjacent directory (`../py-hqfbp`).

## Best Practices
- **Mocking Noise:** Use the `BitErrorChannel` in simulation contexts rather than manually flipping bits in unit tests.
- **Payload Diversity:** Test with small payloads (smaller than FEC `k`), medium (splitting into 2-3 chunks), and large (multi-chunk with RaptorQ).
- **Deterministic Randomness:** Use seeded RNGs in `tests/` if possible to make intermittent failures reproducible.

> [!CAUTION]
> **Always verify `cargo test` before submitting.** A single failing test in `test_generator.rs` or `test_hqfbp.rs` usually indicates a breakage of the protocol state machine.
