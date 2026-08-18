use anyhow::{Context, Result};
use clap::Parser;
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::kiss::encode_kiss_frame;
use hqfbp_rs::{MediaType, parse_encodings};
use std::fs::File;
use std::io::{Read, Write};
use std::net::TcpStream;

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about = "Pack a file into KISS frames using the HQFBP protocol."
)]
struct Args {
    #[arg(help = "Path to the file to send")]
    filepath: String,

    #[arg(long, help = "Source callsign")]
    src_callsign: String,

    #[arg(long, help = "Comma-separated list of encodings")]
    encodings: Option<String>,

    #[arg(long, help = "Comma-separated list of announcement encodings")]
    ann_encodings: Option<String>,

    #[arg(long, help = "Maximum payload size for chunking")]
    max_payload_size: Option<usize>,

    #[arg(long, help = "Starting message ID")]
    msg_id: Option<u32>,

    #[arg(long, help = "Path to TOML configuration file (ignored)")]
    config: Option<String>,

    #[arg(long, help = "Output KISS file path")]
    output: Option<String>,

    #[arg(long, short, help = "Enable verbose logging (DEBUG level)")]
    verbose: bool,

    #[arg(long, help = "KISS-over-TCP server address (e.g., localhost:8001)")]
    tcp: Option<String>,
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

    let encodings = args.encodings.as_deref().map(parse_encodings);
    let ann_encodings = args.ann_encodings.as_deref().map(parse_encodings);

    let mut f = File::open(&args.filepath).context("Failed to open input file")?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;

    // Guess content type (mimic standard fallback)
    let content_type = mime_guess::from_path(&args.filepath)
        .first_or_octet_stream()
        .to_string();

    let mut generator = PDUGenerator::new(
        Some(args.src_callsign),
        None,
        args.max_payload_size,
        encodings,
        ann_encodings,
        args.msg_id.unwrap_or(1),
    );

    let pdus = generator.generate(&data, Some(MediaType::Type(content_type)))?;

    let mut out: Box<dyn Write> = if let Some(addr) = args.tcp {
        log::info!("Connecting to KISS-over-TCP server at {addr}...");
        Box::new(TcpStream::connect(addr).context("Failed to connect to TCP server")?)
    } else {
        let output_path = args
            .output
            .unwrap_or_else(|| format!("{}.kiss", args.filepath));
        log::info!("Writing to KISS file {output_path}...");
        Box::new(File::create(&output_path).context("Failed to create output file")?)
    };

    let mut count = 0;
    for pdu in pdus {
        let frame = encode_kiss_frame(&pdu);
        out.write_all(&frame)?;
        count += 1;
    }

    log::info!("Successfully sent/packed {count} frames.");
    Ok(())
}
