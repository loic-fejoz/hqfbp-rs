# Makefile for KISS-over-TCP verification

SRC_FILE = Cargo.toml
CALLSIGN = TEST_CALL
TCP_ADDR = 127.0.0.1:8001
TCP_ADDR_V6 = [::1]:8001
OUT_DIR_RS = test_out_rs
OUT_DIR_PY = test_out_py
PY_PACK = src/hqfbp/pack.py
PY_UNPACK = src/hqfbp/unpack.py
PYTHON_UV = cd ../py-hqfbp && uv run python3

.PHONY: all clean test-tcp-rust test-tcp-py test-cross-tcp test-py-bench

all: test-tcp-rust test-tcp-py test-cross-tcp test-py-bench

clean:
	rm -rf $(OUT_DIR_RS) $(OUT_DIR_PY) test_tcp.kiss

$(OUT_DIR_RS):
	mkdir -p $(OUT_DIR_RS)

$(OUT_DIR_PY):
	mkdir -p $(OUT_DIR_PY)

test-tcp-rust: $(OUT_DIR_RS)
	@echo "Testing Rust Pack -> TCP -> Unpack"
	# Start listener to capture KISS frames
	(nc -l -p 8001 > test_tcp.kiss) & \
	SLEEP_PID=$$!; \
	sleep 2; \
	cargo run --release --bin pack -- $(SRC_FILE) --src-callsign $(CALLSIGN) --tcp $(TCP_ADDR); \
	sleep 1; \
	kill $$SLEEP_PID || true
	# Unpack captured frames
	cargo run --release --bin unpack -- $(OUT_DIR_RS) --input test_tcp.kiss
	@ls -l $(OUT_DIR_RS)

test-tcp-py: $(OUT_DIR_PY)
	@echo "Testing Python Pack -> TCP -> Unpack"
	(nc -l -p 8001 > test_tcp.kiss) & \
	SLEEP_PID=$$!; \
	sleep 2; \
	$(PYTHON_UV) $(PY_PACK) ../hqfbp-rs/$(SRC_FILE) --src-callsign $(CALLSIGN) --tcp $(TCP_ADDR); \
	sleep 1; \
	kill $$SLEEP_PID || true
	$(PYTHON_UV) $(PY_UNPACK) ../hqfbp-rs/$(OUT_DIR_PY) --input ../hqfbp-rs/test_tcp.kiss
	@ls -l $(OUT_DIR_PY)

test-cross-tcp: $(OUT_DIR_RS) $(OUT_DIR_PY)
	@echo "Testing Cross-Language TCP (Rust Pack -> Python Unpack)"
	# We can't easily pipe TCP with just nc, but we can use nc as a bridge if needed.
	# Simpler: use a KISS file created via TCP.
	(nc -l -p 8001 > test_tcp.kiss) & \
	SLEEP_PID=$$!; \
	sleep 2; \
	cargo run --release --bin pack -- $(SRC_FILE) --src-callsign $(CALLSIGN) --tcp $(TCP_ADDR); \
	sleep 1; \
	kill $$SLEEP_PID || true
	$(PYTHON_UV) $(PY_UNPACK) ../hqfbp-rs/$(OUT_DIR_PY) --input ../hqfbp-rs/test_tcp.kiss
	@ls -l $(OUT_DIR_PY)

test-tcp-v6: $(OUT_DIR_RS) $(OUT_DIR_PY)
	@echo "Testing Rust Pack (IPv6) -> TCP -> Unpack"
	(nc -l -6 -p 8001 > test_tcp.kiss) & \
	SLEEP_PID=$$!; \
	sleep 2; \
	cargo run --release --bin pack -- $(SRC_FILE) --src-callsign $(CALLSIGN) --tcp $(TCP_ADDR_V6); \
	sleep 1; \
	kill $$SLEEP_PID || true
	cargo run --release --bin unpack -- $(OUT_DIR_RS) --input test_tcp.kiss
	@echo "Testing Python Pack (IPv6) -> TCP -> Unpack"
	(nc -l -6 -p 8001 > test_tcp.kiss) & \
	SLEEP_PID=$$!; \
	sleep 2; \
	$(PYTHON_UV) $(PY_PACK) ../hqfbp-rs/$(SRC_FILE) --src-callsign $(CALLSIGN) --tcp $(TCP_ADDR_V6); \
	sleep 1; \
	kill $$SLEEP_PID || true
	$(PYTHON_UV) $(PY_UNPACK) ../hqfbp-rs/$(OUT_DIR_PY) --input ../hqfbp-rs/test_tcp.kiss

broker:
	ncat -l 8001 --broker

pack-img:
	cargo run --release --bin pack -- ../hqfbp/img_0.png \
		--src-callsign F4JXQ \
		--tcp 127.0.0.1:8001 \
		--encodings "gzip,h,chunk(223),rs(255,223),repeat(2)" \
		--ann-encodings "h,crc32,repeat(2)"

test-py-bench:
	@echo "Running cross-implementation benchmark against Python samples"
	python3 scripts/test_against_py_samples.py ../py-hqfbp/samples

simulate-lt-pre:
	cargo run --release --bin simulate -- --file-size 2048 \
		--limit 5000 \
	    --ber 0.0001 \
	    --encodings "lt(dlen,512,10),crc32,h"

simulate-lt-post:
	cargo run --release --bin simulate -- --file-size 2048 \
		--limit 5000 \
	    --ber 0.0001 \
	    --encodings "h,lt(dlen,255,10)" \
	    --ann-encodings "h,crc32,repeat(3)"

simulate-lt-pre-2:
	cargo run --release --bin simulate -- --file-size 2048 \
		--limit 5000 \
	    --ber 0.0001 \
	    --encodings "lt(dlen,156,30),h,rs(255,223)" \
	    --ann-encodings "h,crc32,repeat(3)"