#[cfg(test)]
mod tests {

    use hqfbp_rs::ContentEncoding;
    use hqfbp_rs::deframer::{Deframer, Event};
    use hqfbp_rs::generator::PDUGenerator;

    #[test]
    fn test_rq_rs_user_params() {
        let mut data = vec![0u8; 120000];
        for (i, item) in data.iter_mut().enumerate() {
            *item = (i % 256) as u8;
        }
        let encs = vec![
            ContentEncoding::RaptorQ(122880, 1024, 240),
            ContentEncoding::Crc32,
            ContentEncoding::H,
            ContentEncoding::ReedSolomon(255, 223),
        ];
        let mut generator = PDUGenerator::new(
            Some("SRC".to_string()),
            None,
            None,
            Some(encs),
            Some(vec![
                ContentEncoding::H,
                ContentEncoding::Crc32,
                ContentEncoding::Repeat(50),
            ]),
            1,
        );
        let pdus = generator.generate(&data, None).unwrap();

        let mut deframer = Deframer::new();
        let mut recovered_data = None;
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let ber = 0.0001;
        let mut count_received = 0;
        let mut count_accepted = 0;
        let mut mismatch_count = 0;
        let mut first_mismatch = None;

        for (idx, pdu) in pdus.iter().enumerate() {
            count_received += 1;
            let mut noisy_pdu = pdu.to_vec();
            for byte in noisy_pdu.iter_mut() {
                for bit in 0..8 {
                    if rng.gen_bool(ber) {
                        *byte ^= 1 << bit;
                    }
                }
            }
            deframer.receive_bytes(&noisy_pdu);
            while let Some(ev) = deframer.next_event() {
                match ev {
                    Event::PDU(pe) => {
                        count_accepted += 1;
                        let clean_unpack = hqfbp_rs::unpack(pdu.clone()).unwrap();
                        if !clean_unpack.1.starts_with(&pe.payload) {
                            mismatch_count += 1;
                            if first_mismatch.is_none() {
                                first_mismatch =
                                    Some((idx, clean_unpack.1.to_vec(), pe.payload.to_vec()));
                            }
                        }
                    }
                    Event::Message(me) => {
                        if me.payload.len() == data.len() {
                            recovered_data = Some(me.payload.to_vec());
                        }
                    }
                }
            }
        }

        let msg = format!(
            "Received: {}, Accepted: {}, Mismatched: {}, Total: {}, Recovered: {} Len: {} Match: {}\n",
            count_received,
            count_accepted,
            mismatch_count,
            pdus.len(),
            recovered_data.is_some(),
            recovered_data.as_ref().map(|r| r.len()).unwrap_or(0),
            if let Some(ref rd) = recovered_data {
                rd == &data
            } else {
                false
            }
        );

        if let Some(ref rd) = recovered_data {
            if rd != &data {
                let mut diffs = Vec::new();
                for i in 0..rd.len() {
                    if rd[i] != data[i] {
                        diffs.push((i, data[i], rd[i]));
                        if diffs.len() > 10 {
                            break;
                        }
                    }
                }
                let mut mismatch_dump = String::new();
                if let Some((idx, clean, faulty)) = first_mismatch {
                    mismatch_dump = format!(
                        "FIRST MISMATCH AT PDU {}:\n  CLEAN: {:02x?}\n  FAULTY: {:02x?}\n",
                        idx,
                        &clean[..32.min(clean.len())],
                        &faulty[..32.min(faulty.len())]
                    );
                }
                let diff_msg = format!("DIFFS: {diffs:?}\n{mismatch_dump}");
                std::fs::write("/tmp/test_results.txt", format!("{msg}{diff_msg}")).unwrap();
            } else {
                std::fs::write("/tmp/test_results.txt", msg).unwrap();
            }
        } else {
            std::fs::write("/tmp/test_results.txt", msg).unwrap();
        }

        if recovered_data.is_none() {
            panic!("FAILED TO RECOVER. See /tmp/test_results.txt");
        }
        assert_eq!(recovered_data.unwrap(), data);
    }
}
