use mymouse_protocol::Message;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName, UnixTime};
use rustls::{ClientConfig, DigitallySignedStruct, Error as RustlsError, ServerConfig, SignatureScheme};
use sha2::{Digest, Sha256};
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_rustls::client::TlsStream as ClientTlsStream;
use tokio_rustls::server::TlsStream as ServerTlsStream;
use tokio_rustls::TlsAcceptor;

pub type TransportResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

pub struct Server {
    local_addr: SocketAddr,
    _task: JoinHandle<()>,
    _accepted: mpsc::Receiver<PeerStream>,
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
        let config = ServerConfig::builder()
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
            _accepted: accepted,
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }
}

pub struct Client;

impl Client {
    pub async fn connect(addr: SocketAddr, pinned: &[String]) -> TransportResult<PeerStream> {
        let provider = crypto_provider();
        let verifier = PinnedVerifier {
            fingerprints: pinned.to_vec(),
            provider,
        };
        let config = ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
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
}

impl PeerStream {
    fn from_client(tls: ClientTlsStream<TcpStream>) -> Self {
        Self {
            stream: Box::new(tls),
            connected: true,
        }
    }

    fn from_server(tls: ServerTlsStream<TcpStream>) -> Self {
        Self {
            stream: Box::new(tls),
            connected: true,
        }
    }

    pub fn connected(&self) -> bool {
        self.connected
    }

    pub async fn send(&mut self, message: &Message) -> TransportResult<()> {
        let payload = mymouse_protocol::encode(message);
        let length = payload.len() as u32;
        self.stream.write_all(&length.to_be_bytes()).await?;
        self.stream.write_all(&payload).await?;
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> TransportResult<Message> {
        let mut header = [0u8; 4];
        self.stream.read_exact(&mut header).await?;
        let length = u32::from_be_bytes(header) as usize;
        if length == 0 || length > MAX_FRAME_LEN {
            self.connected = false;
            return Err("invalid frame length".into());
        }
        let mut payload = vec![0u8; length];
        self.stream.read_exact(&mut payload).await?;
        mymouse_protocol::decode(&payload).map_err(Into::into)
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
}
