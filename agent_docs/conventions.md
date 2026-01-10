# Conventions & Patterns

This document outlines implementation logic patterns to maintain protocol consistency.

## 1. Naming & Enums
- **ContentEncoding:** Atomic transformations are defined in the `ContentEncoding` enum ([src/lib.rs](file:///home/loic/projets/hqfbp-rs/src/lib.rs)). 
- **EncodingList:** A wrapper around `Vec<ContentEncoding>` used for CBOR serialization. Always use the `H` boundary to separate message-level vs PDU-level encodings.

## 2. PDU Handling
- **Header Merging:** The `Deframer` merges headers from different chunks of the same message. If you add a new field to `Header`, you must update the `merge` method in `src/lib.rs`.
- **Quality Accumulation:** Physical layer decoders (RS, Viterbi) must return a "quality" score. These scores are accumulated by the `Deframer` to decide which PDU chunk is "better" in case of duplicates.

## 3. Error Handling
- Use `anyhow::Result` for fallible operations in `codec.rs` and `generator.rs`.
- In `deframer.rs`, favor log/skip logic over direct `bail!`. A single malformed PDU should not crash the entire deframing session for others.

## 4. CBOR Serialization
- We use `minicbor` for bit-compact headers. 
- Structs used in headers (like `Header` and `EncodingList`) must implement `minicbor::Encode` and `minicbor::Decode`.
- Example of manual CBOR list handling for encodings can be found in `src/lib.rs:L140-170`.

## 5. Performance
- **Zero-Copy Intent:** While `Vec<u8>` is used for simplicity, avoid unnecessary clones in the `codec` loop. 
- **Chunking:** `PDUGenerator` should naturally align chunks to the `n` parameter of Reed-Solomon or the MTU of RaptorQ if those encodings are active.

> [!NOTE]
> **Coding Style:** Follow standard Rust idiomatic patterns. Run `cargo fmt` and `cargo clippy` to enforce baseline quality without cluttering this guide.
