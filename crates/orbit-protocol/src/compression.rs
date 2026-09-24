//! HTTP content encoding (compression / decompression) support.
//!
//! Covers the mainstream compression schemes:
//! - `gzip` (`x-gzip` accepted as a synonym)
//! - `deflate` (zlib-wrapped, RFC 1950, standard HTTP semantics)
//! - `br`（brotli）
//! - `zstd`
//! - `identity` (passthrough)
//!
//! Decoding order follows RFC 7231: the Content-Encoding header lists encodings comma-separated,
//! and the list order is the order the server applied them, so decoding must run in reverse
//! (the last entry is the outermost encoding and is decoded first).

use std::io::{Read, Write};

use flate2::read::{GzDecoder, ZlibDecoder};
use flate2::write::{GzEncoder, ZlibEncoder};
use flate2::Compression;

use crate::types::ProtocolError;

/// A content encoding, i.e. one token of the `Content-Encoding` header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentEncoding {
    Gzip,
    Brotli,
    Zstd,
    Deflate,
    Identity,
}

impl ContentEncoding {
    /// Parse from a single token of the `Content-Encoding` header (case-insensitive).
    /// Unrecognized tokens return `None`; callers should ignore or reject accordingly.
    pub fn parse_token(token: &str) -> Option<Self> {
        match token.trim().to_ascii_lowercase().as_str() {
            "gzip" | "x-gzip" => Some(ContentEncoding::Gzip),
            "br" => Some(ContentEncoding::Brotli),
            "zstd" => Some(ContentEncoding::Zstd),
            "deflate" => Some(ContentEncoding::Deflate),
            "identity" | "" => Some(ContentEncoding::Identity),
            _ => None,
        }
    }
}

/// Parse a full `Content-Encoding` header value (comma-separated, preserving order).
///
/// Unrecognized tokens return an error rather than being silently ignored - otherwise a value like `gzip, snappy` would decode
/// leftover snappy-compressed data, leaving the frontend showing garbage that is hard to debug).
pub fn parse_content_encoding(value: &str) -> Result<Vec<ContentEncoding>, ProtocolError> {
    let mut out = Vec::new();
    for token in value.split(',') {
        match ContentEncoding::parse_token(token) {
            Some(enc) => out.push(enc),
            None => {
                return Err(ProtocolError::Codec(format!(
                    "unsupported Content-Encoding token: '{}'",
                    token.trim()
                )))
            }
        }
    }
    Ok(out)
}

/// Decompress data.
///
/// `encodings` is the response `Content-Encoding` list (order of appearance).
/// Decode in reverse order per RFC 7231, popping the outermost encoding each time.
pub fn decompress(
    mut data: Vec<u8>,
    encodings: &[ContentEncoding],
) -> Result<Vec<u8>, ProtocolError> {
    for enc in encodings.iter().rev() {
        data = decode_one(data, *enc)?;
    }
    Ok(data)
}

/// Compress data.
///
/// `encodings` is the request `Content-Encoding` list (order of appearance).
/// Apply in order of appearance (the first is applied first and becomes the innermost).
pub fn compress(
    mut data: Vec<u8>,
    encodings: &[ContentEncoding],
) -> Result<Vec<u8>, ProtocolError> {
    for enc in encodings {
        data = encode_one(data, *enc)?;
    }
    Ok(data)
}

fn decode_one(data: Vec<u8>, enc: ContentEncoding) -> Result<Vec<u8>, ProtocolError> {
    match enc {
        ContentEncoding::Identity => Ok(data),
        ContentEncoding::Gzip => {
            let mut decoder = GzDecoder::new(&data[..]);
            let mut out = Vec::with_capacity(data.len());
            decoder
                .read_to_end(&mut out)
                .map_err(|e| ProtocolError::Codec(format!("gzip decompression failed: {e}")))?;
            Ok(out)
        }
        ContentEncoding::Deflate => {
            let mut decoder = ZlibDecoder::new(&data[..]);
            let mut out = Vec::with_capacity(data.len());
            decoder
                .read_to_end(&mut out)
                .map_err(|e| ProtocolError::Codec(format!("deflate decompression failed: {e}")))?;
            Ok(out)
        }
        ContentEncoding::Brotli => {
            let mut reader = brotli::Decompressor::new(&data[..], 4096);
            let mut out = Vec::with_capacity(data.len());
            reader
                .read_to_end(&mut out)
                .map_err(|e| ProtocolError::Codec(format!("brotli decompression failed: {e}")))?;
            Ok(out)
        }
        ContentEncoding::Zstd => zstd::decode_all(&data[..])
            .map_err(|e| ProtocolError::Codec(format!("zstd decompression failed: {e}"))),
    }
}

