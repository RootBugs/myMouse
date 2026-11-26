use mymouse_protocol::Message;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::server::danger::{ClientCertVerified, ClientCertVerifier};
use rustls::{
    ClientConfig, DigitallySignedStruct, DistinguishedName, Error as RustlsError, ServerConfig,
    SignatureScheme,
};
use sha2::{Digest, Sha256};
use std::io;
use std::net::SocketAddr;
use std::sync::{Arc, RwLock};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;
use tokio_rustls::TlsAcceptor;

pub type TransportResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// Trusted client certificates, keyed by SHA-256 fingerprint and mapped to the
/// device ID that owns them. Updated after a successful pairing/connection so
/// the listener accepts only peers whose identity was explicitly pinned.
pub type ClientPinMap = Arc<RwLock<std::collections::HashMap<String, String>>>;

const MAX_FRAME_LEN: usize = 1024 * 1024;

fn fingerprint(cert: &[u8]) -> String {
    Sha256::digest(cert)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn crypto_provider() -> Arc<rustls::crypto::CryptoProvider> {
    let provider = rustls::crypto::ring::default_provider();
    let _ = rustls::crypto::CryptoProvider::install_default(provider.clone());
    Arc::new(provider)
}

#[derive(Debug)]
struct PinnedVerifier {
    fingerprints: Vec<String>,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ServerCertVerifier for PinnedVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, RustlsError> {
        if self.fingerprints.contains(&fingerprint(end_entity.as_ref())) {
            Ok(ServerCertVerified::assertion())
        } else {
            Err(RustlsError::General("unpinned peer certificate".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

#[derive(Debug)]
struct PinnedClientVerifier {
    pins: ClientPinMap,
    provider: Arc<rustls::crypto::CryptoProvider>,
}

impl ClientCertVerifier for PinnedClientVerifier {
    fn offer_client_auth(&self) -> bool {
        true
    }

    fn client_auth_mandatory(&self) -> bool {
        true
    }

    fn root_hint_subjects(&self) -> &[DistinguishedName] {
        &[]
    }

    fn verify_client_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _now: UnixTime,
    ) -> Result<ClientCertVerified, RustlsError> {
        let fp = fingerprint(end_entity.as_ref());
        if self.pins.read().map(|pins| pins.contains_key(&fp)).unwrap_or(false) {
            Ok(ClientCertVerified::assertion())
        } else {
            Err(RustlsError::General("unpinned client certificate".into()))
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls12_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, RustlsError> {
        rustls::crypto::verify_tls13_signature(
            message,
            cert,
            dss,
            &self.provider.signature_verification_algorithms,
        )
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.provider
            .signature_verification_algorithms
            .supported_schemes()
    }
}

pub struct Server {
    local_addr: SocketAddr,
    _task: JoinHandle<()>,
    accepted: tokio::sync::Mutex<mpsc::Receiver<PeerStream>>,
}

impl Server {
    pub async fn bind(
        addr: &str,
        cert_der: Vec<u8>,
        key_der: Vec<u8>,
    ) -> TransportResult<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        crypto_provider();
        let config = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .with_no_client_auth()
            .with_single_cert(
                vec![CertificateDer::from(cert_der)],
                PrivateKeyDer::Pkcs8(key_der.into()),
            )?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let (tx, accepted) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    if let Ok(tls) = acceptor.accept(tcp).await {
                        let _ = tx.send(PeerStream::from_server(tls)).await;
                    }
                });
            }
        });
        Ok(Self {
            local_addr,
            _task: task,
            accepted: tokio::sync::Mutex::new(accepted),
        })
    }

    /// Production listener: requires a client certificate whose SHA-256
    /// fingerprint was explicitly pinned for a trusted device.
    pub async fn bind_pinned(
        addr: &str,
        cert_der: Vec<u8>,
        key_der: Vec<u8>,
        pins: ClientPinMap,
    ) -> TransportResult<Self> {
        let listener = TcpListener::bind(addr).await?;
        let local_addr = listener.local_addr()?;
        let provider = crypto_provider();
        let verifier = PinnedClientVerifier { pins, provider };
        let config = ServerConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
            .with_client_cert_verifier(Arc::new(verifier))
            .with_single_cert(
                vec![CertificateDer::from(cert_der)],
                PrivateKeyDer::Pkcs8(key_der.into()),
            )?;
        let acceptor = TlsAcceptor::from(Arc::new(config));
        let (tx, accepted) = mpsc::channel(8);
        let task = tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let acceptor = acceptor.clone();
                let tx = tx.clone();
                tokio::spawn(async move {
                    if let Ok(tls) = acceptor.accept(tcp).await {
                        let _ = tx.send(PeerStream::from_server(tls)).await;
                    }
                });
            }
        });
        Ok(Self {
            local_addr,
            _task: task,
            accepted: tokio::sync::Mutex::new(accepted),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    /// Wait for the next pinned-TLS peer connection.
    pub async fn accept(&self) -> TransportResult<PeerStream> {
        self.accepted
            .lock()
            .await
            .recv()
            .await
            .ok_or_else(|| "transport server stopped".into())
    }
}

pub struct Client;

impl Client {
    pub async fn connect(addr: SocketAddr, pinned: &[String]) -> TransportResult<PeerStream> {
        Self::connect_with_identity(addr, pinned, None).await
    }

    /// Connect with an explicit server pin and, for production peers, present
    /// our own certificate so the remote listener can authenticate us too.
    pub async fn connect_with_identity(
        addr: SocketAddr,
        pinned: &[String],
        identity: Option<(Vec<u8>, Vec<u8>)>,
    ) -> TransportResult<PeerStream> {
        let provider = crypto_provider();
        let verifier = PinnedVerifier {
            fingerprints: pinned.to_vec(),
            provider,
        };
        let builder =
            ClientConfig::builder_with_protocol_versions(&[&rustls::version::TLS13])
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(verifier));
        let config = match identity {
            Some((cert_der, key_der)) => builder.with_client_auth_cert(
                vec![CertificateDer::from(cert_der)],
                PrivateKeyDer::Pkcs8(key_der.into()),
            )?,
            None => builder.with_no_client_auth(),
        };
        let connector = tokio_rustls::TlsConnector::from(Arc::new(config));
        let tcp = TcpStream::connect(addr).await?;
        let name = ServerName::try_from("mymouse.local")
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error.to_string()))?;
        let tls = connector.connect(name, tcp).await?;
        Ok(PeerStream::from_client(tls))
    }
}

