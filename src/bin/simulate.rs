use anyhow::{Result, anyhow};
use clap::{Parser, ValueEnum};
use hqfbp_rs::ContentEncoding;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use rand::{Rng, RngCore};

#[derive(Parser, Debug)]
#[command(author, version, about = "HQFBP Simulation Engine")]
struct Args {
    #[arg(long, default_value_t = 0.0, help = "Bit Error Rate")]
    ber: f64,

    #[arg(
        long,
        default_value = "h",
        help = "Content encodings (e.g. gzip,h,crc32)"
    )]
    encodings: String,

    #[arg(long, help = "Announcement encodings")]
    ann_encodings: Option<String>,

    #[arg(long, default_value_t = 1024, help = "File size in bytes")]
    file_size: usize,

    #[arg(long, default_value_t = 10, help = "Number of files to transmit")]
    limit: usize,

    #[arg(long, value_enum, default_value_t = Format::Markdown, help = "Output format")]
    format: Format,

    #[arg(long, help = "Enable debug prints")]
    debug: bool,
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum, Debug)]
enum Format {
    Markdown,
    Json,
    Csv,
}

struct BitErrorChannel {
    ber: f64,
}

impl BitErrorChannel {
    fn new(ber: f64) -> Self {
        Self { ber }
    }

    fn process(&self, data: &[u8]) -> (Vec<u8>, usize) {
        if self.ber <= 0.0 {
            return (data.to_vec(), 0);
        }

        let mut rng = rand::thread_rng();
        let mut ba = data.to_vec();
        let mut errors = 0;
        for byte in ba.iter_mut() {
            for bit in 0..8 {
                if rng.gen_bool(self.ber) {
                    *byte ^= 1 << bit;
                    errors += 1;
                }
            }
        }
        (ba, errors)
    }
}

struct SimulationMetrics {
    total_bits_sent: usize,
    total_pdus_sent: usize,
    pdus_lost: usize,
    files_attempted: usize,
    files_recovered: usize,
    total_payload_bits: usize,
    header_bits: usize,
    padding_bits: usize,
    current_burst_loss: usize,
    max_burst_loss: usize,
    total_bit_errors_introduced: usize,
    total_residual_bit_errors: usize,
    total_bits_evaluated: usize,
    total_bits_on_air: usize,
}

impl SimulationMetrics {
    fn new() -> Self {
        Self {
            total_bits_sent: 0,
            total_pdus_sent: 0,
            pdus_lost: 0,
            files_attempted: 0,
            files_recovered: 0,
            total_payload_bits: 0,
            header_bits: 0,
            padding_bits: 0,
            current_burst_loss: 0,
            max_burst_loss: 0,
            total_bit_errors_introduced: 0,
            total_residual_bit_errors: 0,
            total_bits_evaluated: 0,
            total_bits_on_air: 0,
        }
    }

    fn add_pdu(&mut self, pdu_bytes: &[u8], lost: bool, errors_in_pdu: usize) {
        self.total_pdus_sent += 1;
        let bits = pdu_bytes.len() * 8;
        self.total_bits_sent += bits;
        self.total_bits_on_air += bits;
        self.total_bit_errors_introduced += errors_in_pdu;

        if lost {
            self.pdus_lost += 1;
            self.current_burst_loss += 1;
            self.max_burst_loss = self.max_burst_loss.max(self.current_burst_loss);
        } else {
            self.current_burst_loss = 0;
        }
    }

    fn add_residual_errors(&mut self, original_payload: &[u8], decoded_payload: &[u8]) {
        let length = original_payload.len().min(decoded_payload.len());
        self.total_bits_evaluated += length * 8;
        for i in 0..length {
            let diff = original_payload[i] ^ decoded_payload[i];
            self.total_residual_bit_errors += diff.count_ones() as usize;
        }
        self.total_residual_bit_errors +=
            (original_payload.len() as isize - decoded_payload.len() as isize).unsigned_abs() * 8;
    }

