use super::*;
use rcgen::{
    BasicConstraints, Certificate, CertificateParams, DnType, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
    SanType,
};
use std::{collections::HashSet, net::IpAddr, os::unix::fs::PermissionsExt};
use tempfile::TempDir;
use tokio::{
    io::{AsyncRead, AsyncWrite, duplex},
    sync::mpsc::{UnboundedReceiver, unbounded_channel},
};
use tokio_rustls::{
    TlsAcceptor,
    rustls::{
        NoKeyLog, ServerConfig,
        pki_types::{PrivateKeyDer, PrivatePkcs8KeyDer, pem::PemObject},
    },
};

const HOST: &str = "localhost";
const ADDRESS: &str = "127.0.0.1";
const OFFERED: [&[u8]; 2] = [b"h2", b"http/1.1"];
const SPOKEN: &[u8] = b"http/1.1";
const MESSAGE: &[u8] = b"ping";
const KEY_LOG: &str = "capture.keylog";
const TRUST: &str = "trust.pem";
const TRAFFIC_SECRET: &str = "CLIENT_TRAFFIC_SECRET_0";
const PRIVATE: u32 = 0o600;
const PERMISSIONS: u32 = 0o777;
const LIFETIME: time::Duration = time::Duration::days(30);
const BACKDATE: time::Duration = time::Duration::hours(1);

/// A development CA as mkcert leaves it in a folder.
struct DevelopmentCa {
    folder: TempDir,
    certificate: Certificate,
    key: KeyPair,
}

impl DevelopmentCa {
    fn new() -> Self {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::default();
        params.distinguished_name.push(DnType::CommonName, "development CA");
        params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
        fresh(&mut params);
        let certificate = params.self_signed(&key).unwrap();
        let folder = tempfile::tempdir().unwrap();
        let (certificate_file, key_file) = authority::FOLDER_FILES[0];
        std::fs::write(folder.path().join(certificate_file), certificate.pem()).unwrap();
        std::fs::write(folder.path().join(key_file), key.serialize_pem()).unwrap();
        Self { folder, certificate, key }
    }

    fn root(&self) -> CertificateDer<'static> {
        self.certificate.der().clone()
    }

    fn authority(&self) -> Authority {
        Authority::Folder(self.folder.path().to_owned())
    }

    /// What a service you run with the CA shows for `localhost`, speaking HTTP/1.1.
    fn service(&self) -> ServerConfig {
        let key = KeyPair::generate().unwrap();
        let mut params = CertificateParams::new([HOST.to_owned()]).unwrap();
        params.subject_alt_names.push(SanType::IpAddress(IpAddr::from(Ipv4Addr::LOCALHOST)));
        params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        params.use_authority_key_identifier_extension = true;
        fresh(&mut params);
        let certificate = params.signed_by(&key, &self.certificate, &self.key).unwrap();
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key.serialize_der()));
        let mut config = ServerConfig::builder_with_provider(Arc::new(ring::default_provider()))
            .with_safe_default_protocol_versions()
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![certificate.der().clone()], key)
            .unwrap();
        config.alpn_protocols = vec![SPOKEN.to_vec()];
        config
    }
}

/// Valid from a little before now for a month, as the system accepts for a server.
fn fresh(params: &mut CertificateParams) {
    let now = time::OffsetDateTime::now_utc();
    (params.not_before, params.not_after) = (now - BACKDATE, now + LIFETIME);
}

/// A program that trusts `root` beside the system's roots and offers HTTP/2 and HTTP/1.1.
fn trusting(root: CertificateDer<'static>) -> TlsConnector {
    let provider = Arc::new(ring::default_provider());
    let verifier = Verifier::new_with_extra_roots([root], provider.clone()).unwrap();
    let mut config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .unwrap()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(verifier))
        .with_no_client_auth();
    config.alpn_protocols = OFFERED.map(<[u8]>::to_vec).to_vec();
    TlsConnector::from(Arc::new(config))
}

fn server_name(host: &str) -> ServerName<'static> {
    ServerName::try_from(host.to_owned()).unwrap()
}

