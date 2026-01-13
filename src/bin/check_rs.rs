use hqfbp_rs::codec::rs_encode;

fn main() {
    let data = vec![0, 1, 2, 3, 4];
    let encoded = rs_encode(&data, 10, 5).unwrap();
    println!("{}", hex::encode(encoded));
}
