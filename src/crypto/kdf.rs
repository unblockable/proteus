use argon2::Argon2;

pub fn derive_key_256(password: &str, salt: &str) -> [u8; 32] {
    let mut output_key_material = [0u8; 32]; // Can be any desired size
    Argon2::default()
        .hash_password_into(
            password.as_bytes(),
            salt.as_bytes(),
            &mut output_key_material,
        )
        .unwrap();
    output_key_material
}
