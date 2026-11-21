use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;
use hqfbp_rs::deframer::{Deframer, Event};
use hqfbp_rs::kiss::KissDecoder;
use std::fs::{self, File, OpenOptions};
use std::io::{self, ErrorKind, Read, Write};
use std::net::TcpStream;
use std::path::Path;

/// Atomically creates a unique file in `output_dir`, appending `-1`, `-2`, ...
/// when multiple messages share the same timestamped basename.
fn create_unique_output_file(
    output_dir: &Path,
    base_name: &str,
    ext: &str,
) -> Result<Option<(String, File)>> {
    for seq in 0u32..10_000 {
        let filename = if seq == 0 {
            format!("{base_name}{ext}")
        } else {
            format!("{base_name}-{seq}{ext}")
        };
        let filepath = output_dir.join(&filename);

        // Ensure filename didn't traverse outside output directory
        if filepath.parent() != Some(output_dir) {
            log::warn!("Rejected unsafe output file path: {filepath:?}");
            return Ok(None);
        }

        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&filepath)
        {
            Ok(file) => return Ok(Some((filename, file))),
            Err(e) if e.kind() == ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    anyhow::bail!("Could not allocate unique filename for {base_name}{ext} in {output_dir:?}")
}

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
                        let base_name = format!("{timestamp}-{callsign}");
                        let Some((filename, mut file)) =
                            create_unique_output_file(Path::new(&args.output), &base_name, &ext)?
                        else {
                            continue;
                        };

                        file.write_all(&me.payload)?;
                        println!("✅ Received {} ({} bytes)", filename, me.payload.len());
                    }
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_unique_output_file_deduplicates_same_second_burst() {
        let dir =
            std::env::temp_dir().join(format!("hqfbp_unpack_unique_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();

        let mut names = Vec::new();
        for i in 0..5 {
            let (name, mut f) =
                create_unique_output_file(&dir, "2026-11-21-150000-UTC-BURST", ".bin")
                    .unwrap()
                    .expect("should create unique file");
            writeln!(f, "payload-{i}").unwrap();
            names.push(name);
        }

        assert_eq!(
            names,
            vec![
                "2026-11-21-150000-UTC-BURST.bin",
                "2026-11-21-150000-UTC-BURST-1.bin",
                "2026-11-21-150000-UTC-BURST-2.bin",
                "2026-11-21-150000-UTC-BURST-3.bin",
                "2026-11-21-150000-UTC-BURST-4.bin",
            ]
        );
        for (i, name) in names.iter().enumerate() {
            let content = fs::read_to_string(dir.join(name)).unwrap();
            assert_eq!(content, format!("payload-{i}\n"));
        }

        // Unsafe path traversal must be rejected
        assert!(
            create_unique_output_file(&dir, "../evil", ".bin")
                .unwrap()
                .is_none()
        );

        let _ = fs::remove_dir_all(&dir);
    }
}
