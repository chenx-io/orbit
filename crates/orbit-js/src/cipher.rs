//! # orbit-js / cipher
//!
//! Block ciphers compatible with the Postman sandbox - crypto-js semantics:
//!
//! - AES（128/192/256）CBC / ECB
//! - DES（64-bit）CBC / ECB
//! - TripleDES（112/168-bit）CBC / ECB
//! - padding: PKCS#7 (default) / NoPadding / ZeroPadding
//!
//! Inputs and outputs are byte slices / `Vec<u8>`; for passphrase mode (OpenSSL `Salted__` format) the
//! key derivation is in [`crate::crypto::evp_bytes_to_key`].

use crate::crypto::{pkcs7_pad, pkcs7_unpad};

/// Supported block cipher modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockMode {
    Cbc,
    Ecb,
}

/// Supported padding modes (corresponding to crypto-js `pad.Pkcs7/NoPadding/ZeroPadding`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Padding {
    Pkcs7,
    None,
    Zero,
}

impl Padding {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "pkcs7" | "" => Ok(Padding::Pkcs7),
            "none" | "nopadding" => Ok(Padding::None),
            "zero" | "zeropadding" => Ok(Padding::Zero),
            other => Err(format!(
                "unsupported padding: {} (choose pkcs7 / none / zero)",
                other
            )),
        }
    }
}

impl BlockMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "cbc" | "" => Ok(BlockMode::Cbc),
            "ecb" => Ok(BlockMode::Ecb),
            other => Err(format!("unsupported mode: {} (choose cbc / ecb)", other)),
        }
    }
}

/// Pad the input according to the padding mode before encryption; remove padding after decryption.
fn pad_for(mode: &Padding, data: &[u8], block: usize) -> Result<Vec<u8>, String> {
    match mode {
        Padding::Pkcs7 => Ok(pkcs7_pad(data, block)),
        Padding::None => {
            if !data.len().is_multiple_of(block) {
                return Err(format!(
                    "NoPadding: data length {} is not a multiple of the block size {}",
                    data.len(),
                    block
                ));
            }
            Ok(data.to_vec())
        }
        Padding::Zero => {
            let mut out = data.to_vec();
            let pad = block - (data.len() % block);
            if pad != block {
                out.extend(std::iter::repeat_n(0u8, pad));
            }
            Ok(out)
        }
    }
}

fn unpad_for(mode: &Padding, data: &[u8], block: usize) -> Result<Vec<u8>, String> {
    match mode {
        Padding::Pkcs7 => pkcs7_unpad(data, block),
        Padding::None => Ok(data.to_vec()),
        Padding::Zero => {
            // Strip trailing consecutive 0x00 (crypto-js ZeroPadding decryption semantics)
            let mut end = data.len();
            while end > 0 && data[end - 1] == 0 {
                end -= 1;
            }
            Ok(data[..end].to_vec())
        }
    }
}

/// AES encryption/decryption (CBC/ECB + padding).
pub fn aes_crypt(
    encrypt: bool,
    data: &[u8],
    key: &[u8],
    iv: Option<&[u8]>,
    mode: BlockMode,
    padding: Padding,
) -> Result<Vec<u8>, String> {
    use aes::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit};
    use aes::{Aes128, Aes192, Aes256};

    let kl = match key.len() {
        16 | 24 | 32 => key.len(),
        n => {
            return Err(format!(
                "aes: invalid key length {} (needs 16/24/32 bytes)",
                n
            ))
        }
    };
    const BLOCK: usize = 16;

    macro_rules! run_aes {
        ($ty:ty) => {{
            if mode == BlockMode::Cbc {
                type CbcEnc = cbc::Encryptor<$ty>;
                type CbcDec = cbc::Decryptor<$ty>;
                let iv_arr: [u8; 16] = iv
                    .ok_or("aes: CBC mode requires an IV")?
                    .try_into()
                    .map_err(|_| "aes: IV must be 16 bytes")?;
                if encrypt {
                    let cipher =
                        CbcEnc::new_from_slices(key, &iv_arr).map_err(|e| format!("aes: {}", e))?;
                    let padded = pad_for(&padding, data, BLOCK)?;
                    let mut buf = padded.clone();
                    cipher
                        .encrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(
                            &mut buf,
                            padded.len(),
                        )
                        .map_err(|e| format!("aes-encrypt: {}", e))?;
                    Ok(buf.to_vec())
                } else {
                    let cipher =
                        CbcDec::new_from_slices(key, &iv_arr).map_err(|e| format!("aes: {}", e))?;
                    let mut buf = data.to_vec();
                    let raw = cipher
                        .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf)
                        .map_err(|e| format!("aes-decrypt: {}", e))?;
                    unpad_for(&padding, raw, BLOCK)
                }
            } else {
                type EcbEnc = ecb::Encryptor<$ty>;
                type EcbDec = ecb::Decryptor<$ty>;
                if encrypt {
                    let cipher = EcbEnc::new_from_slice(key).map_err(|e| format!("aes: {}", e))?;
                    let padded = pad_for(&padding, data, BLOCK)?;
                    let mut buf = padded.clone();
                    cipher
                        .encrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(
                            &mut buf,
                            padded.len(),
                        )
                        .map_err(|e| format!("aes-encrypt: {}", e))?;
                    Ok(buf.to_vec())
                } else {
                    let cipher = EcbDec::new_from_slice(key).map_err(|e| format!("aes: {}", e))?;
                    let mut buf = data.to_vec();
                    let raw = cipher
                        .decrypt_padded_mut::<aes::cipher::block_padding::NoPadding>(&mut buf)
                        .map_err(|e| format!("aes-decrypt: {}", e))?;
                    unpad_for(&padding, raw, BLOCK)
                }
            }
        }};
    }

    match kl {
        16 => run_aes!(Aes128),
        24 => run_aes!(Aes192),
        _ => run_aes!(Aes256),
    }
}

