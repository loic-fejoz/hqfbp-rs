#!/bin/bash
set -e

INPUT_FILE="test_payload_rs_py.bin"
KISS_FILE="output_rs_py.kiss"
OUTPUT_DIR="unpacked_output_rs_py"
FILE_SIZE=10240

echo "Cleaning up..."
rm -f "$INPUT_FILE" "$KISS_FILE"
rm -rf "$OUTPUT_DIR"

echo "Generating random payload..."
dd if=/dev/urandom of="$INPUT_FILE" bs=1 count="$FILE_SIZE" status=none

echo "Packing with Rust pack..."
cargo run --bin pack -- "$INPUT_FILE" 0.0.0.0 0 \
    --src-callsign "TEST-RS-PY" \
    --encodings "gzip,h,crc32" \
    --output "$KISS_FILE"

echo "Unpacking with Python unpack..."
cd ../py-hqfbp
uv run python src/hqfbp/unpack.py "../hqfbp-rs/$OUTPUT_DIR" "../hqfbp-rs/$KISS_FILE"
cd ../hqfbp-rs

UNPACKED_FILE=$(ls -t "$OUTPUT_DIR"/* | head -1)
ORIG_HASH=$(sha256sum "$INPUT_FILE" | awk '{print $1}')
NEW_HASH=$(sha256sum "$UNPACKED_FILE" | awk '{print $1}')

echo "Original: $ORIG_HASH"
echo "Unpacked: $NEW_HASH"

if [ "$ORIG_HASH" == "$NEW_HASH" ]; then
    echo "✅ SUCCESS: Rust-to-Python verification passed!"
    exit 0
else
    echo "❌ FAILURE: Checksums do not match!"
    exit 1
fi
