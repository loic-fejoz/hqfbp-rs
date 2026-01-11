#!/bin/bash
set -e

ENCODINGS="rs(120,100),h,repeat(2)"
ANN_ENCODINGS="h,crc32,repeat(10)"
BER=0.001
FILE_SIZE=1024
INPUT_FILE="test_payload_noise.bin"
KISS_FILE="output_noise.kiss"
NOISY_KISS_FILE="output_noisy.kiss"

rm -f "$INPUT_FILE" "$KISS_FILE" "$NOISY_KISS_FILE"

echo "Generating random payload..."
dd if=/dev/urandom of="$INPUT_FILE" bs=1 count="$FILE_SIZE" status=none

echo "---- PHASE 1: Rust Pack -> Noise -> Python Unpack ----"
cargo run --bin pack -- "$INPUT_FILE" \
    --src-callsign "R-PK-N" \
    --encodings "$ENCODINGS" \
    --ann-encodings "$ANN_ENCODINGS" \
    --output "$KISS_FILE"

python3 inject_noise.py "$KISS_FILE" "$BER" > "$NOISY_KISS_FILE"

echo "Unpacking with Python..."
rm -rf unpacked_py
mkdir -p unpacked_py
cd ../py-hqfbp
uv run python src/hqfbp/unpack.py "../hqfbp-rs/unpacked_py" "../hqfbp-rs/$NOISY_KISS_FILE" > /dev/null
cd ../hqfbp-rs

SUCCESS_PY=0
ORIG_HASH=$(sha256sum "$INPUT_FILE" | awk '{print $1}')
for f in unpacked_py/*; do
    if [ -f "$f" ]; then
        NEW_HASH=$(sha256sum "$f" | awk '{print $1}')
        if [ "$ORIG_HASH" == "$NEW_HASH" ]; then
            SUCCESS_PY=1
            break
        fi
    fi
done

if [ $SUCCESS_PY -eq 1 ]; then
    echo "✅ Success: Python recovered Rust-packed noisy data"
else
    echo "❌ Failure: Python failed to recover Rust-packed noisy data"
fi

echo "---- PHASE 2: Python Pack -> Noise -> Rust Unpack ----"
cd ../py-hqfbp
rm -f "../hqfbp-rs/$KISS_FILE"
uv run python src/hqfbp/pack.py "../hqfbp-rs/$INPUT_FILE" \
    --src-callsign "P-PK-N" \
    --encodings "$ENCODINGS" \
    --announcement-encodings "$ANN_ENCODINGS" \
    --output "../hqfbp-rs/$KISS_FILE" > /dev/null
cd ../hqfbp-rs

python3 inject_noise.py "$KISS_FILE" "$BER" > "$NOISY_KISS_FILE"

echo "Unpacking with Rust..."
rm -rf unpacked_rs
mkdir -p unpacked_rs
cargo run --bin unpack -- unpacked_rs "$NOISY_KISS_FILE" > /dev/null

SUCCESS_RS=0
for f in unpacked_rs/*; do
    if [ -f "$f" ]; then
        NEW_HASH=$(sha256sum "$f" | awk '{print $1}')
        if [ "$ORIG_HASH" == "$NEW_HASH" ]; then
            SUCCESS_RS=1
            break
        fi
    fi
done

if [ $SUCCESS_RS -eq 1 ]; then
    echo "✅ Success: Rust recovered Python-packed noisy data"
else
    echo "❌ Failure: Rust failed to recover Python-packed noisy data"
fi
