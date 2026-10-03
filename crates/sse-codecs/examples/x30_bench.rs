use std::{env, fs, process, time::Instant};

fn main() {
    let args: Vec<String> = env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: x30_bench <save.sav> [decoded.bin]");
        process::exit(2);
    };
    let data = fs::read(path).unwrap_or_else(|error| {
        eprintln!("read: {error}");
        process::exit(2);
    });
    if data.len() < 8 {
        eprintln!("save is too short");
        process::exit(2);
    }
    let size_bytes = data.get(..4).unwrap_or_default();
    let size = u32::from_le_bytes(<[u8; 4]>::try_from(size_bytes).unwrap_or_default()) as usize;
    let stream_end = data.len().saturating_sub(4);
    let stream = data.get(4..stream_end).unwrap_or_default();
    let mut output = vec![0_u8; size];

    sse_codecs::kraken::decompress_into(stream, &mut output).unwrap_or_else(|error| {
        eprintln!("warmup decode: {error:?}");
        process::exit(1);
    });

    let iterations = 5_u32;
    let started = Instant::now();
    for _ in 0..iterations {
        sse_codecs::kraken::decompress_into(stream, &mut output).unwrap_or_else(|error| {
            eprintln!("decode: {error:?}");
            process::exit(1);
        });
    }
    let elapsed = started.elapsed().as_secs_f64();
    let mib = (size as f64) * f64::from(iterations) / (1024.0 * 1024.0);
    println!("decoded_bytes={size}");
    println!("iterations={iterations}");
    println!("elapsed_seconds={elapsed:.6}");
    println!("throughput_mib_s={:.3}", mib / elapsed);
    if let Some(out) = args.get(2) {
        fs::write(out, &output).unwrap_or_else(|error| {
            eprintln!("write: {error}");
            process::exit(2);
        });
    }
}
