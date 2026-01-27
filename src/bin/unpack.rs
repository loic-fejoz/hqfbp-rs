use chrono::Utc;
use clap::Parser;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::error::{HqfbpContext, Result};
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

const FEND: u8 = 0xC0;
const FESC: u8 = 0xDB;
const TFEND: u8 = 0xDC;
const TFESC: u8 = 0xDD;

struct KISSDeFramer {
    in_frame: bool,
    escaped: bool,
    buffer: Vec<u8>,
}

impl KISSDeFramer {
    fn new() -> Self {
        Self {
            in_frame: false,
            escaped: false,
            buffer: Vec::new(),
        }
    }

    fn process_byte(&mut self, byte: u8) -> Option<Vec<u8>> {
        if self.in_frame {
            if byte == FEND {
                self.in_frame = false;
                if !self.buffer.is_empty() {
                    let frame = std::mem::take(&mut self.buffer);
                    return Some(frame);
                } else {
                    return None;
                }
            } else if byte == FESC {
                self.escaped = true;
            } else if self.escaped {
                if byte == TFEND {
                    self.buffer.push(FEND);
                } else if byte == TFESC {
                    self.buffer.push(FESC);
                } else {
                    self.buffer.push(byte);
                }
                self.escaped = false;
            } else {
                self.buffer.push(byte);
            }
        } else if byte == FEND {
            self.in_frame = true;
            self.buffer.clear();
            self.escaped = false;
        }
        None
    }
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

    println!(
        "Saving files to: {}",
        fs::canonicalize(&args.output)?.display()
    );

    let mut input: Box<dyn Read> = if let Some(addr) = args.tcp {
        println!("Connecting to KISS-over-TCP server at {addr}...");
        Box::new(TcpStream::connect(addr).context("Failed to connect to TCP server")?)
    } else if args.input == "-" {
        println!("Reading from: stdin");
        Box::new(io::stdin())
    } else {
        println!("Reading from file: {}", args.input);
        Box::new(File::open(&args.input).context("Failed to open input file")?)
    };

    let mut deframer = Deframer::new();
    let mut kiss_decoder = KISSDeFramer::new();
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

                        let callsign = me.header.src_callsign.as_deref().unwrap_or("UNKNOWN");
                        let ext = me
                            .header
                            .content_type
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
