#!/bin/bash
set -e

INPUT_FILE="test_payload_rs.bin"
KISS_FILE="output_rs.kiss"
OUTPUT_DIR="unpacked_output_rs"
FILE_SIZE=10240
ENCODINGS=${1:-"gzip,h,crc32"}
ANN_ENCODINGS=${2:-""}

echo "Cleaning up..."
rm -f "$INPUT_FILE" "$KISS_FILE"
rm -rf "$OUTPUT_DIR"

echo "Generating random payload..."
dd if=/dev/urandom of="$INPUT_FILE" bs=1 count="$FILE_SIZE" status=none

echo "Packing with Rust pack (encodings: $ENCODINGS, ann_encodings: $ANN_ENCODINGS)..."
CMD="cargo run --bin pack -- $INPUT_FILE 0.0.0.0 0 --src-callsign TEST-RS-PACK --encodings $ENCODINGS --output $KISS_FILE"
if [ ! -z "$ANN_ENCODINGS" ]; then
    CMD="$CMD --ann-encodings $ANN_ENCODINGS"
fi
$CMD

echo "Unpacking with Rust unpack..."
cargo run --bin unpack -- "$OUTPUT_DIR" "$KISS_FILE"

if [ -z "$(ls -A "$OUTPUT_DIR")" ]; then
   echo "Error: Output directory is empty!"
   exit 1
fi

UNPACKED_FILE=$(ls -t "$OUTPUT_DIR"/* | head -1)
ORIG_HASH=$(sha256sum "$INPUT_FILE" | awk '{print $1}')
NEW_HASH=$(sha256sum "$UNPACKED_FILE" | awk '{print $1}')

echo "Original: $ORIG_HASH"
echo "Unpacked: $NEW_HASH"

if [ "$ORIG_HASH" == "$NEW_HASH" ]; then
    echo "✅ SUCCESS: Rust Roundtrip verification passed!"
    exit 0
else
    echo "❌ FAILURE: Checksums do not match!"
    exit 1
fi
