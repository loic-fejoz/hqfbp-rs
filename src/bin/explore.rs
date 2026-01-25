use anyhow::Result;
use clap::{Parser, ValueEnum};
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::random_sensible_encoding::generate_sensible_encoding;
use rand::{Rng, RngCore};
use std::io::Write;

#[derive(Parser, Debug)]
#[command(author, version, about = "HQFBP Encoding Explorer")]
struct Args {
    #[arg(long, default_value_t = 0.001, help = "Bit Error Rate")]
    ber: f64,

    #[arg(
        long,
        default_value_t = 1024,
        help = "File size in bytes (negative for [10, abs(N)])",
        allow_hyphen_values = true
    )]
    file_size: i64,

    #[arg(long, default_value_t = 1000, help = "Number of files per encoding")]
    limit: usize,

    #[arg(
        long,
        default_value_t = 10,
        help = "Number of random encodings to test"
    )]
    nb_encodings: usize,

    #[arg(long, value_enum, default_value_t = Format::Csv, help = "Output format")]
    format: Format,

    #[arg(long, short, help = "Enable verbose logging (DEBUG level)")]
    verbose: bool,
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
    files_attempted: usize,
    files_recovered: usize,
    total_payload_bits: usize,
    header_bits: usize,
    total_bit_errors_introduced: usize,
    total_bits_on_air: usize,
    pdus_lost: usize,
    total_pdus_sent: usize,
}

impl SimulationMetrics {
    fn new() -> Self {
        Self {
            total_bits_sent: 0,
            files_attempted: 0,
            files_recovered: 0,
            total_payload_bits: 0,
            header_bits: 0,
            total_bit_errors_introduced: 0,
            total_bits_on_air: 0,
            pdus_lost: 0,
            total_pdus_sent: 0,
        }
    }

