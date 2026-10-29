//! Development only: makes a signing key and a JWKS file in a folder, and prints a token signed with it.
//!
//!     cargo run --example dev_token -- <folder> <subject> [organization]
//!
//! Start the server with `LOAMZ_DATA_JWKS_FILE=<folder>/jwks.json`. Never use this key anywhere real.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::print_stdout)]

use std::{fs, path::PathBuf};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use rsa::{
    RsaPrivateKey,
    pkcs1::{DecodeRsaPrivateKey, EncodeRsaPrivateKey},
    traits::PublicKeyParts,
};
use serde_json::json;

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = PathBuf::from(args.next().expect("folder"));
    let sub = args.next().expect("subject");
    let org = args.next();
    fs::create_dir_all(&dir).unwrap();
    let pem_path = dir.join("key.pem");
    let key = if pem_path.exists() {
        RsaPrivateKey::from_pkcs1_pem(&fs::read_to_string(&pem_path).unwrap()).unwrap()
    } else {
        let key = RsaPrivateKey::new(&mut rand::thread_rng(), 2048).unwrap();
        fs::write(
            &pem_path,
            key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF)
                .unwrap()
                .as_bytes(),
        )
        .unwrap();
        key
    };
    let public = key.to_public_key();
    let jwks = json!({"keys": [{"kid": "dev", "kty": "RSA", "use": "sig", "alg": "RS256",
        "n": URL_SAFE_NO_PAD.encode(public.n().to_bytes_be()), "e": URL_SAFE_NO_PAD.encode(public.e().to_bytes_be())}]});
    fs::write(dir.join("jwks.json"), jwks.to_string()).unwrap();
    let mut claims = json!({"sub": sub, "iss": "http://dev.local/realm", "aud": "loamz-data",
        "exp": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() + 3600});
    if let Some(org) = org {
        claims["organization"] = json!({ org: {} });
    }
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some("dev".into());
    let pem = key.to_pkcs1_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    println!(
        "{}",
        encode(
            &header,
            &claims,
            &EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap()
        )
        .unwrap()
    );
}
