use clap::Parser;
use hqfbp_rs::codec::rs_encode;
use hqfbp_rs::error::Result;

#[derive(Parser, Debug)]
#[command(author, version, about = "Check Reed-Solomon encoding")]
struct Args {
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

    let data = vec![0, 1, 2, 3, 4];
    let encoded = rs_encode(&data, 10, 5)?;
    println!("{}", hex::encode(encoded));
    Ok(())
}
