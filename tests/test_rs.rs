#[cfg(test)]
mod tests {
    use hqfbp_rs::codec::rs::{rs_decode, rs_encode};

    #[test]
    fn test_rs_correction() {
        let n = 120;
        let k = 100;
        let data = vec![0x42u8; k];

        let encoded = rs_encode(&data, n, k).expect("RS encode failed");
        assert_eq!(encoded.len(), n, "Encoded block length mismatch");

        let mut noisy = encoded.clone();
        // (120, 100) can correct (120-100)/2 = 10 errors.
        for item in noisy.iter_mut().take(10) {
            *item ^= 0xFF; // Injected errors
        }

        let (decoded, _corrected) = rs_decode(&noisy, n, k).expect("Decode failed");
        assert_eq!(decoded.len(), k, "Decoded data length mismatch");
        assert_eq!(decoded, data, "Data recovery failed after 10 errors");
        println!("SUCCESS: RS (120, 100) corrected 10 errors!");
    }

    #[test]
    fn test_rs_failure() {
        let n = 120;
        let k = 100;
        let data = vec![0u8; k];
        let encoded = rs_encode(&data, n, k).expect("RS encode failed");

        let mut noisy = encoded.clone();
        // 11 errors should be too many for (120, 100)
        for item in noisy.iter_mut().take(11) {
            *item ^= 0xFF;
        }

        let result = rs_decode(&noisy, n, k);
        assert!(result.is_err(), "RS should have failed with 11 errors");
        println!("SUCCESS: RS correctly failed with too many errors!");
    }
}
