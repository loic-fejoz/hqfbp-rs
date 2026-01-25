use criterion::{Criterion, black_box, criterion_group, criterion_main};
use hqfbp_rs::deframer::Deframer;
use hqfbp_rs::generator::PDUGenerator;
use hqfbp_rs::unpack;

fn bench_unpack(c: &mut Criterion) {
    let mut generator = PDUGenerator::new(Some("BENCH".to_string()), None, None, None, None, 1);
    let data = vec![0u8; 1024];
    let pdus = generator.generate(&data, None).unwrap();
    let pdu = pdus[0].clone();

    c.bench_function("unpack_1k", |b| {
        b.iter(|| {
            let _ = unpack(black_box(pdu.clone()));
        })
    });
}

fn bench_deframer_100k(c: &mut Criterion) {
    let mut generator = PDUGenerator::new(Some("BENCH".to_string()), None, None, None, None, 1);
    let data = vec![0u8; 100 * 1024]; // 100KB
    let pdus = generator.generate(&data, None).unwrap();

    c.bench_function("deframer_100k", |b| {
        b.iter(|| {
            let mut deframer = Deframer::new();
            for pdu in &pdus {
                deframer.receive_bytes(black_box(pdu));
                while deframer.next_event().is_some() {}
            }
        })
    });
}

use hqfbp_rs::ContentEncoding;

fn bench_deframer_stack(c: &mut Criterion, name: &str, encodings: Vec<ContentEncoding>) {
    let mut generator = PDUGenerator::new(
        Some("BENCH".to_string()),
        None,
        None,
        Some(encodings.clone()),
        None,
        1,
    );
    let data = vec![0u8; 1024]; // 1KB
    let pdus = generator.generate(&data, None).expect("Generator failed");

    // Pre-verify that decoding actually works once
    {
        let mut deframer = Deframer::new();
        deframer.register_announcement(Some("BENCH".to_string()), 1, encodings.clone());
        for pdu in &pdus {
            deframer.receive_bytes(pdu);
        }
        let mut count = 0;
        while deframer.next_event().is_some() {
            count += 1;
        }
        assert!(
            count > 0,
            "Benchmark {} setup failed: No events decoded during pre-check",
            name
        );
    }

    c.bench_function(name, |b| {
        b.iter(|| {
            let mut deframer = Deframer::new();
            deframer.register_announcement(Some("BENCH".to_string()), 1, encodings.clone());

            for pdu in &pdus {
                deframer.receive_bytes(black_box(pdu));
            }
            let mut decoded_any = false;
            while deframer.next_event().is_some() {
                decoded_any = true;
            }
            // For benchmarking we generally avoid assertions inside the loop,
            // but the user requested "ensure decoding".
            // `black_box` prevents optimization of `decoded_any`.
            // Asserting loop success is technically safer but costlier.
            // Let's settle for the pre-check above + avoiding optimization.
            black_box(decoded_any);
        })
    });
}

fn bench_opaque_deframer(c: &mut Criterion) {
    let encodings = vec![
        ContentEncoding::H,
        ContentEncoding::Crc32,
        ContentEncoding::Scrambler(0x1a9, Some(0xff)),
        ContentEncoding::ReedSolomon(120, 92),
        ContentEncoding::Conv(7, "1/2".to_string()),
    ];
    bench_deframer_stack(c, "deframer_opaque_scr_rs_conv_1k", encodings);
}

fn bench_complex_deframer(c: &mut Criterion) {
    // "rq(dlen, 72, 10%),crc32,h,rs(120,100),conv(7,1/2)"
    let encodings = vec![
        ContentEncoding::RaptorQDynamicPercent(72, 10),
        ContentEncoding::Crc32,
        ContentEncoding::H,
        ContentEncoding::ReedSolomon(120, 100),
        ContentEncoding::Conv(7, "1/2".to_string()),
    ];
    bench_deframer_stack(c, "deframer_complex_stack_1k", encodings);
}

fn bench_golay_complex_deframer(c: &mut Criterion) {
    // rq(dlen, 256, 64), crc32, h, scr(G3RUH), golay(24,12)
    let encodings = vec![
        ContentEncoding::RaptorQDynamic(256, 64),
        ContentEncoding::Crc32,
        ContentEncoding::H,
        ContentEncoding::Scrambler(0x21001, None),
        ContentEncoding::Golay(24, 12),
    ];
    bench_deframer_stack(c, "deframer_golay_complex_stack_1k", encodings);
}

criterion_group!(
    benches,
    bench_unpack,
    bench_deframer_100k,
    bench_opaque_deframer,
    bench_complex_deframer,
    bench_golay_complex_deframer
);
criterion_main!(benches);
