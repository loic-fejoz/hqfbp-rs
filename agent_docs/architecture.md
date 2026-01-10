# Architecture: HQFBP Data Flow

The project implements a layered transformation pipeline designed for radio link reliability.

## 1. Transmission Pipeline (The Generator)
The `PDUGenerator` ([src/generator.rs](file:///home/loic/projets/hqfbp-rs/src/generator.rs)) transforms a source payload into a stream of PDUs.

- **Content Encoding (Pre-Boundary):** High-level transformations (Compression like Gzip/Brotli/LZMA) applied to the message content.
- **The Boundary Marker (`H`):** A sentinel in the encoding list. Transformations *after* this marker are applied per-PDU.
- **PDU Creation:** The message is split into chunks (see `MAX_PAYLOAD_SIZE`).
- **Post-Boundary Encodings:** Lower-level transformations (CRC, Reed-Solomon, Scrambler) applied to raw PDUs.
- **Announcement Generation:** A specialized PDU (`application/vnd.hqfbp+cbor`) is often sent first to describe upcoming encodings.

## 2. Reception Pipeline (The Deframer)
The `Deframer` ([src/deframer.rs](file:///home/loic/projets/hqfbp-rs/src/deframer.rs)) reassembles the transmission.

- **Direct Unpacking:** Attempts to read PDU headers directly using `unpack` ([src/lib.rs](file:///home/loic/projets/hqfbp-rs/src/lib.rs)).
- **Recursive Unpacking:** Handles nested PDUs (PDUs within PDUs) resulting from multi-stage forward error correction (FEC).
- **Session Management:** Groups PDUs by `(src_callsign, message_id)`.
- **Multi-PDU FEC:** Collects enough symbols for RaptorQ or Reed-Solomon before reassembling the full data.
- **Content Decoding:** Reverses pre-boundary encodings after full message reassembly.

## 3. Core Logic Patterns
- **Merged Headers:** When reassembling chunks, headers from multiple PDUs are merged to recover missing metadata.
- **Quality Metrics:** The `Deframer` tracks "quality" (e.g., bit corrections in RS/Viterbi) to prioritize better chunks.
- **Late Truncation:** The `file_size` field is used to truncate the final payload *only after* all decodings are complete.

> [!TIP]
> Use `src/bin/simulate.rs` to visualize this flow by injecting Bit Error Rates (BER) and observing recovery success.
