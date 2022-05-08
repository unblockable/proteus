

Generator Procedure:

1. Decide on a field order following the random procedure described in the doc

2. Push things onto the frame once the order is decided.

Formatter:

- Needs an encryption module to encrypt payloads before they go out.

Need to add an encryption module that can interpret the EncryptedMaterial
enum and return the necessary Bytes object.

Encryption Module:

  - Interpret EncryptionMaterialKind and produce the right Bytes for what is
    asked.

  - Needs to take in EncryptionMaterialKind and update state accordingly (e.g.,
    negotiating a key)

  - Take a payload and produce ciphertext and MAC (Bytes and Bytes)

  - Take in a ciphertext and MAC and produce a cleartext

Let's assume that the formatter owns the encryption module, and calls whatever
init routines that the enc module needs.
