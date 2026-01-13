use reed_solomon::{Decoder, Encoder};

#[test]
fn test_rs_basic_and_shortened() {
    let ecc_len = 32;
    let encoder = Encoder::new(ecc_len);
    let decoder = Decoder::new(ecc_len);

    let data = vec![1, 2, 3, 4, 5];
    println!("Data len: {}", data.len());

    let encoded = encoder.encode(&data);
    println!("Encoded len: {}", encoded.len());
    // println!("Encoded bytes: {:02x?}", &encoded[..]);

    let corrupted = encoded.to_vec();
    // No corruption
    match decoder.correct(&corrupted, None) {
        Ok(corrected) => {
            println!("Corrected len: {}", corrected.len());
            // println!("Corrected bytes: {:02x?}", &corrected[..]);
            // corrected contains data + parity
            assert_eq!(&corrected[..data.len()], data.as_slice());
        }
        Err(e) => panic!("Error decoding uncorrupted data: {e:?}"),
    }

    // Try shortened block with padding
    // PDUGenerator / PDUDeframer logic simulation for shortened blocks
    let k = 223;
    let short_data = vec![0xaa; 100];
    let internal_pad = k - 100;

    // Simulate what rs_encode does: pad to k
    let mut block = vec![0u8; internal_pad];
    block.extend_from_slice(&short_data);

    let enc_block = encoder.encode(&block);
    println!("Enc block len: {}", enc_block.len());
    // parity is the last 32 bytes
    let parity = &enc_block[enc_block.len() - 32..];

    // Simulate reception: data + parity (without internal padding)
    let mut received = short_data.clone();
    received.extend_from_slice(parity);
    println!("Received len: {}", received.len());

    let mut full_codeword = vec![0u8; 255 - received.len()];
    full_codeword.extend_from_slice(&received);

    match decoder.correct(&full_codeword, None) {
        Ok(corrected) => {
            println!("Success! Corrected data len: {}", corrected.len());
            // corrected is [PAD (123) | DATA (100) | PARITY (32)]
            let actual_data = &corrected[internal_pad..internal_pad + short_data.len()];
            assert_eq!(actual_data, short_data.as_slice());
        }
        Err(e) => panic!("Failed to decode shortened block: {e:?}"),
    }
}
