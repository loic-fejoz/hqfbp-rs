use anyhow::{Result, Context};
use clap::{Parser};
use hqfbp_rs::generator::{PDUGenerator, EncValue};
use std::fs::File;
use std::io::{Read, Write};

#[derive(Parser, Debug)]
#[command(author, version, about = "Pack a file into KISS frames using the HQFBP protocol.")]
struct Args {
    #[arg(help = "Path to the file to send")]
    filepath: String,

    #[arg(help = "Destination IP address (ignored)")]
    ip: String,

    #[arg(help = "Destination UDP port (ignored)")]
    port: u16,

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
}

const FEND: u8 = 0xC0;
const FESC: u8 = 0xDB;
const TFEND: u8 = 0xDC;
const TFESC: u8 = 0xDD;

fn encode_kiss_frame(pdu: &[u8]) -> Vec<u8> {
    let mut frame = Vec::new();
    frame.push(FEND);
    frame.push(0x00); // Command Byte: Data frame, Port 0

    for &byte in pdu {
        if byte == FEND {
            frame.push(FESC);
            frame.push(TFEND);
        } else if byte == FESC {
            frame.push(FESC);
            frame.push(TFESC);
        } else {
            frame.push(byte);
        }
    }

    frame.push(FEND);
    frame
}

fn parse_encodings(s: &str) -> Vec<EncValue> {
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
            if c == '(' { depth += 1; }
            if c == ')' { depth -= 1; }
            current.push(c);
        }
    }
    if !current.is_empty() {
        results.push(parse_single_enc(&current));
    }
    results
}

fn parse_single_enc(s: &str) -> EncValue {
    if let Ok(i) = s.parse::<i8>() {
        EncValue::Integer(i)
    } else {
        EncValue::String(s.to_string())
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    
    let encodings = args.encodings.as_ref().map(|s| parse_encodings(s));
    let ann_encodings = args.ann_encodings.as_ref().map(|s| parse_encodings(s));
    
    let mut f = File::open(&args.filepath).context("Failed to open input file")?;
    let mut data = Vec::new();
    f.read_to_end(&mut data)?;

    // Guess content type (mimic standard fallback)
    let content_type = mime_guess::from_path(&args.filepath).first_or_octet_stream().to_string();

    let mut generator = PDUGenerator::new(
        Some(args.src_callsign),
        None,
        args.max_payload_size,
        encodings,
        ann_encodings,
        args.msg_id.unwrap_or(1),
    );

    let pdus = generator.generate(&data, Some(content_type))?;

    let output_path = args.output.unwrap_or_else(|| format!("{}.kiss", args.filepath));
    let mut out_file = File::create(&output_path).context("Failed to create output file")?;

    let mut count = 0;
    for pdu in pdus {
        let frame = encode_kiss_frame(&pdu);
        out_file.write_all(&frame)?;
        count += 1;
    }

    println!("Successfully packed {} frames of {} into {}", count, args.filepath, output_path);
    Ok(())
}
