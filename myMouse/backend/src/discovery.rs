use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use tokio::sync::mpsc;

const SERVICE_TYPE: &str = "_mymouse._tcp.local.";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub address: String,
    pub port: u16,
    /// Certificate fingerprint advertised in TXT records, for pairing.
    pub fingerprint: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidServiceName;

impl std::fmt::Display for InvalidServiceName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("invalid myMouse service name")
    }
}

impl std::error::Error for InvalidServiceName {}

pub fn parse_service_name(service: &str) -> Result<Peer, InvalidServiceName> {
    // mDNS fullnames look like `Name@device-id._mymouse._tcp.local.`. Older
    // builds used `service:Name@device-id`; accept both so existing peers stay
    // discoverable.
    let (left, right) = service.split_once('@').ok_or(InvalidServiceName)?;
    let name = left.rsplit_once(':').map(|(_, name)| name).unwrap_or(left);
    let id = right
        .split(['.', ':'])
        .next()
        .filter(|id| !id.is_empty())
        .ok_or(InvalidServiceName)?;
    if name.is_empty() {
        return Err(InvalidServiceName);
    }
    Ok(Peer {
        id: id.to_string(),
        name: name.to_string(),
        address: String::new(),
        port: 0,
        fingerprint: String::new(),
    })
}

/// Pick the local IPv4 address used for the default LAN route. `UdpSocket::connect`
/// only selects a route; it does not send a packet.
fn local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let address = socket.local_addr().ok()?.ip();
    match address {
        std::net::IpAddr::V4(ip) if !ip.is_unspecified() && !ip.is_loopback() => Some(ip.to_string()),
        _ => None,
    }
}

pub struct Discovery {
    local_id: String,
    local_name: String,
}

impl Discovery {
    pub fn new(local_id: &str, local_name: &str) -> Self {
        Self {
            local_id: local_id.to_string(),
            local_name: local_name.to_string(),
        }
    }

    /// Advertise this install's real listener port and public fingerprint so
    /// discovered peers can pair and connect without copying values by hand.
    pub fn start(&self, port: u16, fingerprint: &str) -> Result<mpsc::Receiver<Peer>, mdns_sd::Error> {
        let (tx, rx) = mpsc::channel(32);
        let local_id = self.local_id.clone();
        let local_name = self.local_name.clone();
        let fingerprint = fingerprint.to_string();

        std::thread::spawn(move || {
            let daemon = match ServiceDaemon::new() {
                Ok(daemon) => daemon,
                Err(_) => return,
            };
            let instance = format!("{}@{}", local_name, local_id);
            let host = format!("{}.local.", local_id);
            let mut properties = HashMap::new();
            properties.insert("port".to_string(), port.to_string());
            properties.insert("fp".to_string(), fingerprint);
            let address = local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
            let info = ServiceInfo::new(
                SERVICE_TYPE,
                &instance,
                &host,
                &address,
                port,
                Some(properties),
            );
            if let Ok(info) = info {
                let _ = daemon.register(info);
            }
            let events = match daemon.browse(SERVICE_TYPE) {
                Ok(events) => events,
                Err(_) => return,
            };
            while let Ok(event) = events.recv() {
                if let ServiceEvent::ServiceResolved(info) = event {
                    if let Ok(mut peer) = parse_service_name(&info.get_fullname()) {
                        if peer.id != local_id {
                            peer.address = info
                                .get_addresses()
                                .iter()
                                .next()
                                .map(|addr| addr.to_string())
                                .unwrap_or_default();
                            peer.port = info
                                .get_property_val_str("port")
                                .and_then(|value| value.parse().ok())
                                .unwrap_or_else(|| info.get_port());
                            peer.fingerprint = info
                                .get_property_val_str("fp")
                                .unwrap_or_default()
                                .to_string();
                            let _ = tx.blocking_send(peer);
                        }
                    }
                }
            }
        });

        Ok(rx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_peer_from_service_name() {
        let peer = parse_service_name("mymouse._tcp.local:PC-A@deadbeef").unwrap();
        assert_eq!(peer.id, "deadbeef");
        assert_eq!(peer.name, "PC-A");
    }

    #[test]
    fn parses_peer_from_mdns_fullname() {
        let peer = parse_service_name("PC-A@deadbeef._mymouse._tcp.local.").unwrap();
        assert_eq!(peer.id, "deadbeef");
        assert_eq!(peer.name, "PC-A");
    }

    #[test]
    fn ignores_malformed_service_name() {
        assert!(parse_service_name("garbage").is_err());
        assert!(parse_service_name("PC-A@._mymouse._tcp.local.").is_err());
    }
}
