#!/bin/bash
set -e

INPUT_FILE="test_payload_py_rs.bin"
KISS_FILE="output_py_rs.kiss"
OUTPUT_DIR="unpacked_output_py_rs"
FILE_SIZE=10240
ENCODINGS=${1:-"gzip,h,crc32"}
ANN_ENCODINGS=${2:-""}

echo "Cleaning up..."
rm -f "$INPUT_FILE" "$KISS_FILE"
rm -rf "$OUTPUT_DIR"

echo "Generating random payload..."
dd if=/dev/urandom of="$INPUT_FILE" bs=1 count="$FILE_SIZE" status=none

echo "Packing with Python pack (encodings: $ENCODINGS, ann_encodings: $ANN_ENCODINGS)..."
cd ../py-hqfbp
CMD="uv run python src/hqfbp/pack.py ../hqfbp-rs/$INPUT_FILE --src-callsign TEST-PY-RS --encodings $ENCODINGS --output ../hqfbp-rs/$KISS_FILE"
if [ ! -z "$ANN_ENCODINGS" ]; then
    CMD="$CMD --announcement-encodings $ANN_ENCODINGS"
fi
$CMD
cd ../hqfbp-rs

echo "Unpacking with Rust unpack..."
cargo run --bin unpack -- "$OUTPUT_DIR" --input "$KISS_FILE"

UNPACKED_FILE=$(ls -t "$OUTPUT_DIR"/* | head -1)
ORIG_HASH=$(sha256sum "$INPUT_FILE" | awk '{print $1}')
NEW_HASH=$(sha256sum "$UNPACKED_FILE" | awk '{print $1}')

echo "Original: $ORIG_HASH"
echo "Unpacked: $NEW_HASH"

if [ "$ORIG_HASH" == "$NEW_HASH" ]; then
    echo "✅ SUCCESS: Python-to-Rust verification passed!"
    exit 0
else
    echo "❌ FAILURE: Checksums do not match!"
    exit 1
fi
