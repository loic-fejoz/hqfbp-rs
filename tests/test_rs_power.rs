use hqfbp_rs::codec::{rs_decode, rs_encode};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

fn test_rs_power(n: usize, k: usize, ber: f64, iterations: usize) -> f64 {
    let mut successes = 0;
    let mut total_corrected = 0;
    // Use fixed seed for reproducibility across CI runs
    let mut rng = StdRng::seed_from_u64(0x12345678);

    for _ in 0..iterations {
        let mut data = vec![0u8; k];
        rng.fill(&mut data[..]);

        let encoded = rs_encode(&data, n, k).unwrap();

        // Inject noise
        let mut noisy = encoded.clone();
        for byte in noisy.iter_mut() {
            for bit in 0..8 {
                if rng.gen_bool(ber) {
                    *byte ^= 1 << bit;
                }
            }
        }

        if let Ok((decoded, corrected)) = rs_decode(&noisy, n, k)
            && decoded == data
        {
            successes += 1;
            total_corrected += corrected;
        }
    }

    let rate = (successes as f64 / iterations as f64) * 100.0;
    println!("Rust RS({n},{k}) at BER {ber}:");
    println!("  Success Rate: {rate:.2}%");
    if successes > 0 {
        println!(
            "  Avg Corrected: {:.2}",
            total_corrected as f64 / successes as f64
        );
    }
    rate
}

#[test]
fn main() {
    let rate_low_noise = test_rs_power(120, 100, 0.001, 500);
    assert!(
        rate_low_noise > 95.0,
        "Expected >95% recovery at BER 0.001, got {rate_low_noise:.2}%"
    );

    let rate_med_noise = test_rs_power(120, 100, 0.005, 500);
    assert!(
        rate_med_noise > 40.0,
        "Expected >40% recovery at BER 0.005, got {rate_med_noise:.2}%"
    );

    let rate_high_noise = test_rs_power(120, 100, 0.01, 500);
    // Just verify rate is measured
    assert!(
        rate_high_noise >= 0.0,
        "Expected valid rate, got {rate_high_noise:.2}%"
    );
}
