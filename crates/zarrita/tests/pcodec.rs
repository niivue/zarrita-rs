//! Read arrays written by zarr-python with `numcodecs.pcodec` (v2 compressor
//! and v3 array-to-bytes codec). Fixtures: `scripts/make_pcodec_fixtures.py`.

#![cfg(all(feature = "fs", feature = "pcodec"))]

use base64::Engine;
use serde_json::Value;
use std::path::PathBuf;
use zarrita::{DataType, FileSystemStore, GetOptions, NdArray, get, open_array, root};

async fn read_bytes(
    arr: &zarrita::Array<FileSystemStore>,
) -> zarrita::Result<(Vec<u8>, Vec<usize>)> {
    fn raw<T: bytemuck::Pod>(a: NdArray<T>) -> (Vec<u8>, Vec<usize>) {
        (bytemuck::cast_slice(&a.data).to_vec(), a.shape)
    }
    let o = GetOptions::default();
    Ok(match arr.dtype() {
        DataType::Int8 => raw(get::<i8, _>(arr, &[], o).await?),
        DataType::Int16 => raw(get::<i16, _>(arr, &[], o).await?),
        DataType::Int32 => raw(get::<i32, _>(arr, &[], o).await?),
        DataType::Int64 => raw(get::<i64, _>(arr, &[], o).await?),
        DataType::Uint8 => raw(get::<u8, _>(arr, &[], o).await?),
        DataType::Uint16 => raw(get::<u16, _>(arr, &[], o).await?),
        DataType::Uint32 => raw(get::<u32, _>(arr, &[], o).await?),
        DataType::Uint64 => raw(get::<u64, _>(arr, &[], o).await?),
        DataType::Float16 => raw(get::<half::f16, _>(arr, &[], o).await?),
        DataType::Float32 => raw(get::<f32, _>(arr, &[], o).await?),
        DataType::Float64 => raw(get::<f64, _>(arr, &[], o).await?),
        other => return Err(zarrita::Error::Unsupported(other.to_string())),
    })
}

#[tokio::test]
async fn reads_pcodec_arrays() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let expected: Vec<Value> =
        serde_json::from_slice(&std::fs::read(dir.join("tests/pcodec.json")).unwrap()).unwrap();
    assert!(expected.len() >= 18);
    for e in &expected {
        let id = e["id"].as_str().unwrap();
        let store = FileSystemStore::new(dir.join("../../fixtures/pcodec.zarr"));
        let arr = open_array(&root(store).resolve(id).unwrap())
            .await
            .unwrap_or_else(|err| panic!("{id}: {err}"));
        let (bytes, shape) = read_bytes(&arr)
            .await
            .unwrap_or_else(|err| panic!("{id}: {err}"));
        let want = base64::engine::general_purpose::STANDARD
            .decode(e["data"].as_str().unwrap())
            .unwrap();
        assert_eq!(shape, vec![37, 50], "{id}");
        assert!(bytes == want, "{id}: data mismatch");
    }
}

#[cfg(feature = "encode")]
#[tokio::test]
async fn pcodec_round_trip_and_corruption() {
    use zarrita::{AbsolutePath, MemoryStore, Readable, Writable};
    let store = std::sync::Arc::new(MemoryStore::default());
    let meta = serde_json::json!({
        "zarr_format": 3, "node_type": "array", "shape": [10, 10], "data_type": "float32",
        "chunk_grid": {"name": "regular", "configuration": {"chunk_shape": [10, 10]}},
        "chunk_key_encoding": {"name": "default"}, "fill_value": 0, "attributes": {},
        "codecs": [{"name": "numcodecs.pcodec", "configuration": {"level": 4}}]
    });
    store
        .set(
            &AbsolutePath::new("/zarr.json").unwrap(),
            serde_json::to_vec(&meta).unwrap().into(),
        )
        .await
        .unwrap();
    let loc = zarrita::Location::new(store.clone(), AbsolutePath::root());
    let arr = open_array(&loc).await.unwrap();
    let values: Vec<f32> = (0..100).map(|i| i as f32 * 0.25).collect();
    arr.set_chunk(&[0, 0], bytemuck::cast_slice(&values).to_vec().into())
        .await
        .unwrap();
    let got = get::<f32, _>(&arr, &[], GetOptions::default())
        .await
        .unwrap();
    assert_eq!(got.data, values);

    let key = AbsolutePath::new("/c/0/0").unwrap();
    let stored = store.get(&key).await.unwrap().unwrap();
    store
        .set(&key, stored.slice(..stored.len() / 2))
        .await
        .unwrap();
    let err = get::<f32, _>(&arr, &[], GetOptions::default())
        .await
        .unwrap_err();
    let msg = format!("{err} {:?}", std::error::Error::source(&err));
    assert!(msg.contains("Corrupt(\"pcodec"), "{msg}");
}
