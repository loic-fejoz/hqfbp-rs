# hqfbp-rs

Rust implementation to pack and unpack Protocol Data Units (PDUs) for the **Hamradio Quick File Broadcasting Protocol (HQFBP)**.

## About HQFBP

The Hamradio Quick File Broadcasting Protocol (HQFBP) is designed for efficient, robust, and asynchronous file and data broadcasting over radio communication links. It is particularly suited for challenging environments like satellite downlinks.

Key features include:
- **Low Overhead**: Uses CBOR indexing to minimize header size.
- **Error Tolerance**: Supports asynchronous delivery and reassembly.
- **File Broadcasting**: Efficient for one-to-many transmissions.
- **Chunking**: Mandatory support for large file split into smaller PDUs.

For more details, refer to the [HQFBP RFC](https://github.com/loic-fejoz/hqfbp/blob/main/RFC.md) ([local version](../hqfbp/RFC.md)).

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
hqfbp-rs = { git = "https://github.com/loic-fejoz/hqfbp-rs" }
```

## Usage

```rust
use hqfbp_rs::generator::{PDUGenerator};
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::ContentEncoding;

fn main() -> anyhow::Result<()> {
    // 1. Initialize Generator with complex encodings
    // - gzip: compress the message content (pre-boundary)
    // - h: boundary marker
    // - rs(255,233): apply Reed-Solomon FEC to the whole PDU (post-boundary)
    // - announcement_encodings: used for the preliminary metadata PDU
    let mut gen = PDUGenerator::new(
        Some("F4JXQ".to_string()),
        None, // dst_callsign
        None, // max_payload_size
        Some(vec![
            ContentEncoding::Gzip,
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 233),
        ]),
        Some(vec![
            ContentEncoding::H,
            ContentEncoding::Crc32,
        ]),
        1, // initial_msg_id
    );

    let data = b"Hello, this is a robust message!";
    let pdus = gen.generate(data, None)?;

    // 2. Initialize Deframer
    let mut deframer = Deframer::new();

    // 3. Process PDUs
    // The first PDU in this case is the Announcement PDU that helps 
    // the Deframer understand upcoming encoded data.
    for pdu in pdus {
        deframer.receive_bytes(&pdu);
    }

    // 4. Handle resulting events
    while let Some(ev) = deframer.next_event() {
        if let Event::Message(me) = ev {
            let src = me.header.src_callsign.unwrap_or_default();
            println!("Received from {}: {}", src, String::from_utf8_lossy(&me.payload));
        }
    }

    Ok(())
}
```

## Encoding Exploration

To find the optimal balance between transmission efficiency and reliability, you can use the built-in exploration tools.

### CLI Explorer
The `explore` binary generates random encodings and simulates their performance under noise.

```bash
cargo run --release --bin explore -- --nb-encodings 100 --limit 1000 --ber 0.001
```

### Visual Explorer (Parallel)
The `visual_explore.py` script runs multiple exploration threads in parallel and provides a live-updating interactive dashboard.

1. Install dependencies: `./.venv/bin/pip install plotly dash pandas`
2. Run the explorer:
   ```bash
   ./.venv/bin/python3 visual_explore.py --n-thread 4 --nb-encodings 100
   ```
3. Open `http://127.0.0.1:8050` in your browser to view the Pareto frontier in real-time.

## Features

- **Bit-Accuracy**: Fully compatible with the Python reference implementation.
- **Robust FEC**: Support for RaptorQ and Reed-Solomon.
- **Compression**: Support for Gzip, Brotli, and LZMA.
- **Integrity**: Built-in CRC16 and CRC32 support.
- **Fast**: High-performance Rust reassembly engine.
