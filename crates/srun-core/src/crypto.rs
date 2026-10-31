use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::{Digest, Sha1};

const SRUN_B64_ALPHABET: &[u8; 64] =
    b"LVoJPiCN2R8G90yg+hmFHuacZ1OWMnrsSTXkYpUq/3dlbfKwv6xztjI7DeBE45QA";
const STANDARD_B64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn build_username(username: &str, domain: &str) -> String {
    let username = username.trim();
    let domain = domain.trim();
    if domain.is_empty() || username.contains('@') {
        return username.to_owned();
    }
    if domain.starts_with('@') {
        format!("{username}{domain}")
    } else {
        format!("{username}@{domain}")
    }
}

pub fn hmac_md5_hex(password: &str, token: &str) -> String {
    let mut mac =
        Hmac::<Md5>::new_from_slice(token.as_bytes()).expect("HMAC accepts keys of any length");
    mac.update(password.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub fn sha1_hex(value: &str) -> String {
    hex::encode(Sha1::digest(value.as_bytes()))
}

pub fn srun_base64_encode(input: &[u8]) -> String {
    let standard = STANDARD.encode(input);
    standard
        .bytes()
        .map(|byte| {
            STANDARD_B64_ALPHABET
                .iter()
                .position(|candidate| *candidate == byte)
                .map_or(byte as char, |index| SRUN_B64_ALPHABET[index] as char)
        })
        .collect()
}

pub fn xencode(message: &str, key: &str) -> Vec<u8> {
    if message.is_empty() {
        return Vec::new();
    }

    let mut values = string_to_words(message, true);
    let mut key_words = string_to_words(key, false);
    if key_words.len() < 4 {
        key_words.resize(4, 0);
    }

    let n = values.len() - 1;
    let mut z = values[n];
    let delta = 0x9E37_79B9u32;
    let mut sum = 0u32;
    let mut rounds = 6u32 + 52 / (n as u32 + 1);

    while rounds > 0 {
        rounds -= 1;
        sum = sum.wrapping_add(delta);
        let e = (sum >> 2) & 3;

        for p in 0..n {
            let y = values[p + 1];
            let m = (z >> 5 ^ y << 2).wrapping_add(
                (y >> 3 ^ z << 4 ^ (sum ^ y)).wrapping_add(key_words[(p & 3) ^ e as usize] ^ z),
            );
            values[p] = values[p].wrapping_add(m);
            z = values[p];
        }

        let p = n;
        let y = values[0];
        let m = (z >> 5 ^ y << 2).wrapping_add(
            (y >> 3 ^ z << 4 ^ (sum ^ y)).wrapping_add(key_words[(p & 3) ^ e as usize] ^ z),
        );
        values[n] = values[n].wrapping_add(m);
        z = values[n];
    }

    words_to_bytes(&values)
}

fn string_to_words(value: &str, include_length: bool) -> Vec<u32> {
    let units: Vec<u16> = value.encode_utf16().collect();
    let mut words = Vec::with_capacity(units.len().div_ceil(4) + usize::from(include_length));
    for chunk in units.chunks(4) {
        let mut word = 0u32;
        for (index, unit) in chunk.iter().enumerate() {
            word |= u32::from(*unit) << (index * 8);
        }
        words.push(word);
    }
    if include_length {
        words.push(units.len() as u32);
    }
    words
}

fn words_to_bytes(words: &[u32]) -> Vec<u8> {
    let mut output = Vec::with_capacity(words.len() * 4);
    for word in words {
        output.extend_from_slice(&word.to_le_bytes());
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_operator_username_without_double_suffix() {
        assert_eq!(build_username("2026001", ""), "2026001");
        assert_eq!(build_username("2026001", "cmcc"), "2026001@cmcc");
        assert_eq!(build_username("2026001", "@cucc"), "2026001@cucc");
        assert_eq!(build_username("2026001@ctcc", "cmcc"), "2026001@ctcc");
    }

    #[test]
    fn known_hash_vectors_are_stable() {
        assert_eq!(
            hmac_md5_hex("example", "sample-token"),
            "a017ae5be19d3c7b56b9e622327cada2"
        );
        assert_eq!(
            sha1_hex("srun-campus-wifi-assistant"),
            "1f48e79319ff596577c6748f8aced89408a9710b"
        );
    }

    #[test]
    fn xencode_matches_the_python_reference() {
        assert_eq!(
            hex::encode(xencode("hello", "12345678901234567890123456789012")),
            "f1093f5f3ed9252b68483a8a"
        );
    }

    #[test]
    fn srun_base64_matches_the_python_reference() {
        let encrypted = xencode(
            r#"{"username":"demo","password":"example","ip":"10.0.0.2","acid":"5","enc_ver":"srun_bx1"}"#,
            "sample-token",
        );
        assert_eq!(
            hex::encode(&encrypted),
            concat!(
                "b41a8e83599cbddd8c699f80735addf511bd39d7dfa98fffe9e9111f568f5138",
                "f6239b98fa168218c103f2c0703efa07618e4467ef7988b8c6a562abc230a37ef",
                "886037bf48e38befcdf8a1a01117a5b3857bb55140f6c9f969e3c2c"
            )
        );
        assert_eq!(
            srun_base64_encode(&encrypted),
            "fVdySjUMwnI9O1QLMjln5hC5ynssdZAABrYhNjOgHFkI2eKZQTOoC9PJ4bVvgw/NZZeP1Q5eXGkC3a8lvkokswXCL7wtkkXQA0Q8CSPhrpbDuEfuiL5bqeOrgov="
        );
    }
}
