use mdns_sd::{ServiceDaemon, ServiceEvent, ServiceInfo};
use std::collections::HashMap;
use tokio::sync::mpsc;

const SERVICE_TYPE: &str = "_mymouse._tcp.local.";
const SERVICE_PORT: u16 = 45552;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Peer {
    pub id: String,
    pub name: String,
    pub address: String,
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
    let (left, id) = service.split_once('@').ok_or(InvalidServiceName)?;
    let (_, name) = left.rsplit_once(':').ok_or(InvalidServiceName)?;
    if id.is_empty() || name.is_empty() {
        return Err(InvalidServiceName);
    }
    Ok(Peer {
        id: id.to_string(),
        name: name.to_string(),
        address: String::new(),
    })
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

    pub fn start(&self) -> Result<mpsc::Receiver<Peer>, mdns_sd::Error> {
        let (tx, rx) = mpsc::channel(32);
        let local_id = self.local_id.clone();
        let local_name = self.local_name.clone();

        std::thread::spawn(move || {
            let daemon = match ServiceDaemon::new() {
                Ok(daemon) => daemon,
                Err(_) => return,
            };
            let instance = format!("{}@{}", local_name, local_id);
            let host = format!("{}.local.", local_id);
            let info = ServiceInfo::new(
                SERVICE_TYPE,
                &instance,
                &host,
                "0.0.0.0",
                SERVICE_PORT,
                None::<HashMap<String, String>>,
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
    fn ignores_malformed_service_name() {
        assert!(parse_service_name("garbage").is_err());
    }
}
