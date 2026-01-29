use hqfbp_rs::codec::rq::rq_encode;

fn main() {
    // rq(dlen, 80, 64) with file-size 1024
    let data = vec![0u8; 1024];
    let original_count = 1024;
    let mtu = 80;
    let repairs = 64;

    println!(
        "Testing rq_encode with mtu={}, original_count={}, repairs={}",
        mtu, original_count, repairs
    );
    match rq_encode(&data, original_count, mtu, repairs) {
        Ok(res) => println!("Success: {} packets", res.len()),
        Err(e) => println!("Error: {}", e),
    }
}

#[test]
fn test_rq_panic_repro() {
    // rq(dlen, 80, 64) with file-size 1024
    let data = vec![0u8; 1024];
    let original_count = 1024;
    let mtu = 80;
    let repairs = 64;

    println!(
        "Testing rq_encode with mtu={}, original_count={}, repairs={}",
        mtu, original_count, repairs
    );
    // This is expected to panic in raptorq crate if the user's report is correct
    let _ = rq_encode(&data, original_count, mtu, repairs);
}
