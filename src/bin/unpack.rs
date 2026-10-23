use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::kiss::KissDecoder;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::Path;

#[derive(Parser, Debug)]
#[command(author, version, about = "Unpack KISS frames containing HQFBP PDUs.")]
struct Args {
    #[arg(help = "Output folder to save received files")]
    output: String,

    #[arg(long, help = "Input KISS file (default: stdin)", default_value = "-")]
    input: String,

    #[arg(long, help = "KISS-over-TCP server address (e.g., localhost:8001)")]
    tcp: Option<String>,

    #[arg(long, short, help = "Enable verbose logging (DEBUG level)")]
    verbose: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize logger
    let level = if args.verbose {
        log::LevelFilter::Debug
    } else {
        log::LevelFilter::Info
    };
    env_logger::Builder::new()
        .filter(None, level)
        .format_timestamp(None)
        .init();

    fs::create_dir_all(&args.output).context("Failed to create output directory")?;

    log::info!(
        "Saving files to: {}",
        fs::canonicalize(&args.output)?.display()
    );

    let mut input: Box<dyn Read> = if let Some(addr) = args.tcp {
        log::info!("Connecting to KISS-over-TCP server at {addr}...");
        Box::new(TcpStream::connect(addr).context("Failed to connect to TCP server")?)
    } else if args.input == "-" {
        log::info!("Reading from: stdin");
        Box::new(io::stdin())
    } else {
        log::info!("Reading from file: {}", args.input);
        Box::new(File::open(&args.input).context("Failed to open input file")?)
    };

    let mut deframer = Deframer::new();
    let mut kiss_decoder = KissDecoder::new();
    let mut buffer = [0u8; 4096];

    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }

        for &byte in &buffer[..n] {
            if let Some(frame) = kiss_decoder.process_byte(byte)
                && frame.len() > 1
                && frame[0] == 0x00
            {
                let pdu = &frame[1..];
                deframer.receive_bytes(pdu);
                print!(".");
                io::stdout().flush()?;

                while let Some(ev) = deframer.next_event() {
                    if let Event::Message(me) = ev {
                        println!();

                        let raw_callsign = me.header.src_callsign.as_deref().unwrap_or("UNKNOWN");
                        let sanitized: String = raw_callsign
                            .chars()
                            .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
                            .take(32)
                            .collect();
                        let callsign = if sanitized.is_empty() {
                            "UNKNOWN"
                        } else {
                            &sanitized
                        };

                        let mime_str = me
                            .header
                            .media_type()
                            .map(|mt| mt.to_mime())
                            .or_else(|| me.header.content_type.clone());
                        let ext = mime_str
                            .as_deref()
                            .and_then(|ct| {
                                mime_guess::get_mime_extensions_str(ct)
                                    .and_then(|exts| exts.first())
                            })
                            .map(|e| format!(".{e}"))
                            .unwrap_or_else(|| ".bin".to_string());

                        let timestamp = Utc::now().format("%Y-%m-%d-%H%M%S-UTC");
                        let filename = format!("{timestamp}-{callsign}{ext}");
                        let filepath = Path::new(&args.output).join(&filename);

                        // Ensure filename didn't traverse outside output directory
                        if filepath.parent() != Some(Path::new(&args.output)) {
                            log::warn!("Rejected unsafe output file path: {filepath:?}");
                            continue;
                        }

                        let mut file = File::create(&filepath)?;
                        file.write_all(&me.payload)?;
                        println!("✅ Received {} ({} bytes)", filename, me.payload.len());
                    }
                }
            }
        }
    }

    Ok(())
}
