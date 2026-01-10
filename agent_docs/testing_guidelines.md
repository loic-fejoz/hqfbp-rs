# Testing Guidelines

This project uses a multi-tier testing strategy.

## 1. Unit Tests
Location: `tests/*.rs`.
Used for verifying individual components (e.g., [tests/test_rs.rs](file:///home/loic/projets/hqfbp-rs/tests/test_rs.rs)).

## 2. Regression Suite
Location: [tests/test_regressions.rs](file:///home/loic/projets/hqfbp-rs/tests/test_regressions.rs).
**CRITICAL:** Every bug fix related to protocol logic must add a test case here. This ensures fixes for announcement handling, PDU nesting, and shortened FEC blocks remain stable.

## 3. Simulation (Power Testing)
Location: [src/bin/simulate.rs](file:///home/loic/projets/hqfbp-rs/src/bin/simulate.rs).
Used to verify high-level performance metrics.
```bash
# Run a 100-iteration simulation at 10⁻³ BER
cargo run --bin simulate -- --ber 1e-3 --limit 100 --encodings gzip,h,rs(120,100) --ann-encodings h,crc16 --file-size 1024
```
Refer to [tests/test_rs_power.rs](file:///home/loic/projets/hqfbp-rs/tests/test_rs_power.rs) for an example of automated power-testing.

## 4. Cross-Platform Compatibility
Scripts: `test_cross_compat_*.sh`.
Validates bit-accuracy between the Rust and Python implementations. 
- **`test_cross_compat_py_rs.sh`**: Packs with Python, injects noise (optionally), and unpacks with Rust.
- **`test_cross_compat_rs_py.sh`**: Packs with Rust, injects noise (optionally), and unpacks with Python.

Requires the Python reference implementation (`https://github.com/loic-fejoz/py-hqfbp/`) to be available in the adjacent directory. These tests are essential for ensuring that any logic changes in Rust remain compatible with the protocol's master reference.

## Best Practices
- **Mocking Noise:** Use the `BitErrorChannel` in simulation contexts rather than manually flipping bits in unit tests.
- **Payload Diversity:** Test with small payloads (smaller than FEC `k`), medium (splitting into 2-3 chunks), and large (multi-chunk with RaptorQ).
- **Deterministic Randomness:** Use seeded RNGs in `tests/` if possible to make intermittent failures reproducible.

> [!CAUTION]
> **Always verify `cargo test` before submitting.** A single failing test in `test_generator.rs` or `test_hqfbp.rs` usually indicates a breakage of the protocol state machine.