/// Listens on the loopback address and sends back the message each connection sends, over TLS
/// when `tls` says how.
async fn echoing(tls: Option<ServerConfig>) -> SocketAddr {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
    let address = listener.local_addr().unwrap();
    let acceptor = tls.map(|config| TlsAcceptor::from(Arc::new(config)));
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let acceptor = acceptor.clone();
            tokio::spawn(async move {
                match acceptor {
                    Some(acceptor) => {
                        if let Ok(stream) = acceptor.accept(stream).await {
                            echo(stream).await;
                        }
                    }
                    None => echo(stream).await,
                }
            });
        }
    });
    address
}

async fn echo(mut stream: impl AsyncRead + AsyncWrite + Unpin) {
    let mut message = [0; MESSAGE.len()];
    if stream.read_exact(&mut message).await.is_ok() {
        let _ = stream.write_all(&message).await;
        let _ = stream.flush().await;
    }
}

/// Sends the message and reads what comes back.
async fn round_trip(stream: &mut (impl AsyncRead + AsyncWrite + Unpin)) -> Vec<u8> {
    stream.write_all(MESSAGE).await.unwrap();
    let mut echoed = vec![0; MESSAGE.len()];
    stream.read_exact(&mut echoed).await.unwrap();
    echoed
}

/// A proxy for a capture whose key log and trust bundle sit in a folder of their own.
struct Running {
    proxy: Proxy,
    events: UnboundedReceiver<Event>,
    key_log: PathBuf,
    _folder: TempDir,
}

impl Running {
    async fn start(authority: &Authority) -> Self {
        let folder = tempfile::tempdir().unwrap();
        let key_log = folder.path().join(KEY_LOG);
        std::fs::write(&key_log, "").unwrap();
        let (sender, events) = unbounded_channel();
        let report: Report = Box::new(move |event| drop(sender.send(event)));
        let proxy = Proxy::start(authority, &key_log, folder.path().join(TRUST), report).await.unwrap();
        Self { proxy, events, key_log, _folder: folder }
    }

    /// Asks for a tunnel to `target`, and gives the stream and the proxy's answer.
    async fn connect(&self, target: SocketAddr) -> (TcpStream, Vec<u8>) {
        let mut stream = TcpStream::connect(self.proxy.endpoint().address).await.unwrap();
        let request = format!("{CONNECT} {target} HTTP/1.1\r\nHost: {target}\r\n\r\n");
        stream.write_all(request.as_bytes()).await.unwrap();
        let answer = read_head(&mut stream).await.unwrap();
        (stream, answer)
    }

    async fn next(&mut self) -> Event {
        timeout(WAIT, self.events.recv()).await.unwrap().unwrap()
    }

    /// The CA certificate the trust bundle starts with.
    fn root(&self) -> CertificateDer<'static> {
        CertificateDer::from_pem_file(self.proxy.endpoint().trust).unwrap()
    }
}

#[test]
fn a_tunnel_goes_to_the_host_and_port_connect_names() {
    let head = |line: &str| format!("{line}\r\nHost: example.com\r\n\r\n");
    let asked = |line: &str| requested(head(line).as_bytes());
    let target = |host: &str, port| Ok(Target { host: host.to_owned(), port });
    assert_eq!(asked("CONNECT example.com:443 HTTP/1.1"), target("example.com", 443));
    assert_eq!(asked("CONNECT [::1]:8443 HTTP/1.1"), target("::1", 8443));
    assert_eq!(asked("CONNECT example.com HTTP/1.1"), Err(BAD_REQUEST));
    assert_eq!(asked("GET http://example.com/ HTTP/1.1"), Err(NOT_IMPLEMENTED));
    assert_eq!(requested(b"\x16\x03\x01\r\n\r\n"), Err(BAD_REQUEST));
}

#[tokio::test]
async fn a_program_that_trusts_the_temporary_ca_accepts_what_it_signs() {
    let authority = CertificateAuthority::temporary().unwrap();
    let root = CertificateDer::from_pem_slice(authority.pem().as_bytes()).unwrap();
    for host in [HOST, ADDRESS] {
        let (client, server) = duplex(HEAD_LIMIT);
        let config = authority.server_config(host, Some(SPOKEN.to_vec()), Arc::new(NoKeyLog)).unwrap();
        let accepted = tokio::spawn(TlsAcceptor::from(config).accept(server));
        let connected = trusting(root.clone()).connect(server_name(host), client).await.unwrap();
        assert_eq!(connected.get_ref().1.alpn_protocol(), Some(SPOKEN), "{host}");
        accepted.await.unwrap().unwrap();
    }
}

