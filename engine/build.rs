//! build.rs: compiles the game key into the engine. GAME_KEY is the hex the editor shows on a
//! release export; it's stored XOR-masked with random bytes, so it isn't sitting in the binary as
//! a plain hex string for `strings` to find. This only deters casual extraction: anyone with a
//! debugger can still read the key while the game runs.
use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};

fn main() {
    println!("cargo:rerun-if-env-changed=GAME_KEY");
    let code = match std::env::var("GAME_KEY") {
        Err(_) => "const GAME_KEY: Option<([u8; 32], [u8; 32])> = None;".to_string(),
        Ok(hex) => {
            let hex = hex.trim();
            assert!(
                hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()),
                "GAME_KEY must be the 64 hex digits the editor shows on release export"
            );
            let key: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).unwrap()).collect();
            // random mask bytes: each RandomState is seeded from the OS
            let mask: Vec<u8> = (0..32)
                .map(|i| {
                    let mut h = RandomState::new().build_hasher();
                    h.write_usize(i);
                    h.finish() as u8
                })
                .collect();
            let masked: Vec<u8> = key.iter().zip(&mask).map(|(k, m)| k ^ m).collect();
            format!("const GAME_KEY: Option<([u8; 32], [u8; 32])> = Some(({masked:?}, {mask:?}));")
        }
    };
    let out = std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("key.rs");
    std::fs::write(out, code).unwrap();
}
