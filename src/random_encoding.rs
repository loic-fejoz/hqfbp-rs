use crate::{ContentEncoding, EncodingList};
use rand::{Rng, SeedableRng, rngs::StdRng};

/// Generates a random, EncodingList based on a seed.
///
/// The grammar followed is:
/// [Compression] -> [Integrity] -> [Expansion] -> [h] -> [Integrity] -> [FEC] -> [Scramble/Repeat]
pub fn generate_encoding(seed: u64) -> EncodingList {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut encodings = Vec::new();

    // --- PRE-H STAGE ---

    // 1. Compression (Optional, pick at most one)
    if rng.gen_bool(0.3) {
        match rng.gen_range(0..3) {
            0 => encodings.push(ContentEncoding::Gzip),
            1 => encodings.push(ContentEncoding::Brotli),
            _ => encodings.push(ContentEncoding::Lzma),
        }
    }

    // 2. Pre-H Integrity (Optional, pick one)
    if rng.gen_bool(0.2) {
        if rng.gen_bool(0.5) {
            encodings.push(ContentEncoding::Crc16);
        } else {
            encodings.push(ContentEncoding::Crc32);
        }
    }

    // 3. Expansion/Fragmentation (Optional, pick at most one)
    if rng.gen_bool(0.4) {
        match rng.gen_range(0..4) {
            0 => {
                // RaptorQ Dynamic
                let mtu = rng.gen_range(64..=1200) as u16;
                let repair = rng.gen_range(10..100);
                encodings.push(ContentEncoding::RaptorQDynamic(mtu, repair));
            }
            1 => {
                // LT Dynamic
                let mtu = rng.gen_range(64..=1200) as u16;
                let repair = rng.gen_range(10..100);
                encodings.push(ContentEncoding::LTDynamic(mtu, repair));
            }
            2 => {
                // Chunk
                let size = rng.gen_range(64..2048);
                encodings.push(ContentEncoding::Chunk(size));
            }
            _ => {
                // Repeat
                let count = rng.gen_range(2..6);
                encodings.push(ContentEncoding::Repeat(count));
            }
        }
    }

    // --- PIVOT ---
    encodings.push(ContentEncoding::H);

    // --- POST-H STAGE ---

    // 4. Post-H Integrity (Optional, pick one)
    if rng.gen_bool(0.3) {
        if rng.gen_bool(0.5) {
            encodings.push(ContentEncoding::Crc16);
        } else {
            encodings.push(ContentEncoding::Crc32);
        }
    }

    // 5. FEC (Optional, pick at most one)
    if rng.gen_bool(0.5) {
        if rng.gen_bool(0.8) {
            // Reed Solomon
            let n: usize = match rng.gen_range(0..3) {
                0 => 224,
                1 => 128,
                _ => rng.gen_range(64..=224),
            };
            let max_k = n.saturating_sub(1);
            let min_k = n.saturating_sub(32).max(1);
            let k = if min_k < max_k {
                rng.gen_range(min_k..=max_k)
            } else {
                max_k
            };
            encodings.push(ContentEncoding::ReedSolomon(n, k));
        } else {
            // Convolutional (fixed params as supported by codec.rs)
            encodings.push(ContentEncoding::Conv(7, "1/2".to_string()));
        }
    }

    // 6. Scramble / Extra Repeat (Optional)
    if rng.gen_bool(0.2) {
        if rng.gen_bool(0.5) {
            let poly = rng.r#gen::<u64>() | 1; // Ensure some bits
            encodings.push(ContentEncoding::Scrambler(poly, None));
        } else {
            let count = rng.gen_range(2..4);
            encodings.push(ContentEncoding::Repeat(count));
        }
    }

    EncodingList(encodings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, pack};

    #[test]
    fn test_random_generation_consistency() {
        let e1 = generate_encoding(42);
        let e2 = generate_encoding(42);
        let e3 = generate_encoding(43);

        assert_eq!(e1, e2);
        assert_ne!(e1, e3);
    }

    #[test]
    fn test_random_encodings_validity() {
        for seed in 0..50 {
            let encs = generate_encoding(seed);
            let h_count = encs
                .0
                .iter()
                .filter(|e| matches!(e, ContentEncoding::H))
                .count();
            assert_eq!(
                h_count, 1,
                "Should have exactly one 'h' in encoding for seed {}",
                seed
            );

            // Basic parameter checks
            for enc in &encs.0 {
                match enc {
                    ContentEncoding::ReedSolomon(n, k) => {
                        assert!(*n <= 255);
                        assert!(*k < *n);
                        assert!(*k > 0);
                    }
                    ContentEncoding::LTDynamic(mtu, _) | ContentEncoding::LT(_, mtu, _) => {
                        assert!(*mtu > 0);
                    }
                    ContentEncoding::RaptorQDynamic(mtu, _)
                    | ContentEncoding::RaptorQ(_, mtu, _) => {
                        assert!(*mtu > 0);
                    }
                    _ => {}
                }
            }

            // Integration test with pack
            let mut header = Header::default();
            header.message_id = Some(123);
            header.content_encoding = Some(encs.clone());
            let payload = b"hello world random encoding test";

            // Should not panic
            let res = pack(&header, payload);
            assert!(
                res.is_ok(),
                "Packing failed for encodings: {:?}, Err: {:?}",
                encs,
                res.err()
            );
        }
    }
}