trait AsyncStream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> AsyncStream for T {}

type Stream = Box<dyn AsyncStream>;

pub struct PeerStream {
    stream: Stream,
    connected: bool,
    /// Fingerprint of the authenticated client certificate (server side only).
    peer_fingerprint: Option<String>,
}

async fn write_frame<S: AsyncWrite + Unpin>(stream: &mut S, message: &Message) -> TransportResult<()> {
    let payload = mymouse_protocol::encode(message);
    let length = payload.len() as u32;
    stream.write_all(&length.to_be_bytes()).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

async fn read_frame<S: AsyncRead + Unpin>(stream: &mut S) -> TransportResult<Message> {
    let mut header = [0u8; 4];
    stream.read_exact(&mut header).await?;
    let length = u32::from_be_bytes(header) as usize;
    if length == 0 || length > MAX_FRAME_LEN {
        return Err("invalid frame length".into());
    }
    let mut payload = vec![0u8; length];
    stream.read_exact(&mut payload).await?;
    mymouse_protocol::decode(&payload).map_err(Into::into)
}

/// Read half of a split peer connection. Kept separate from the writer so a
/// pending read is never cancelled by an outbound send (which could otherwise
/// drop part of a length-prefixed frame).
pub struct PeerReader {
    inner: tokio::io::ReadHalf<Stream>,
}

/// Write half of a split peer connection.
pub struct PeerWriter {
    inner: tokio::io::WriteHalf<Stream>,
}

impl PeerReader {
    pub async fn recv(&mut self) -> TransportResult<Message> {
        read_frame(&mut self.inner).await
    }
}

impl PeerWriter {
    pub async fn send(&mut self, message: &Message) -> TransportResult<()> {
        write_frame(&mut self.inner, message).await
    }
}

impl PeerStream {
    fn from_client(tls: ClientTlsStream<TcpStream>) -> Self {
        Self {
            stream: Box::new(tls),
            connected: true,
            peer_fingerprint: None,
        }
    }

    fn from_server(tls: ServerTlsStream<TcpStream>) -> Self {
        let peer_fingerprint = tls
            .get_ref()
            .1
            .peer_certificates()
            .and_then(|certs| certs.first())
            .map(|cert| fingerprint(cert.as_ref()));
        Self {
            stream: Box::new(tls),
            connected: true,
            peer_fingerprint,
        }
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    /// SHA-256 fingerprint of the client certificate presented on an accepted
    /// connection, if the listener required client authentication.
    pub fn peer_fingerprint(&self) -> Option<&str> {
        self.peer_fingerprint.as_deref()
    }

    /// Split the pinned-TLS stream into independent read/write halves.
    pub fn into_split(self) -> (PeerReader, PeerWriter) {
        let (reader, writer) = tokio::io::split(self.stream);
        (PeerReader { inner: reader }, PeerWriter { inner: writer })
    }

    pub async fn send(&mut self, message: &Message) -> TransportResult<()> {
        write_frame(&mut self.stream, message).await
    }

    pub async fn recv(&mut self) -> TransportResult<Message> {
        match read_frame(&mut self.stream).await {
            Ok(message) => Ok(message),
            Err(error) => {
                self.connected = false;
                Err(error)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cert() -> (Vec<u8>, Vec<u8>, String) {
        let mut params = rcgen::CertificateParams::new(vec!["mymouse.local".to_string()]).unwrap();
        params.is_ca = rcgen::IsCa::NoCa;
        let key_pair = rcgen::KeyPair::generate().unwrap();
        let cert = params.self_signed(&key_pair).unwrap();
        let der = cert.der().to_vec();
        let fp = fingerprint(&der);
        (der, key_pair.serialize_der(), fp)
    }

    #[tokio::test]
    async fn pinned_certificate_required() {
        let (server_cert, server_key, fingerprint) = test_cert();
        let server = Server::bind("127.0.0.1:0", server_cert, server_key).await.unwrap();
        let addr = server.local_addr();
        assert!(Client::connect(addr, &["wrong-fingerprint".into()]).await.is_err());
        let stream = Client::connect(addr, &[fingerprint]).await.unwrap();
        assert!(stream.connected());
    }

    #[tokio::test]
    async fn mutual_tls_requires_pinned_client_certificate() {
        let (server_cert, server_key, server_fp) = test_cert();
        let (client_cert, client_key, client_fp) = test_cert();
        let pins: ClientPinMap = Arc::new(RwLock::new(std::collections::HashMap::from([(
            client_fp.clone(),
            "pc-b".to_string(),
        )])));
        let server = Server::bind_pinned("127.0.0.1:0", server_cert, server_key, pins)
            .await
            .unwrap();
        let addr = server.local_addr();

        // A client without a certificate cannot complete this handshake. With
        // TLS 1.3 the client may only learn of the rejection on first write,
        // so assert the server never surfaces an accepted stream.
        if let Ok(mut unauthenticated) =
            Client::connect_with_identity(addr, &[server_fp.clone()], None).await
        {
            let _ = unauthenticated
                .send(&Message::Hello {
                    version: mymouse_protocol::PROTOCOL_VERSION,
                    device_id: "pc-b".to_string(),
                    name: "PC B".to_string(),
                })
                .await;
        }
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), server.accept())
                .await
                .is_err()
        );

        // A pinned client certificate is accepted and attributed to its device.
        let mut client = Client::connect_with_identity(
            addr,
            &[server_fp],
            Some((client_cert, client_key)),
        )
        .await
        .unwrap();
        client
            .send(&Message::Hello {
                version: mymouse_protocol::PROTOCOL_VERSION,
                device_id: "pc-b".to_string(),
                name: "PC B".to_string(),
            })
            .await
            .unwrap();

        let mut accepted = server.accept().await.unwrap();
        assert_eq!(accepted.peer_fingerprint(), Some(client_fp.as_str()));
        match accepted.recv().await.unwrap() {
            Message::Hello { device_id, .. } => assert_eq!(device_id, "pc-b"),
            other => panic!("unexpected message: {other:?}"),
        }
    }
}
