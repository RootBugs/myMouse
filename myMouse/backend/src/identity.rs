//! Production TLS identity.
//!
//! Each install generates one self-signed certificate on first run and stores
//! its DER cert/key inside the myMouse config directory. The certificate
//! fingerprint is the public pairing token returned by `localIdentity`; the
//! private key is never written to stdout, logs, RPC responses or debug output.

use sha2::{Digest, Sha256};
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

const CERT_FILE: &str = "tls_cert.der";
const KEY_FILE: &str = "tls_key.der";

pub struct Identity {
    pub cert_der: Vec<u8>,
    pub key_der: Vec<u8>,
    fingerprint: String,
}

impl fmt::Debug for Identity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Deliberately omit key_der and cert_der: this type must be safe to log.
        f.debug_struct("Identity")
            .field("fingerprint", &self.fingerprint)
            .finish_non_exhaustive()
    }
}

impl Identity {
    /// Load the persisted identity, generating it atomically on first run.
    pub fn load_or_create(config_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(config_dir)?;
        let cert_path = config_dir.join(CERT_FILE);
        let key_path = config_dir.join(KEY_FILE);

        if let (Ok(cert), Ok(key)) = (fs::read(&cert_path), fs::read(&key_path)) {
            if !cert.is_empty() && !key.is_empty() {
                return Ok(Self::from_der(cert, key));
            }
        }

        let identity = Self::generate()?;
        identity.persist(config_dir, &cert_path, &key_path)?;
        Ok(identity)
    }

    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    fn from_der(cert_der: Vec<u8>, key_der: Vec<u8>) -> Self {
        let fingerprint = fingerprint(&cert_der);
        Self {
            cert_der,
            key_der,
            fingerprint,
        }
    }

    fn generate() -> io::Result<Self> {
        let mut params = rcgen::CertificateParams::new(vec!["mymouse.local".to_string()])
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        params.is_ca = rcgen::IsCa::NoCa;
        let key_pair = rcgen::KeyPair::generate()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        let cert = params
            .self_signed(&key_pair)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        Ok(Self::from_der(
            cert.der().to_vec(),
            key_pair.serialize_der(),
        ))
    }

    fn persist(&self, config_dir: &Path, cert_path: &Path, key_path: &Path) -> io::Result<()> {
        let cert_tmp = config_dir.join(format!("{CERT_FILE}.tmp"));
        let key_tmp = config_dir.join(format!("{KEY_FILE}.tmp"));
        fs::write(&cert_tmp, &self.cert_der)?;
        fs::write(&key_tmp, &self.key_der)?;
        fs::rename(&cert_tmp, cert_path)?;
        fs::rename(&key_tmp, key_path)?;
        Ok(())
    }
}

fn fingerprint(cert_der: &[u8]) -> String {
    Sha256::digest(cert_der)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};
    use tempfile::tempdir;

    #[test]
    fn load_or_create_persists_identity_and_stable_fingerprint() {
        let dir = tempdir().unwrap();
        let first = Identity::load_or_create(dir.path()).unwrap();

        assert!(dir.path().join(CERT_FILE).exists());
        assert!(dir.path().join(KEY_FILE).exists());
        assert_eq!(first.fingerprint.len(), 64);
        let expected = Sha256::digest(&first.cert_der)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(first.fingerprint(), expected);

        let second = Identity::load_or_create(dir.path()).unwrap();
        assert_eq!(second.fingerprint(), first.fingerprint());
        assert_eq!(second.cert_der, first.cert_der);
        assert_eq!(second.key_der, first.key_der);
    }

    #[test]
    fn debug_output_never_includes_the_private_key() {
        let dir = tempdir().unwrap();
        let identity = Identity::load_or_create(dir.path()).unwrap();
        let debug = format!("{identity:?}");
        assert!(!debug.to_lowercase().contains("key_der"));
        assert!(debug.contains(identity.fingerprint()));
    }
}
