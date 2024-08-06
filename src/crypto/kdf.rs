pub fn derive_key_256(password: &str, _: &str) -> [u8; 32] {
    let mut output_key_material = [0u8; 32];
    output_key_material.clone_from_slice(hex::decode(sha256::digest(password)).unwrap().as_slice());
    output_key_material
}