/// DES / TripleDES encryption/decryption (CBC/ECB + padding). `algo` = "des" | "3des"/"tripledes".
pub fn des_crypt(
    algo: &str,
    encrypt: bool,
    data: &[u8],
    key: &[u8],
    iv: Option<&[u8]>,
    mode: BlockMode,
    padding: Padding,
) -> Result<Vec<u8>, String> {
    use des::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, KeyIvInit};
    use des::{Des, TdesEde3};

    let (kl, is3) = match algo.to_ascii_lowercase().as_str() {
        "des" => (8usize, false),
        "3des" | "tripledes" => (24usize, true),
        other => return Err(format!("des_crypt: unknown algorithm {}", other)),
    };
    if key.len() != kl {
        return Err(format!(
            "{}: invalid key length {} (needs {} bytes)",
            algo,
            key.len(),
            kl
        ));
    }
    const BLOCK: usize = 8;

    macro_rules! run_des {
        ($ty:ty) => {{
            if mode == BlockMode::Cbc {
                type CbcEnc = cbc::Encryptor<$ty>;
                type CbcDec = cbc::Decryptor<$ty>;
                let iv_arr: [u8; 8] = iv
                    .ok_or("des: CBC mode requires an IV")?
                    .try_into()
                    .map_err(|_| "des: IV must be 8 bytes")?;
                if encrypt {
                    let cipher = CbcEnc::new_from_slices(key, &iv_arr)
                        .map_err(|e| format!("{}: {}", algo, e))?;
                    let padded = pad_for(&padding, data, BLOCK)?;
                    let mut buf = padded.clone();
                    cipher
                        .encrypt_padded_mut::<des::cipher::block_padding::NoPadding>(
                            &mut buf,
                            padded.len(),
                        )
                        .map_err(|e| format!("{}-encrypt: {}", algo, e))?;
                    Ok(buf.to_vec())
                } else {
                    let cipher = CbcDec::new_from_slices(key, &iv_arr)
                        .map_err(|e| format!("{}: {}", algo, e))?;
                    let mut buf = data.to_vec();
                    let raw = cipher
                        .decrypt_padded_mut::<des::cipher::block_padding::NoPadding>(&mut buf)
                        .map_err(|e| format!("{}-decrypt: {}", algo, e))?;
                    unpad_for(&padding, raw, BLOCK)
                }
            } else {
                type EcbEnc = ecb::Encryptor<$ty>;
                type EcbDec = ecb::Decryptor<$ty>;
                if encrypt {
                    let cipher =
                        EcbEnc::new_from_slice(key).map_err(|e| format!("{}: {}", algo, e))?;
                    let padded = pad_for(&padding, data, BLOCK)?;
                    let mut buf = padded.clone();
                    cipher
                        .encrypt_padded_mut::<des::cipher::block_padding::NoPadding>(
                            &mut buf,
                            padded.len(),
                        )
                        .map_err(|e| format!("{}-encrypt: {}", algo, e))?;
                    Ok(buf.to_vec())
                } else {
                    let cipher =
                        EcbDec::new_from_slice(key).map_err(|e| format!("{}: {}", algo, e))?;
                    let mut buf = data.to_vec();
                    let raw = cipher
                        .decrypt_padded_mut::<des::cipher::block_padding::NoPadding>(&mut buf)
                        .map_err(|e| format!("{}-decrypt: {}", algo, e))?;
                    unpad_for(&padding, raw, BLOCK)
                }
            }
        }};
    }

    if is3 {
        run_des!(TdesEde3)
    } else {
        run_des!(Des)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::{b64_decode, b64_encode};
    // crypto::hex_decode returns a Result; the test uses a local wrapper
    fn hex_decode(s: &str) -> Vec<u8> {
        crate::crypto::hex_decode(s).unwrap()
    }

    #[test]
    fn aes256_cbc_known_vector() {
        // NIST SP 800-38A F.2.1: AES-256-CBC (data is a full block, Pkcs7 does not affect the result)
        let key = hex_decode("603deb1015ca71be2b73aef0857d77811f352c073b6108d72d9810a30914dff4");
        let iv = hex_decode("000102030405060708090a0b0c0d0e0f");
        let plain = hex_decode("6bc1bee22e409f96e93d7e117393172a");
        let expect = hex_decode("f58c4c04d6e5f1ba779eabfb5f7bfbd6");
        let out = aes_crypt(
            true,
            &plain,
            &key,
            Some(&iv),
            BlockMode::Cbc,
            Padding::Pkcs7,
        )
        .unwrap();
        assert_eq!(&out[..16], expect.as_slice());
        let back = aes_crypt(false, &out, &key, Some(&iv), BlockMode::Cbc, Padding::Pkcs7).unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn aes_ecb_known_vector() {
        // NIST SP 800-38A F.1.1: AES-128-ECB
        let key = hex_decode("2b7e151628aed2a6abf7158809cf4f3c");
        let plain = hex_decode("6bc1bee22e409f96e93d7e117393172a");
        let expect = hex_decode("3ad77bb40d7a3660a89ecaf32466ef97");
        let out = aes_crypt(true, &plain, &key, None, BlockMode::Ecb, Padding::Pkcs7).unwrap();
        assert_eq!(&out[..16], expect.as_slice());
    }

    #[test]
    fn aes_passphrase_openssl_roundtrip() {
        // Simulate crypto-js passphrase mode: EVP_BytesToKey + AES-256-CBC + Salted__ format
        let salt = b"12345678";
        let (key, iv) = crate::crypto::evp_bytes_to_key(b"mysecret", salt, 32, 16);
        let plain = b"hello world";
        let ct = aes_crypt(true, plain, &key, Some(&iv), BlockMode::Cbc, Padding::Pkcs7).unwrap();
        let pt = aes_crypt(false, &ct, &key, Some(&iv), BlockMode::Cbc, Padding::Pkcs7).unwrap();
        assert_eq!(pt, plain);
        // Full OpenSSL format: Salted__ + salt + ciphertext, decryptable by the openssl CLI
        let mut openssl = b"Salted__".to_vec();
        openssl.extend_from_slice(salt);
        openssl.extend_from_slice(&ct);
        let b64 = b64_encode(&openssl);
        let decoded = b64_decode(&b64).unwrap();
        assert_eq!(&decoded[..8], b"Salted__");
        let (k2, i2) = crate::crypto::evp_bytes_to_key(b"mysecret", &decoded[8..16], 32, 16);
        let pt2 = aes_crypt(
            false,
            &decoded[16..],
            &k2,
            Some(&i2),
            BlockMode::Cbc,
            Padding::Pkcs7,
        )
        .unwrap();
        assert_eq!(pt2, plain);
    }

    #[test]
    fn aes_nopadding_requires_block_alignment() {
        let key = hex_decode("2b7e151628aed2a6abf7158809cf4f3c");
        assert!(aes_crypt(
            true,
            b"not aligned!",
            &key,
            None,
            BlockMode::Ecb,
            Padding::None
        )
        .is_err());
        // Can encrypt/decrypt after alignment
        let data = hex_decode("6bc1bee22e409f96e93d7e117393172a");
        let ct = aes_crypt(true, &data, &key, None, BlockMode::Ecb, Padding::None).unwrap();
        let pt = aes_crypt(false, &ct, &key, None, BlockMode::Ecb, Padding::None).unwrap();
        assert_eq!(pt, data);
    }

    #[test]
    fn aes_zeropadding_roundtrip() {
        let key = hex_decode("2b7e151628aed2a6abf7158809cf4f3c");
        let plain = b"hello";
        let ct = aes_crypt(true, plain, &key, None, BlockMode::Ecb, Padding::Zero).unwrap();
        let pt = aes_crypt(false, &ct, &key, None, BlockMode::Ecb, Padding::Zero).unwrap();
        assert_eq!(pt, plain);
    }

    #[test]
    fn des_cbc_roundtrip() {
        let key = hex_decode("0123456789abcdef");
        let iv = hex_decode("1234567890abcdef");
        let plain = b"Now is the time";
        let out = des_crypt(
            "des",
            true,
            plain,
            &key,
            Some(&iv),
            BlockMode::Cbc,
            Padding::Pkcs7,
        )
        .unwrap();
        let back = des_crypt(
            "des",
            false,
            &out,
            &key,
            Some(&iv),
            BlockMode::Cbc,
            Padding::Pkcs7,
        )
        .unwrap();
        assert_eq!(back, plain);
    }

    #[test]
    fn tripledes_cbc_roundtrip() {
        let key = hex_decode("0123456789abcdeffedcba98765432100123456789abcdef");
        let iv = hex_decode("1234567890abcdef");
        let plain = b"triple des test data";
        let out = des_crypt(
            "3des",
            true,
            plain,
            &key,
            Some(&iv),
            BlockMode::Cbc,
            Padding::Pkcs7,
        )
        .unwrap();
        let back = des_crypt(
            "3des",
            false,
            &out,
            &key,
            Some(&iv),
            BlockMode::Cbc,
            Padding::Pkcs7,
        )
        .unwrap();
        assert_eq!(back, plain);
    }
}
