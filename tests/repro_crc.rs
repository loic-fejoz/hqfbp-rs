#[cfg(test)]
mod tests {
    use hqfbp_rs::ContentEncoding;
    use hqfbp_rs::deframer::{Deframer, Event};
    use hqfbp_rs::generator::PDUGenerator;

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
            Some(vec![ContentEncoding::H]),
            1,
        );
        let pdus = generator.generate(&data, None).unwrap();
        assert_eq!(pdus.len(), 2);

        // Deframer
        let mut deframer = Deframer::new();
        for pdu in pdus {
            deframer.receive_bytes(&pdu);
        }

        let mut recovered = false;
        while let Some(ev) = deframer.next_event() {
            if let Event::Message(me) = ev {
                if me.payload == data {
                    recovered = true;
                }
            }
        }
        assert!(recovered, "Failed to recover");
    }
}
