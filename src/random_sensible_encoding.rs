use crate::{ContentEncoding, EncodingList};
use rand::{Rng, SeedableRng, rngs::StdRng};

/// Generates a random, sensible EncodingList based on a seed.
///
/// It does not cover compression.
/// The grammar followed is:
/// [Object segmentation] -> [Object integrity] -> [Header] -> [Flow Protection] ->
/// [Packet Integrity] -> [Scramble/Repeat] -> [External FEC] -> [Internal FEC]
pub fn generate_sensible_encoding(seed: u64) -> EncodingList {
    let mut rng = StdRng::seed_from_u64(seed);
    let mut encodings = Vec::new();

    let mut current_mtu: usize = 240;

    // 8. Internal FEC:
    let rnd_8 = rng.gen_range(0..=1);
    if rnd_8 == 0 {
        encodings.push(ContentEncoding::Conv(7, "1/2".to_string()));
        current_mtu /= 2;
    }

    // 7. External FEC
    let rnd_7 = if rnd_8 == 0 {
        rng.gen_range(0..=1) // No double convolution
    } else {
        rng.gen_range(0..=2) // ID, RS, or Conv
    };

    match rnd_7 {
        1 => {
            // RS
            let k = rng.gen_range((current_mtu / 2)..current_mtu);
            encodings.push(ContentEncoding::ReedSolomon(current_mtu, k));
            current_mtu = k;
        }
        2 => {
            // Conv
            encodings.push(ContentEncoding::Conv(7, "1/2".to_string()));
            current_mtu /= 2;
        }
        _ => {} // ID
    }

    // 6. Whitening Scramble / Extra Repeat
    match rng.gen_range(0..=3) {
        1 => {
            // `scr(g3ruh)` is $1+x^{12}+x^{17}$.
            encodings.push(ContentEncoding::Scrambler(0x1C7, None));
        }
        2 => {
            // `scr(ccsds)` is $1+x^3+x^5+x^7+x^8$ thus `scr(0x1A9, 0xFF)`.
            encodings.push(ContentEncoding::Scrambler(0x1A9, Some(0xFF)));
        }
        3 => {
            let count = rng.gen_range(2..5);
            encodings.push(ContentEncoding::Repeat(count));
        }
        _ => {} // ContentEncoding::ID
    }

    // Choices for step 4/5 and 1/2
    // let mut rnd_4 = rng.gen_range(0..=3); // Flow Protection: 0: None, 1: None, 2: RQ, 3: LT
    let rnd_1 = rng.gen_range(0..=3); // Segmentation: 0: None, 1: Chunk, 2: RQ, 3: LT

    // Reroll if some Fountain Code are picked twice (Reduce probability of duplicates)
    // if rnd_4 >= 2 && rnd_1 >= 2 {
    //     if rng.gen_bool(0.8) {
    //         rnd_4 = rng.gen_range(0..=3);
    //     }
    // }

    // 5. Packet Integrity: Id or CRC
    // let rnd_5 = if rnd_4 >= 2 {
    //     rng.gen_range(1..=2) // CRC mandatory if FEC
    // } else {
    //     rng.gen_range(0..=2)
    // };
    let rnd_5 = rng.gen_range(0..=2);
    match rnd_5 {
        1 => {
            encodings.push(ContentEncoding::Crc16);
            current_mtu = current_mtu.saturating_sub(2);
        }
        2 => {
            encodings.push(ContentEncoding::Crc32);
            current_mtu = current_mtu.saturating_sub(4);
        }
        _ => {} // ContentEncoding::ID
    }

    // 4. flow protection
    // match rnd_4 {
    //     // 2 => {
    //     //     // RaptorQ
    //     //     if rng.gen_bool(0.5) {
    //     //         let k = rng.gen_range(50..=100) as u8;
    //     //         encodings.push(ContentEncoding::RaptorQDynamicPercent(
    //     //             current_mtu as u16,
    //     //             k,
    //     //         ));
    //     //     } else {
    //     //         let k = rng.gen_range(1..=20) as u32;
    //     //         encodings.push(ContentEncoding::RaptorQDynamic(current_mtu as u16, k));
    //     //     }
    //     // }
    //     // 3 => {
    //     //     // LT
    //     //     let k = rng.gen_range(1..=20) as u32;
    //     //     encodings.push(ContentEncoding::LTDynamic(current_mtu as u16, k));
    //     // }
    //     _ => {}
    // }

    // 3. header
    encodings.push(ContentEncoding::H);
    current_mtu = current_mtu.saturating_sub(40); // TODO better estimation of size of header

    // 2. Object integrity : id or crc
    let rnd_2 = if rnd_1 >= 2 {
        rng.gen_range(1..=2) // CRC mandatory if FEC at step 1
    } else {
        rng.gen_range(0..=2)
    };
    match rnd_2 {
        1 => {
            encodings.push(ContentEncoding::Crc16);
            current_mtu = current_mtu.saturating_sub(2);
        }
        2 => {
            encodings.push(ContentEncoding::Crc32);
            current_mtu = current_mtu.saturating_sub(4);
        }
        _ => {}
    }

    // 1. Object segmentation
    match rnd_1 {
        1 => {
            encodings.push(ContentEncoding::Chunk(current_mtu));
        }
        2 => {
            // RaptorQ
            if rng.gen_bool(0.5) {
                let k = rng.gen_range(50..=100) as u8;
                encodings.push(ContentEncoding::RaptorQDynamicPercent(
                    current_mtu as u16,
                    k,
                ));
            } else {
                let k = rng.gen_range(1..=20) as u32;
                encodings.push(ContentEncoding::RaptorQDynamic(current_mtu as u16, k));
            }
        }
        3 => {
            // LT
            let k = rng.gen_range(1..=20) as u32;
            encodings.push(ContentEncoding::LTDynamic(current_mtu as u16, k));
        }
        _ => {}
    }

    encodings.reverse();
    EncodingList(encodings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Header, pack};

    #[test]
    fn test_random_generation_consistency() {
        let e1 = generate_sensible_encoding(42);
        let e2 = generate_sensible_encoding(42);
        let e3 = generate_sensible_encoding(43);

        assert_eq!(e1, e2);
        assert_ne!(e1, e3);
    }

    #[test]
    fn test_random_encodings_validity() {
        for seed in 0..50 {
            let encs = generate_sensible_encoding(seed);
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
