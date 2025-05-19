use asn1::{Asn1Read, Asn1Write};

// PEM Length: 115 bytes
// DER length: 44  bytes

#[derive(Asn1Read, Asn1Write)]
struct X25519KeyASN<'a> {
    algorithm: asn1::ObjectIdentifier,
    public_key: asn1::BitString<'a>,
}

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct X25519PubKey {
    value: [u8; 32],
}

impl Default for X25519PubKey {
    fn default() -> Self {
        Self::new()
    }
}

impl X25519PubKey {
    pub fn new() -> Self {
        use x25519_dalek::{EphemeralSecret, PublicKey};
        let secret = EphemeralSecret::random();
        let public = PublicKey::from(&secret);

        X25519PubKey {
            value: public.to_bytes(),
        }
    }

    pub fn from_bytes(value: &[u8]) -> Self {
        let mut key: [u8; 32] = [0; 32];
        assert!(value.len() == 32);
        key.clone_from_slice(value);
        X25519PubKey { value: key }
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.value
    }

    pub fn to_der(self) -> Vec<u8> {
        // <https://datatracker.ietf.org/doc/html/rfc8410#section-3>
        let key_asn = X25519KeyASN {
            algorithm: asn1::oid!(1, 3, 101, 110),
            public_key: asn1::BitString::new(&self.value, 0).unwrap(),
        };

        let der = asn1::write(|w| {
            w.write_element(&asn1::SequenceWriter::new(&|w| {
                w.write_element(&key_asn)?;
                Ok(())
            }))
        })
        .unwrap();

        der
    }

    pub fn from_der(value: Vec<u8>) -> Self {
        let result: asn1::ParseResult<_> = asn1::parse(&value, |d| {
            d.read_element::<asn1::Sequence>()?.parse(|d| {
                let k = d.read_element::<X25519KeyASN>()?;
                Ok(k)
            })
        });

        if let Ok(k) = result {
            Self::from_bytes(k.public_key.as_bytes())
        } else {
            panic!()
        }
    }

    pub fn to_pem(self) -> Vec<u8> {
        let pem = pem::Pem::new("PUBLIC KEY", self.to_der());
        let encoded = pem::encode(&pem);
        let mut bytes = encoded.trim().as_bytes().to_vec();
        bytes.push(b'\0');
        bytes
    }

    pub fn from_pem(value: Vec<u8>) -> Self {
        let der = pem::parse(value).expect("PEM value encoding");
        assert_eq!(der.tag(), "PUBLIC KEY");
        Self::from_der(der.into_contents())
    }
}

impl std::convert::From<X25519PubKey> for [u8; 32] {
    fn from(val: X25519PubKey) -> Self {
        val.value
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn test_der() {
        let key = X25519PubKey::new();
        let der = key.clone().to_der();
        let key2 = X25519PubKey::from_der(der);
        assert!(key == key2);
    }

    #[test]
    fn test_pem() {
        let key = X25519PubKey::new();
        let pem = key.clone().to_pem();
        let key2 = X25519PubKey::from_pem(pem);
        assert!(key == key2);
    }
}