fn encode_one(data: Vec<u8>, enc: ContentEncoding) -> Result<Vec<u8>, ProtocolError> {
    match enc {
        ContentEncoding::Identity => Ok(data),
        ContentEncoding::Gzip => {
            let mut encoder =
                GzEncoder::new(Vec::with_capacity(data.len()), Compression::default());
            encoder
                .write_all(&data)
                .map_err(|e| ProtocolError::Codec(format!("gzip compression failed: {e}")))?;
            encoder
                .finish()
                .map_err(|e| ProtocolError::Codec(format!("gzip compression failed: {e}")))
        }
        ContentEncoding::Deflate => {
            let mut encoder =
                ZlibEncoder::new(Vec::with_capacity(data.len()), Compression::default());
            encoder
                .write_all(&data)
                .map_err(|e| ProtocolError::Codec(format!("deflate compression failed: {e}")))?;
            encoder
                .finish()
                .map_err(|e| ProtocolError::Codec(format!("deflate compression failed: {e}")))
        }
        ContentEncoding::Brotli => {
            let mut out = Vec::with_capacity(data.len());
            {
                let mut writer = brotli::CompressorWriter::new(&mut out, 4096, 5, 22);
                writer
                    .write_all(&data)
                    .map_err(|e| ProtocolError::Codec(format!("brotli compression failed: {e}")))?;
                writer
                    .flush()
                    .map_err(|e| ProtocolError::Codec(format!("brotli compression failed: {e}")))?;
            }
            Ok(out)
        }
        ContentEncoding::Zstd => zstd::encode_all(&data[..], 3)
            .map_err(|e| ProtocolError::Codec(format!("zstd compression failed: {e}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"{\"hello\":\"world\",\"numbers\":[1,2,3,4,5],\"nested\":{\"a\":true}}";

    #[test]
    fn parse_tokens() {
        assert_eq!(
            ContentEncoding::parse_token("gzip"),
            Some(ContentEncoding::Gzip)
        );
        assert_eq!(
            ContentEncoding::parse_token("br"),
            Some(ContentEncoding::Brotli)
        );
        assert_eq!(
            ContentEncoding::parse_token("ZSTD"),
            Some(ContentEncoding::Zstd)
        );
        assert_eq!(
            ContentEncoding::parse_token("Deflate"),
            Some(ContentEncoding::Deflate)
        );
        assert_eq!(
            ContentEncoding::parse_token(""),
            Some(ContentEncoding::Identity)
        );
        assert_eq!(ContentEncoding::parse_token("snappy"), None);
        assert_eq!(
            parse_content_encoding("gzip, br").unwrap(),
            vec![ContentEncoding::Gzip, ContentEncoding::Brotli]
        );
        // Unknown tokens must error out rather than be silently ignored
        assert!(parse_content_encoding("gzip, snappy").is_err());
    }

    #[test]
    fn roundtrip_single() {
        for enc in [
            ContentEncoding::Gzip,
            ContentEncoding::Deflate,
            ContentEncoding::Brotli,
            ContentEncoding::Zstd,
        ] {
            let encs = vec![enc];
            let compressed = compress(SAMPLE.to_vec(), &encs).unwrap();
            // Small samples may grow slightly due to header/checksum overhead; here we only verify the round trip
            assert!(!compressed.is_empty(), "compressed empty for {enc:?}");
            let decompressed = decompress(compressed, &encs).unwrap();
            assert_eq!(decompressed, SAMPLE, "roundtrip failed for {enc:?}");
        }
    }

    #[test]
    fn roundtrip_large_shrinks() {
        // A sufficiently large repetitive payload should actually shrink when compressed
        let big = vec![b'A'; 4096];
        for enc in [
            ContentEncoding::Gzip,
            ContentEncoding::Deflate,
            ContentEncoding::Brotli,
            ContentEncoding::Zstd,
        ] {
            let encs = vec![enc];
            let compressed = compress(big.clone(), &encs).unwrap();
            assert!(
                compressed.len() < big.len(),
                "{enc:?} should shrink repetitive data: {} vs {}",
                compressed.len(),
                big.len()
            );
            assert_eq!(decompress(compressed, &encs).unwrap(), big);
        }
    }

    #[test]
    fn roundtrip_chained_order() {
        // Content-Encoding: gzip, br  => gzip first, then br (application order)
        // Decoding must be in reverse: br first, then gzip.
        let encs = vec![ContentEncoding::Gzip, ContentEncoding::Brotli];
        let compressed = compress(SAMPLE.to_vec(), &encs).unwrap();
        let decompressed = decompress(compressed, &encs).unwrap();
        assert_eq!(decompressed, SAMPLE);
    }

    #[test]
    fn identity_passthrough() {
        let data = SAMPLE.to_vec();
        assert_eq!(
            compress(data.clone(), &[ContentEncoding::Identity]).unwrap(),
            data
        );
        assert_eq!(
            decompress(data.clone(), &[ContentEncoding::Identity]).unwrap(),
            data
        );
    }
}
