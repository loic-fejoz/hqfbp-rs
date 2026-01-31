# Encoding Stack Principles

This document defines the core principles for managing encoding lists in `hqfbp-rs`. All agents and developers MUST strictly adhere to these rules when implementing or debugging codec logic.

## 1. The Left-to-Right Rule

Encodings are always specified as a list (e.g., `vec![E1, E2, E3]`). This list represents the chronological **Order of Application** to the data stream by the Generator.

### Generator (TX) Logic
The Generator applies encodings from **Left-to-Right** (Start of List to End of List).

**Example:** Stack `[Compression, Header, CRC]`
1.  **Input:** Raw Payload
2.  **Apply `Compression`:** Result = `Compressed(Payload)`
3.  **Apply `Header`:** Result = `Header + Compressed(Payload)`
4.  **Apply `CRC`:** Result = `CRC(Header + Compressed(Payload))`
5.  **Output:** Wire Data

### Deframer (RX) Logic
The Deframer must peel layers in the **Reverse Order** (Right-to-Left) to recover the payload.

1.  **Input:** Wire Data
2.  **Strip `CRC`:** Result = `Header + Compressed(Payload)`
3.  **Process `Header`:** Identify/Validate, strip Header -> Result = `Compressed(Payload)`
4.  **Reverse `Compression`:** Result = Raw Payload

> [!CRITICAL]
> **Implementation Note:** When iterating through encoding lists in `Deframer` logic (e.g., `apply_decodings_multi` or `apply_pdu_level_decodings`), you MUST iterate in **`rev()`** order to correctly peel the outer layers first.

## 2. Headers as Boundaries

Certain encodings are designated as **Headers** (e.g., `H`, `Ax25`). These play a special role as structural boundaries in the stack.

- **Inner Layers (Pre-Header):** Encodings *before* the header (e.g., Compression, Encryption) apply to the content *inside* the packet.
- **The Header:** Wraps the processed inner content. It marks the boundary of the "Packet" or "PDU".
- **Outer Layers (Post-Header):** Encodings *after* the header (e.g., CRC, Reed-Solomon, Scrambler) apply to the *entire packet* (Header + Content).

**Example Stack:** `[Gzip, H, ReedSolomon]`
- `Gzip` compresses the user data.
- `H` wraps the compressed data.
- `ReedSolomon` protects the `H`-packet (including the header fields).

## 3. Chunk Independence

Encodings are applied statelessly or statefully depending on the codec, but the **Encoding List topology** applies uniformly to **any number of chunks**.

- Whether the message is 1 chunk or 1000 chunks, the stack `[E1, E2]` means every single chunk `C_i` is transformed as `E2(E1(C_i))`.
- The `Deframer` must be able to recognize and reverse this stack for every individual PDU it receives.