    fn get_results(&self, encodings: String) -> std::collections::BTreeMap<String, String> {
        let efficiency = if self.total_bits_sent > 0 {
            (self.total_payload_bits as f64 / self.total_bits_sent as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };
        let file_loss_rate = if self.files_attempted > 0 {
            ((self.files_attempted.saturating_sub(self.files_recovered)) as f64
                / self.files_attempted as f64
                * 100.0)
                .clamp(0.0, 100.0)
        } else {
            100.0
        };
        let air_ber = if self.total_bits_on_air > 0 {
            self.total_bit_errors_introduced as f64 / self.total_bits_on_air as f64
        } else {
            0.0
        };
        let p_loss = if self.total_pdus_sent > 0 {
            (self.pdus_lost as f64 / self.total_pdus_sent as f64 * 100.0).clamp(0.0, 100.0)
        } else {
            0.0
        };

        let avg_file_size = if self.files_recovered > 0 {
            self.total_payload_bits / self.files_recovered / 8
        } else {
            0
        };

        let mut data = std::collections::BTreeMap::new();
        data.insert("Encodings".to_string(), encodings);
        data.insert("File Size".to_string(), avg_file_size.to_string());
        data.insert("Eff (%)".to_string(), format!("{:.2}", efficiency));
        data.insert(
            "File Loss (%)".to_string(),
            format!("{:.2}", file_loss_rate),
        );
        data.insert("PDU Loss (%)".to_string(), format!("{:.2}", p_loss));
        data.insert("Air-BER".to_string(), format!("{:.2e}", air_ber));
        data
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let level = if args.verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    env_logger::Builder::new()
        .filter(None, level)
        .format_timestamp(None)
        .init();

    let channel = BitErrorChannel::new(args.ber);
    let mut rng = rand::thread_rng();

    let mut first_out = true;

    for _ in 0..args.nb_encodings {
        let seed = rng.next_u64();
        let enc_list = generate_sensible_encoding(seed);
        let enc_str = enc_list
            .0
            .iter()
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
            .join(",");

        use rayon::prelude::*;

        let results: SimulationMetrics = (0..args.limit)
            .into_par_iter()
            .map(|_| {
                let mut local_rng = rand::thread_rng();
                let size = if args.file_size < 0 {
                    local_rng.gen_range(10..=(args.file_size.abs() as usize))
                } else {
                    args.file_size as usize
                };
                let mut source_data = vec![0u8; size];
                local_rng.fill_bytes(&mut source_data);

                let mut m = SimulationMetrics::new();
                m.files_attempted = 1;

                let mut generator = PDUGenerator::new(
                    Some("EXPLR".to_string()),
                    None,
                    None,
                    Some(enc_list.0.clone()),
                    None,
                    1,
                );

                let pdus = match generator.generate(&source_data, None) {
                    Ok(pdus) => pdus,
                    Err(e) => {
                        log::warn!("Generator failed for {}: {}", enc_str, e);
                        return m;
                    }
                };

                let mut clean_pdus_info = Vec::new();
                let mut clean_deframer = Deframer::new();
                clean_deframer.register_announcement(
                    Some("EXPLR".to_string()),
                    1,
                    enc_list.0.clone(),
                );

                for pdu in &pdus {
                    clean_deframer.receive_bytes(pdu);
                    while let Some(ev) = clean_deframer.next_event() {
                        if let Event::PDU(pe) = ev {
                            clean_pdus_info.push((pdu.clone(), pe.payload));
                        }
                    }
                }

                for (pdu, payload) in &clean_pdus_info {
                    let h_size = pdu.len().saturating_sub(payload.len());
                    m.header_bits += h_size * 8;
                }

                if clean_pdus_info.is_empty() {
                    return m;
                }

                let mut noisy_deframer = Deframer::new();
                noisy_deframer.register_announcement(
                    Some("EXPLR".to_string()),
                    1,
                    enc_list.0.clone(),
                );
                let mut recovered = false;

                for (clean_pdu, _) in clean_pdus_info.iter() {
                    m.total_pdus_sent += 1;
                    let bits = clean_pdu.len() * 8;
                    m.total_bits_sent += bits;
                    m.total_bits_on_air += bits;

                    let (noisy_pdu, errors_in_pdu) = channel.process(clean_pdu);
                    m.total_bit_errors_introduced += errors_in_pdu;

                    noisy_deframer.receive_bytes(&noisy_pdu);

                    let mut pdu_accepted = false;
                    while let Some(ev) = noisy_deframer.next_event() {
                        match ev {
                            Event::PDU(_) => {
                                pdu_accepted = true;
                            }
                            Event::Message(me) => {
                                if me.payload.len() == source_data.len()
                                    && me.payload == source_data
                                {
                                    recovered = true;
                                }
                            }
                        }
                    }
                    if !pdu_accepted {
                        m.pdus_lost += 1;
                    }
                }

                if recovered {
                    m.files_recovered = 1;
                    m.total_payload_bits = source_data.len() * 8;
                }
                m
            })
            .reduce(SimulationMetrics::new, |mut a, b| {
                a.total_bits_sent += b.total_bits_sent;
                a.files_attempted += b.files_attempted;
                a.files_recovered += b.files_recovered;
                a.total_payload_bits += b.total_payload_bits;
                a.header_bits += b.header_bits;
                a.total_bit_errors_introduced += b.total_bit_errors_introduced;
                a.total_bits_on_air += b.total_bits_on_air;
                a.pdus_lost += b.pdus_lost;
                a.total_pdus_sent += b.total_pdus_sent;
                a
            });

        let res = results.get_results(enc_str);

        match args.format {
            Format::Csv => {
                let mut stdout = std::io::stdout();
                let mut wtr = csv::WriterBuilder::new()
                    .has_headers(first_out)
                    .from_writer(stdout.by_ref());

                if first_out {
                    wtr.write_record(res.keys())?;
                    first_out = false;
                }
                wtr.write_record(res.values())?;
                wtr.flush()?;
            }
            Format::Json => {
                println!("{}", serde_json::to_string(&res)?);
                std::io::stdout().flush()?;
            }
            Format::Markdown => {
                if first_out {
                    println!(
                        "| {} |",
                        res.keys().cloned().collect::<Vec<_>>().join(" | ")
                    );
                    println!(
                        "| {} |",
                        res.keys().map(|_| "---").collect::<Vec<_>>().join(" | ")
                    );
                    first_out = false;
                }
                println!(
                    "| {} |",
                    res.values().cloned().collect::<Vec<_>>().join(" | ")
                );
                std::io::stdout().flush()?;
            }
        }
    }

    Ok(())
}