    fn report(&self, format: Format) -> String {
        let efficiency = (self.total_payload_bits as f64 / self.total_bits_sent as f64 * 100.0)
            .clamp(0.0, 100.0);
        let packet_loss_rate =
            (self.pdus_lost as f64 / self.total_pdus_sent as f64 * 100.0).clamp(0.0, 100.0);
        let file_loss_rate = ((self.files_attempted as f64 - self.files_recovered as f64)
            / self.files_attempted as f64
            * 100.0)
            .clamp(0.0, 100.0);
        let overhead = ((self.header_bits as f64 + self.padding_bits as f64)
            / self.total_bits_sent as f64
            * 100.0)
            .clamp(0.0, 100.0);
        let fec_recovery =
            (self.files_recovered as f64 / self.files_attempted as f64 * 100.0).clamp(0.0, 100.0);
        let rber = self.total_residual_bit_errors as f64 / self.total_bits_evaluated as f64;
        let air_ber = self.total_bit_errors_introduced as f64 / self.total_bits_on_air as f64;

        let mut data = std::collections::BTreeMap::new();
        data.insert(
            "Total Bytes Sent".to_string(),
            (self.total_bits_sent / 8).to_string(),
        );
        data.insert(
            "Packet Loss Rate (%)".to_string(),
            format!("{packet_loss_rate:.2}"),
        );
        data.insert(
            "File Loss Rate (%)".to_string(),
            format!("{file_loss_rate:.2}"),
        );
        data.insert(
            "Bit Error Rate (on air)".to_string(),
            format!("{air_ber:.2e}"),
        );
        data.insert(
            "Bit Errors Introduced".to_string(),
            self.total_bit_errors_introduced.to_string(),
        );
        data.insert("Residual Bit Error Rate".to_string(), format!("{rber:.2e}"));
        data.insert(
            "FEC Recovery Rate (%)".to_string(),
            format!("{fec_recovery:.2}"),
        );
        data.insert(
            "Transmission Efficiency (%)".to_string(),
            format!("{efficiency:.2}"),
        );
        data.insert(
            "Max Burst Loss".to_string(),
            self.max_burst_loss.to_string(),
        );
        data.insert(
            "Protocol Overhead (%)".to_string(),
            format!("{overhead:.2}"),
        );

        match format {
            Format::Json => serde_json::to_string_pretty(&data).unwrap(),
            Format::Csv => {
                let mut wtr = csv::Writer::from_writer(vec![]);
                wtr.write_record(data.keys()).unwrap();
                wtr.write_record(data.values()).unwrap();
                String::from_utf8(wtr.into_inner().unwrap()).unwrap()
            }
            Format::Markdown => {
                let keys: Vec<_> = data.keys().cloned().collect();
                let vals: Vec<_> = data.values().cloned().collect();

                let mut k_width = "Metric".len();
                let mut v_width = "Value".len();
                for k in &keys {
                    k_width = k_width.max(k.len());
                }
                for v in &vals {
                    v_width = v_width.max(v.len());
                }

                let mut output = String::new();
                output.push_str(&format!(
                    "| {:<k_width$} | {:<v_width$} |\n",
                    "Metric",
                    "Value",
                    k_width = k_width,
                    v_width = v_width
                ));
                output.push_str(&format!(
                    "| {:-<k_width$} | {:-<v_width$} |\n",
                    "",
                    "",
                    k_width = k_width,
                    v_width = v_width
                ));
                for (k, v) in data {
                    output.push_str(&format!("| {k:<k_width$} | {v:<v_width$} |\n"));
                }
                output
            }
        }
    }
}

fn parse_encodings(s: &str) -> Vec<ContentEncoding> {
    let mut results = Vec::new();
    let mut current = String::new();
    let mut depth = 0;

    for c in s.chars() {
        if c == ',' && depth == 0 {
            if !current.is_empty() {
                results.push(parse_single_enc(&current));
                current.clear();
            }
        } else {
            if c == '(' {
                depth += 1;
            }
            if c == ')' {
                depth -= 1;
            }
            current.push(c);
        }
    }
    if !current.is_empty() {
        results.push(parse_single_enc(&current));
    }
    results
}

fn parse_single_enc(s: &str) -> ContentEncoding {
    ContentEncoding::try_from(s).unwrap_or(ContentEncoding::OtherString(s.to_string()))
}

fn main() -> Result<()> {
    let args = Args::parse();

    let channel = BitErrorChannel::new(args.ber);
    let mut metrics = SimulationMetrics::new();

    let mut rng = rand::thread_rng();
    let mut source_data = vec![0u8; args.file_size];
    rng.fill_bytes(&mut source_data);

    let encs = parse_encodings(&args.encodings);
    let ann_encs = args.ann_encodings.as_ref().map(|s| parse_encodings(s));

    for _ in 0..args.limit {
        metrics.files_attempted += 1;
        let mut generator = PDUGenerator::new(
            Some("SIMUL".to_string()),
            None,
            None,
            Some(encs.clone()),
            ann_encs.clone(),
            1,
        );

        let pdus = generator
            .generate(&source_data, None)
            .map_err(|e| anyhow!("Generator failed: {e}"))?;
        if args.debug {
            eprintln!("Generated {} PDUs for file", pdus.len());
        }
        let mut clean_pdus_info = Vec::new();
        let mut clean_deframer = Deframer::new();

        for pdu in &pdus {
            clean_deframer.receive_bytes(pdu);
            while let Some(ev) = clean_deframer.next_event() {
                if let Event::PDU(pe) = ev {
                    clean_pdus_info.push((pdu.clone(), pe.payload));
                }
            }
        }

        // Calculate header bits from clean PDUs
        for (pdu, payload) in &clean_pdus_info {
            let h_size = pdu.len() - payload.len();
            metrics.header_bits += h_size * 8;
        }

        if clean_pdus_info.is_empty() {
            continue;
        }

        let mut noisy_deframer = Deframer::new();
        let mut recovered = false;

        for (clean_pdu, expected_payload) in clean_pdus_info.iter() {
            let (noisy_pdu, errors_in_pdu) = channel.process(clean_pdu);

            noisy_deframer.receive_bytes(&noisy_pdu);

            let mut pdu_accepted = false;
            while let Some(ev) = noisy_deframer.next_event() {
                match ev {
                    Event::PDU(pe) => {
                        pdu_accepted = true;
                        metrics.add_residual_errors(expected_payload, &pe.payload);
                    }
                    Event::Message(me) => {
                        if me.payload.starts_with(&source_data) {
                            recovered = true;
                        }
                    }
                }
            }

            metrics.add_pdu(clean_pdu, !pdu_accepted, errors_in_pdu);
        }

        if recovered {
            metrics.files_recovered += 1;
            metrics.total_payload_bits += source_data.len() * 8;
        }
    }

    println!("{}", metrics.report(args.format));

    Ok(())
}
