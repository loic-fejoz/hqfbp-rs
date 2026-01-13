#!/bin/bash
set -e

# Compile release binary first to avoid recompiling in loop
cargo build --release --bin simulate

RESULTS_FILE="simulation_results.md"
echo "# Simulation Results" > $RESULTS_FILE
echo "Generated at $(date)" >> $RESULTS_FILE
echo "" >> $RESULTS_FILE

# Function to run simulation
run_sim() {
    NAME=$1
    ARGS=$2
    echo "Running: $NAME"
    echo "## $NAME" >> $RESULTS_FILE
    echo "\`simulate $ARGS\`" >> $RESULTS_FILE
    echo "" >> $RESULTS_FILE
    ./target/release/simulate $ARGS --format markdown >> $RESULTS_FILE
    echo "" >> $RESULTS_FILE
}

# --- hqfbp-rs Report Scenarios ---

# Small Files (< 150B)
# Config: h,rs(255,191),repeat(3)
run_sim "hqfbp-rs Small Files" "--ber 0.001 --encodings h,rs(255,191),repeat(3) --ann-encodings h,crc32,repeat(10) --file-size 140 --limit 100"

# Large Files (~ 120KB)
# Config: rq(122880,1024,240),h,rs(255,223)
# Note: Lower limit for large files to save time, but enough for stats
run_sim "hqfbp-rs Large Files" "--ber 0.001 --encodings rq(122880,1024,240),h,rs(255,223) --ann-encodings h,crc32,repeat(10) --file-size 122880 --limit 20"

# User Request: Dynamic RQ
# Config: gzip,h,rq(dlen,255,240)
# Announcement: h,rs(255,191),repeat(3)
# Note: Using 120KB to stress test
run_sim "hqfbp-rs User Request (Dynamic RQ)" "--ber 0.001 --encodings gzip,h,rq(dlen,255,240) --ann-encodings h,rs(255,191),repeat(3) --file-size 122880 --limit 20"

# --- py-hqfbp Report Scenarios ---
# Using 10KB as standard size for comparison
FILE_SIZE=10240
LIMIT=50

# 1. Baseline
run_sim "py-hqfbp Baseline" "--ber 0.001 --encodings h --file-size $FILE_SIZE --limit $LIMIT"

# 2. Fragile
run_sim "py-hqfbp Fragile" "--ber 0.001 --encodings rs(255,127),h --ann-encodings h,repeat(10) --file-size $FILE_SIZE --limit $LIMIT"

# 3. Hybrid ARQ
run_sim "py-hqfbp Hybrid ARQ" "--ber 0.001 --encodings rs(255,127),h,repeat(3) --ann-encodings h,repeat(10) --file-size $FILE_SIZE --limit $LIMIT"

# 4. Robust (Chunked)
run_sim "py-hqfbp Robust" "--ber 0.001 --encodings chunk(100),crc32,h,rs(120,100) --ann-encodings h,repeat(10) --file-size 1000 --limit $LIMIT" 
# Reduced file size for chunk(100) to keep simulation reasonable (1000 bytes = 10 chunks)

# 5. Degraded
run_sim "py-hqfbp Degraded" "--ber 0.001 --encodings rs(255,223),h,crc32,repeat(5) --ann-encodings h,repeat(10) --file-size $FILE_SIZE --limit $LIMIT"

# 6. Winner (Gzip)
run_sim "py-hqfbp Winner" "--ber 0.001 --encodings gzip,h,rs(120,100),repeat(2) --ann-encodings h,crc32,repeat(10) --file-size $FILE_SIZE --limit $LIMIT"

echo "Simulations complete. Results saved to $RESULTS_FILE"