#[test]
fn a_ca_folder_holds_a_certificate_and_its_key() {
    let empty = tempfile::tempdir().unwrap();
    let error = CertificateAuthority::from_folder(empty.path()).err().unwrap();
    let (certificate, key) = authority::FOLDER_FILES[0];
    assert!(error.contains(certificate) && error.contains(key), "{error}");

    let ca = DevelopmentCa::new();
    let authority = CertificateAuthority::from_folder(ca.folder.path()).unwrap();
    assert_eq!(CertificateDer::from_pem_slice(authority.pem().as_bytes()).unwrap(), ca.root());
    assert_eq!(authority.own_roots(), [ca.root()], "the proxy trusts it for your own services too");
}

#[tokio::test]
async fn https_through_the_proxy_reaches_your_service_and_both_sessions_leave_their_keys() {
    let ca = DevelopmentCa::new();
    let service = echoing(Some(ca.service())).await;
    let mut running = Running::start(&ca.authority()).await;
    assert_eq!(running.root(), ca.root(), "the bundle starts with your CA");
    let trust = running.proxy.endpoint().trust;
    assert_eq!(std::fs::metadata(&trust).unwrap().permissions().mode() & PERMISSIONS, PRIVATE);

    let (stream, answer) = running.connect(service).await;
    assert_eq!(answer, ESTABLISHED);
    let client = stream.local_addr().unwrap();
    let mut tls = trusting(ca.root()).connect(server_name(HOST), stream).await.unwrap();
    assert_eq!(tls.get_ref().1.alpn_protocol(), Some(SPOKEN), "the protocol the service chose");
    assert_eq!(round_trip(&mut tls).await, MESSAGE);
    let Event::Opened { client: opened_for, remote, .. } = running.next().await else { panic!("a connection") };
    assert_eq!((opened_for, remote), (client, service));

    let keys = std::fs::read_to_string(&running.key_log).unwrap();
    let randoms: HashSet<&str> =
        keys.lines().filter_map(|line| line.strip_prefix(TRAFFIC_SECRET)?.split_whitespace().next()).collect();
    assert_eq!(randoms.len(), 2, "the program's session with the proxy and the proxy's with the service:\n{keys}");
    drop(running);
    assert!(!trust.exists(), "the bundle goes with the proxy");
}

#[tokio::test]
async fn a_program_that_does_not_trust_the_proxy_is_reported() {
    let ca = DevelopmentCa::new();
    let service = echoing(Some(ca.service())).await;
    let mut running = Running::start(&ca.authority()).await;
    let (stream, _) = running.connect(service).await;
    let stranger = DevelopmentCa::new();
    assert!(trusting(stranger.root()).connect(server_name(HOST), stream).await.is_err());
    assert!(matches!(running.next().await, Event::Opened { .. }));
    assert_eq!(running.next().await, Event::Refused { host: HOST.to_owned() });
}

#[tokio::test]
async fn a_host_that_does_not_prove_who_it_is_is_closed_and_reported() {
    let ca = DevelopmentCa::new();
    let service = echoing(Some(ca.service())).await;
    let mut running = Running::start(&Authority::Temporary).await;
    let (stream, _) = running.connect(service).await;
    assert!(trusting(running.root()).connect(server_name(HOST), stream).await.is_err());
    assert!(matches!(running.next().await, Event::Opened { .. }));
    let event = running.next().await;
    assert!(matches!(&event, Event::Unverified { host, .. } if host == HOST), "{event:?}");
}

#[tokio::test]
async fn anything_else_passes_through_and_a_host_that_is_not_there_gets_bad_gateway() {
    let service = echoing(None).await;
    let running = Running::start(&Authority::Temporary).await;
    let (mut stream, answer) = running.connect(service).await;
    assert_eq!(answer, ESTABLISHED);
    assert_eq!(round_trip(&mut stream).await, MESSAGE);

    let gone = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap().local_addr().unwrap();
    let (_, answer) = running.connect(gone).await;
    assert_eq!(answer, BAD_GATEWAY);
}
