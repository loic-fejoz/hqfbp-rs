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

criterion_group!(benches, bench_unpack, bench_deframer_100k);
criterion_main!(benches);
