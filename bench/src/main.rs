//! Benchmarks for zarrita-rs, mirrored by `bench/js/bench.mjs`.
//!
//!   zarrita-bench gen  <dir>            write the benchmark datasets
//!   zarrita-bench run  <dir> [iters]    time reads, print JSON lines

use std::time::Instant;
use zarrita::{
    CreateArrayOptions, DimSelection, FileSystemStore, GetOptions, create_array, get, get_par,
    open_array, root, slice,
};

const N: usize = 256;

/// (name, chunk edge, codecs JSON)
fn datasets() -> Vec<(&'static str, usize, serde_json::Value)> {
    use serde_json::json;
    let bytes = json!({ "name": "bytes", "configuration": { "endian": "little" } });
    vec![
        ("raw_c64", 64, json!([bytes])),
        (
            "gzip_c64",
            64,
            json!([bytes, { "name": "gzip", "configuration": { "level": 5 } }]),
        ),
        (
            "zstd_c64",
            64,
            json!([bytes, { "name": "zstd", "configuration": { "level": 3, "checksum": false } }]),
        ),
        (
            "blosc_lz4_c64",
            64,
            json!([bytes, { "name": "blosc", "configuration": {
                "cname": "lz4", "clevel": 5, "shuffle": "shuffle", "typesize": 2, "blocksize": 0 } }]),
        ),
        (
            "blosc_lz4_c32",
            32,
            json!([bytes, { "name": "blosc", "configuration": {
                "cname": "lz4", "clevel": 5, "shuffle": "shuffle", "typesize": 2, "blocksize": 0 } }]),
        ),
    ]
}

/// Read patterns: (name, selection).
fn selections() -> Vec<(&'static str, Vec<DimSelection>)> {
    vec![
        ("full", vec![]),
        (
            "roi_160",
            vec![
                slice(40, 200).into(),
                slice(40, 200).into(),
                slice(40, 200).into(),
            ],
        ),
        ("slice_axis0", vec![DimSelection::Index(128)]),
        (
            "slice_axis2",
            vec![
                DimSelection::All,
                DimSelection::All,
                DimSelection::Index(128),
            ],
        ),
    ]
}

/// Smooth, MRI-ish int16 volume with a little noise so it compresses realistically.
fn volume_value(z: usize, y: usize, x: usize, seed: &mut u64) -> i16 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    let noise = ((*seed >> 33) % 64) as f64 - 32.0;
    let c = (N as f64) / 2.0;
    let r = (((z as f64 - c).powi(2) + (y as f64 - c).powi(2) + (x as f64 - c).powi(2)).sqrt()) / c;
    let tissue = if r < 0.8 {
        800.0 + 300.0 * (x as f64 / 9.0).sin() * (y as f64 / 11.0).cos()
    } else {
        0.0
    };
    (tissue + (z as f64 / 5.0).sin() * 50.0 + noise) as i16
}

async fn generate(dir: &str) -> zarrita::Result<()> {
    let mut vol = vec![0i16; N * N * N];
    let mut seed = 42u64;
    for z in 0..N {
        for y in 0..N {
            for x in 0..N {
                vol[(z * N + y) * N + x] = volume_value(z, y, x, &mut seed);
            }
        }
    }
    for (name, c, codecs) in datasets() {
        let loc = root(FileSystemStore::new(format!("{dir}/{name}.zarr")));
        let opts = CreateArrayOptions::new(vec![N as u64; 3], vec![c as u64; 3], "int16")
            .codecs(serde_json::from_value(codecs).unwrap())
            .fill_value(0.into());
        let arr = create_array(&loc, opts).await?;
        let nc = N / c;
        let mut total = 0usize;
        for cz in 0..nc {
            for cy in 0..nc {
                for cx in 0..nc {
                    let mut chunk = Vec::with_capacity(c * c * c * 2);
                    for z in 0..c {
                        for y in 0..c {
                            let row = ((cz * c + z) * N + cy * c + y) * N + cx * c;
                            for v in &vol[row..row + c] {
                                chunk.extend_from_slice(&v.to_le_bytes());
                            }
                        }
                    }
                    total += chunk.len();
                    arr.set_chunk(&[cz, cy, cx], chunk.into()).await?;
                }
            }
        }
        let on_disk: u64 = std::fs::read_dir(format!("{dir}/{name}.zarr/c"))
            .map(|_| dir_size(&format!("{dir}/{name}.zarr/c")))
            .unwrap_or(0);
        eprintln!(
            "{name}: {} MiB raw, {:.1} MiB on disk",
            total >> 20,
            on_disk as f64 / (1 << 20) as f64
        );
    }
    Ok(())
}

fn dir_size(p: &str) -> u64 {
    let mut n = 0;
    for e in std::fs::read_dir(p).unwrap() {
        let e = e.unwrap();
        let m = e.metadata().unwrap();
        n += if m.is_dir() {
            dir_size(e.path().to_str().unwrap())
        } else {
            m.len()
        };
    }
    n
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[v.len() / 2]
}

async fn run(dir: &str, iters: usize) -> zarrita::Result<()> {
    let opts = GetOptions::default();
    for (name, _, _) in datasets() {
        let loc = root(FileSystemStore::new(format!("{dir}/{name}.zarr")));
        // Opening: parse metadata and build the codec pipeline.
        let mut t_open = vec![];
        for _ in 0..iters {
            let t = Instant::now();
            let _ = open_array(&loc).await?;
            t_open.push(t.elapsed().as_secs_f64() * 1e3);
        }
        println!(
            "{}",
            serde_json::json!({"impl": "rust", "dataset": name, "read": "open", "ms": median(t_open)})
        );
        let arr = open_array(&loc).await?;
        for (sel_name, sel) in selections() {
            for variant in ["rust", "rust-par"] {
                let mut times = vec![];
                let mut checksum = 0i64;
                for i in 0..iters + 2 {
                    let t = Instant::now();
                    let out = if variant == "rust" {
                        get::<i16, _>(&arr, &sel, opts).await?
                    } else {
                        get_par::<i16, _>(&arr, &sel, opts).await?
                    };
                    let ms = t.elapsed().as_secs_f64() * 1e3;
                    if i >= 2 {
                        times.push(ms); // first two are warmup
                    }
                    checksum = out.data.iter().map(|&v| v as i64).sum();
                }
                println!(
                    "{}",
                    serde_json::json!({"impl": variant, "dataset": name, "read": sel_name, "ms": median(times), "checksum": checksum})
                );
            }
        }
    }
    Ok(())
}

fn main() -> zarrita::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    match args.get(1).map(String::as_str) {
        Some("gen") => rt.block_on(generate(&args[2])),
        Some("run") => rt.block_on(run(
            &args[2],
            args.get(3).and_then(|s| s.parse().ok()).unwrap_or(10),
        )),
        _ => {
            eprintln!("usage: zarrita-bench gen|run <dir> [iters]");
            std::process::exit(2)
        }
    }
}
