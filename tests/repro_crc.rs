#[cfg(test)]
mod tests {
    use hqfbp_rs::generator::PDUGenerator;
    use hqfbp_rs::deframer::{Deframer, Event};
    use hqfbp_rs::ContentEncoding;

    #[test]
    fn test_crc_repro() {
        let data = vec![0u8; 100]; // Small data
        let encs = vec![
            ContentEncoding::Crc32,
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 223),
        ];
        
        // Generator
        let mut generator = PDUGenerator::new(
            Some("SRC".to_string()),
            None,
            None,
            Some(encs.clone()),
            None,
            1,
        );
        let pdus = generator.generate(&data, None).unwrap();
        assert_eq!(pdus.len(), 1);
        let pdu = &pdus[0];
        println!("Generated PDU len: {}", pdu.len());

        // Deframer
        let mut deframer = Deframer::new();
        deframer.receive_bytes(pdu);
        
        let mut recovered = false;
        while let Some(ev) = deframer.next_event() {
            if let Event::Message(me) = ev {
                assert_eq!(me.payload, data);
                recovered = true;
            }
        }
        assert!(recovered, "Failed to recover");
    }
}
