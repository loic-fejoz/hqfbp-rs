use hqfbp_rs::codec::{rs_encode, rs_decode};
use rand::{Rng, thread_rng};

fn test_rs_power(n: usize, k: usize, ber: f64, iterations: usize) {
    let mut successes = 0;
    let mut total_corrected = 0;
    let mut rng = thread_rng();

    for _ in 0..iterations {
        let mut data = vec![0u8; k];
        rng.fill(&mut data[..]);
        
        let encoded = rs_encode(&data, n, k).unwrap();
        
        // Inject noise
        let mut noisy = encoded.clone();
        let mut errors = 0;
        for byte in noisy.iter_mut() {
            for bit in 0..8 {
                if rng.gen_bool(ber) {
                    *byte ^= 1 << bit;
                    errors += 1;
                }
            }
        }
        
        if let Ok((decoded, corrected)) = rs_decode(&noisy, n, k) {
            if decoded == data {
                successes += 1;
                total_corrected += corrected;
            }
        }
    }

    println!("Rust RS({},{}) at BER {}:", n, k, ber);
    println!("  Success Rate: {:.2}%", (successes as f64 / iterations as f64) * 100.0);
    if successes > 0 {
        println!("  Avg Corrected: {:.2}", total_corrected as f64 / successes as f64);
    }
}

#[test]
fn main() {
    test_rs_power(120, 100, 0.001, 1000);
    test_rs_power(120, 100, 0.005, 1000);
    test_rs_power(120, 100, 0.01, 1000);
}
