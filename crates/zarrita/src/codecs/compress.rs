//! gzip / zlib / zstd / lz4 byte codecs (ports of `codecs/gzip.ts`,
//! `codecs/zlib.ts`; TS uses numcodecs' wasm builds for zstd and lz4).

// Compression levels are only read when encoding.
#![cfg_attr(not(feature = "encode"), allow(dead_code))]

use super::{BytesToBytesCodec, CodecConfig, config_i64};
use crate::{Error, Result};
use bytes::Bytes;
use std::io::Read;
#[cfg(feature = "encode")]
use std::io::Write;

fn other(e: impl std::fmt::Display) -> Error {
    Error::Corrupt(e.to_string())
}

pub struct GzipCodec {
    level: u32,
}
impl GzipCodec {
    pub fn from_config(c: &CodecConfig) -> Self {
        Self {
            level: config_i64(c, "level").unwrap_or(5).clamp(0, 9) as u32,
        }
    }
}
impl BytesToBytesCodec for GzipCodec {
    #[cfg(feature = "encode")]
    fn encode(&self, b: Bytes) -> Result<Bytes> {
        let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(self.level));
        e.write_all(&b).map_err(other)?;
        Ok(e.finish().map_err(other)?.into())
    }
    #[cfg(not(feature = "encode"))]
    fn encode(&self, _b: Bytes) -> Result<Bytes> {
        Err(Error::Unsupported(
            "encoding requires the `encode` feature".into(),
        ))
    }
    fn decode(&self, b: Bytes) -> Result<Bytes> {
        // gzip trailer holds the uncompressed size (mod 2^32): use it to presize.
        let hint = b
            .len()
            .checked_sub(4)
            .map(|i| u32::from_le_bytes(b[i..].try_into().unwrap()) as usize);
        let mut out = Vec::with_capacity(hint.unwrap_or(b.len() * 4));
        flate2::read::MultiGzDecoder::new(&b[..])
            .read_to_end(&mut out)
            .map_err(other)?;
        Ok(out.into())
    }
}

pub struct ZlibCodec {
    level: u32,
}
impl ZlibCodec {
    pub fn from_config(c: &CodecConfig) -> Self {
        Self {
            level: config_i64(c, "level").unwrap_or(1).clamp(0, 9) as u32,
        }
    }
}
impl BytesToBytesCodec for ZlibCodec {
    #[cfg(feature = "encode")]
    fn encode(&self, b: Bytes) -> Result<Bytes> {
        let mut e =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(self.level));
        e.write_all(&b).map_err(other)?;
        Ok(e.finish().map_err(other)?.into())
    }
    #[cfg(not(feature = "encode"))]
    fn encode(&self, _b: Bytes) -> Result<Bytes> {
        Err(Error::Unsupported(
            "encoding requires the `encode` feature".into(),
        ))
    }
    fn decode(&self, b: Bytes) -> Result<Bytes> {
        let mut out = Vec::with_capacity(b.len() * 4);
        flate2::read::ZlibDecoder::new(&b[..])
            .read_to_end(&mut out)
            .map_err(other)?;
        Ok(out.into())
    }
}

pub struct ZstdCodec {
    level: i32,
}
impl ZstdCodec {
    pub fn from_config(c: &CodecConfig) -> Self {
        Self {
            level: config_i64(c, "level").unwrap_or(0) as i32,
        }
    }
}
impl BytesToBytesCodec for ZstdCodec {
    #[cfg(feature = "encode")]
    fn encode(&self, b: Bytes) -> Result<Bytes> {
        Ok(zstd::bulk::compress(&b, self.level).map_err(other)?.into())
    }
    #[cfg(not(feature = "encode"))]
    fn encode(&self, _b: Bytes) -> Result<Bytes> {
        Err(Error::Unsupported(
            "encoding requires the `encode` feature".into(),
        ))
    }
    fn decode(&self, b: Bytes) -> Result<Bytes> {
        match zstd::zstd_safe::get_frame_content_size(&b) {
            Ok(Some(n)) => Ok(zstd::bulk::decompress(&b, n as usize)
                .map_err(other)?
                .into()),
            _ => Ok(zstd::stream::decode_all(&b[..]).map_err(other)?.into()),
        }
    }
}

/// numcodecs `lz4`: a little-endian u32 decoded size, then one LZ4 block.
pub struct Lz4Codec;
impl BytesToBytesCodec for Lz4Codec {
    #[cfg(feature = "encode")]
    fn encode(&self, b: Bytes) -> Result<Bytes> {
        Ok(lz4_flex::block::compress_prepend_size(&b).into())
    }
    #[cfg(not(feature = "encode"))]
    fn encode(&self, _b: Bytes) -> Result<Bytes> {
        Err(Error::Unsupported(
            "encoding requires the `encode` feature".into(),
        ))
    }
    fn decode(&self, b: Bytes) -> Result<Bytes> {
        let (head, block) = b
            .split_first_chunk::<4>()
            .ok_or_else(|| other("lz4: missing size header"))?;
        let n = u32::from_le_bytes(*head) as usize;
        // LZ4 expands at most ~255x, so a larger header is damage; checking it
        // stops a corrupt chunk from forcing a multi-GB allocation.
        if n > block.len().saturating_mul(255) {
            return Err(other("lz4: size header exceeds block"));
        }
        let out = lz4_flex::block::decompress(block, n).map_err(other)?;
        if out.len() != n {
            return Err(other("lz4: decoded size mismatch"));
        }
        Ok(out.into())
    }
}

#[cfg(all(test, feature = "encode"))]
mod tests {
    use super::*;

    /// Damaged data must be reported as corrupt, never as an unsupported feature
    /// (the JS layer falls back to other decoders only for the latter).
    #[test]
    fn corrupt_data_is_corrupt_not_unsupported() {
        let raw = Bytes::from(
            (0..10_000u32)
                .flat_map(|i| (i / 3).to_le_bytes())
                .collect::<Vec<u8>>(),
        );
        let cfg = CodecConfig::new();
        let codecs: Vec<Box<dyn BytesToBytesCodec>> = vec![
            Box::new(GzipCodec::from_config(&cfg)),
            Box::new(ZlibCodec::from_config(&cfg)),
            Box::new(ZstdCodec::from_config(&cfg)),
            Box::new(Lz4Codec),
        ];
        for c in codecs {
            let mut enc = c.encode(raw.clone()).unwrap().to_vec();
            assert_eq!(c.decode(enc.clone().into()).unwrap(), raw);
            let n = enc.len();
            enc.truncate(n / 2);
            let e = c.decode(enc.into()).unwrap_err();
            assert!(matches!(e, Error::Corrupt(_)), "got {e:?}");
        }
    }
}
