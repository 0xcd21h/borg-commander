//! The certificate authority that signs what Borg's proxy shows the programs going through it:
//! one Borg makes for a capture, whose key stays in memory, or your own development CA read
//! from a folder, which also vouches for the hosts you run with it.

use borg_i18n::tr;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DistinguishedName, DnType, ExtendedKeyUsagePurpose, IsCa,
    KeyPair, KeyUsagePurpose, SanType,
};
use std::{
    collections::HashMap,
    io,
    net::IpAddr,
    path::Path,
    sync::{Arc, Mutex},
};
use time::{Duration, OffsetDateTime};
use tokio_rustls::rustls::{
    KeyLog, ServerConfig,
    crypto::ring,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject},
};

/// The certificate and key a CA folder holds, as mkcert names them first.
pub(crate) const FOLDER_FILES: [(&str, &str); 2] = [("rootCA.pem", "rootCA-key.pem"), ("ca.pem", "ca-key.pem")];
const TEMPORARY_NAME: &str = "Borg Commander capture CA";
const ORGANIZATION: &str = "Borg Commander";
/// How long the certificates Borg signs last, from a little before they are made.
const LIFETIME: Duration = Duration::days(30);
const BACKDATE: Duration = Duration::hours(1);
/// A temporary CA signs certificates for hosts only, with none below them.
const PATH_LENGTH: u8 = 0;

/// A host's certificate and its key, as the proxy shows them.
#[derive(Clone)]
struct Leaf {
    chain: Vec<CertificateDer<'static>>,
    key: Vec<u8>,
}

pub(crate) struct CertificateAuthority {
    issuer: Certificate,
    key: KeyPair,
    /// The CA's certificate as the programs that go through the proxy trust it.
    pem: String,
    /// Your own CA, which the proxy also trusts when it checks the hosts.
    own: Option<CertificateDer<'static>>,
    leaves: Mutex<HashMap<String, Leaf>>,
}

impl CertificateAuthority {
    /// A CA for one capture.
    pub(crate) fn temporary() -> Result<Self, rcgen::Error> {
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::default();
        params.distinguished_name = name(TEMPORARY_NAME);
        params.distinguished_name.push(DnType::OrganizationName, ORGANIZATION);
        params.is_ca = IsCa::Ca(BasicConstraints::Constrained(PATH_LENGTH));
        params.key_usages =
            vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign, KeyUsagePurpose::DigitalSignature];
        (params.not_before, params.not_after) = validity();
        let issuer = params.self_signed(&key)?;
        Ok(Self { pem: issuer.pem(), issuer, key, own: None, leaves: Mutex::default() })
    }

    /// The CA whose certificate and key `folder` holds.
    pub(crate) fn from_folder(folder: &Path) -> Result<Self, String> {
        let found = FOLDER_FILES
            .iter()
            .map(|(certificate, key)| (folder.join(certificate), folder.join(key)))
            .find(|(certificate, key)| certificate.is_file() && key.is_file());
        let Some((certificate, key_file)) = found else {
            let (certificate, key) = FOLDER_FILES[0];
            return Err(tr!(
                capture_proxy_no_authority,
                folder = folder.display(),
                certificate = certificate,
                key = key
            ));
        };
        let read = |path: &Path| std::fs::read_to_string(path).map_err(|error| about(path, error));
        let (pem, key) = (read(&certificate)?, read(&key_file)?);
        let own = CertificateDer::from_pem_slice(pem.as_bytes()).map_err(|error| about(&certificate, error))?;
        let key = KeyPair::from_pem(&key).map_err(|error| about(&key_file, error))?;
        let issuer = CertificateParams::from_ca_cert_pem(&pem)
            .and_then(|params| params.self_signed(&key))
            .map_err(|error| about(&certificate, error))?;
        let pem = if pem.ends_with('\n') { pem } else { format!("{pem}\n") };
        Ok(Self { issuer, key, pem, own: Some(own), leaves: Mutex::default() })
    }

    pub(crate) fn pem(&self) -> &str {
        &self.pem
    }

    /// The roots the proxy trusts beyond the system's when it checks the hosts.
    pub(crate) fn own_roots(&self) -> Vec<CertificateDer<'static>> {
        self.own.iter().cloned().collect()
    }

    /// How the proxy answers a program that wants `host`, speaking `protocol` when the host
    /// chose one, and writing its keys to `keys`.
    pub(crate) fn server_config(
        &self,
        host: &str,
        protocol: Option<Vec<u8>>,
        keys: Arc<dyn KeyLog>,
    ) -> io::Result<Arc<ServerConfig>> {
        let leaf = self.leaf(host).map_err(io::Error::other)?;
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf.key));
        let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .and_then(|builder| builder.with_no_client_auth().with_single_cert(leaf.chain, key))
            .map_err(io::Error::other)?;
        config.alpn_protocols = protocol.into_iter().collect();
        config.key_log = keys;
        Ok(Arc::new(config))
    }

    /// The certificate for `host`, made the first time a program asks for it.
    fn leaf(&self, host: &str) -> Result<Leaf, rcgen::Error> {
        if let Some(leaf) = self.leaves.lock().ok().and_then(|leaves| leaves.get(host).cloned()) {
            return Ok(leaf);
        }
        let key = KeyPair::generate()?;
        let mut params = CertificateParams::default();
        params.distinguished_name = name(host);
        params.subject_alt_names = vec![subject(host)?];
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
        params.use_authority_key_identifier_extension = true;
        (params.not_before, params.not_after) = validity();
        let certificate = params.signed_by(&key, &self.issuer, &self.key)?;
        let leaf = Leaf { chain: vec![certificate.der().clone()], key: key.serialize_der() };
        if let Ok(mut leaves) = self.leaves.lock() {
            leaves.insert(host.to_owned(), leaf.clone());
        }
        Ok(leaf)
    }
}

/// What went wrong with a file in the CA folder.
fn about(path: &Path, error: impl std::fmt::Display) -> String {
    format!("{}: {error}", path.display())
}

fn name(common: &str) -> DistinguishedName {
    let mut name = DistinguishedName::new();
    name.push(DnType::CommonName, common);
    name
}

/// The name a certificate is for: an address, or a host name.
fn subject(host: &str) -> Result<SanType, rcgen::Error> {
    match host.parse::<IpAddr>() {
        Ok(address) => Ok(SanType::IpAddress(address)),
        Err(_) => Ok(SanType::DnsName(host.try_into()?)),
    }
}

fn validity() -> (OffsetDateTime, OffsetDateTime) {
    let now = OffsetDateTime::now_utc();
    (now - BACKDATE, now + LIFETIME)
}
