//! `numcodecs.pcodec` (Pcodec / pco standalone format).
//!
//! An array-to-bytes codec in Zarr v3 (`numcodecs.zarr3.PCodec`) and a
//! compressor in v2; both store `pcodec.standalone.simple_compress` output,
//! which is self-describing, so decoding only checks that the stored number
//! type matches the array's data type.

use super::{ArrayToBytesCodec, ChunkMeta, CodecConfig, RawChunk};
use crate::dtype::DataType;
use crate::util::{c_order, get_strides};
use crate::{Error, Result};
use bytes::Bytes;

pub struct PcodecCodec {
    data_type: DataType,
    shape: Vec<usize>,
    #[cfg_attr(not(feature = "encode"), allow(dead_code))]
    level: usize,
}

/// Lets decoded numbers become `Bytes` without a copy.
struct PodVec<T>(Vec<T>);
impl<T: bytemuck::Pod> AsRef<[u8]> for PodVec<T> {
    fn as_ref(&self) -> &[u8] {
        bytemuck::cast_slice(&self.0)
    }
}

fn corrupt(e: pco::errors::PcoError) -> Error {
    Error::Corrupt(format!("pcodec: {e}"))
}

/// Run `$body` with `$t` bound to the Rust number type of `$dt`.
macro_rules! dispatch {
    ($dt:expr, $t:ident => $body:expr) => {
        match $dt {
            DataType::Uint8 => {
                type $t = u8;
                $body
            }
            DataType::Uint16 => {
                type $t = u16;
                $body
            }
            DataType::Uint32 => {
                type $t = u32;
                $body
            }
            DataType::Uint64 => {
                type $t = u64;
                $body
            }
            DataType::Int8 => {
                type $t = i8;
                $body
            }
            DataType::Int16 => {
                type $t = i16;
                $body
            }
            DataType::Int32 => {
                type $t = i32;
                $body
            }
            DataType::Int64 => {
                type $t = i64;
                $body
            }
            DataType::Float16 => {
                type $t = half::f16;
                $body
            }
            DataType::Float32 => {
                type $t = f32;
                $body
            }
            DataType::Float64 => {
                type $t = f64;
                $body
            }
            other => return Err(Error::Unsupported(format!("pcodec for data type {other}"))),
        }
    };
}

impl PcodecCodec {
    pub fn from_config(c: &CodecConfig, meta: &ChunkMeta) -> Result<Self> {
        dispatch!(&meta.data_type, T => { let _ = std::mem::size_of::<T>(); });
        let level = super::config_i64(c, "level").unwrap_or(8);
        Ok(Self {
            data_type: meta.data_type.clone(),
            shape: meta.shape.clone(),
            level: level.clamp(0, 12) as usize,
        })
    }

    fn n(&self) -> usize {
        self.shape.iter().product()
    }
}

impl ArrayToBytesCodec for PcodecCodec {
    fn decode(&self, bytes: Bytes) -> Result<RawChunk> {
        let n = self.n();
        let data: Bytes = dispatch!(&self.data_type, T => {
            let mut out = vec![<T as bytemuck::Zeroable>::zeroed(); n];
            let progress = pco::standalone::simple_decompress_into::<T>(&bytes, &mut out)
                .map_err(corrupt)?;
            if progress.n_processed != n || !progress.finished {
                return Err(Error::Corrupt(format!(
                    "pcodec: expected {n} values, got {}{}",
                    progress.n_processed,
                    if progress.finished { "" } else { " or more" }
                )));
            }
            Bytes::from_owner(PodVec(out))
        });
        Ok(RawChunk {
            data,
            shape: self.shape.clone(),
            stride: get_strides(&self.shape, &c_order(self.shape.len())),
        })
    }

    #[cfg(feature = "encode")]
    fn encode(&self, chunk: RawChunk) -> Result<Bytes> {
        let config = pco::ChunkConfig::default().with_compression_level(self.level);
        let out = dispatch!(&self.data_type, T => {
            let values: &[T] = bytemuck::try_cast_slice(&chunk.data)
                .map_err(|e| Error::Corrupt(format!("pcodec: {e}")))?;
            pco::standalone::simple_compress(values, &config).map_err(corrupt)?
        });
        Ok(out.into())
    }

    #[cfg(not(feature = "encode"))]
    fn encode(&self, _chunk: RawChunk) -> Result<Bytes> {
        Err(Error::Unsupported(
            "pcodec encoding (built without `encode`)".into(),
        ))
    }
}
