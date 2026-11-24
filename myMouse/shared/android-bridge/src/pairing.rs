use rand::Rng;
use std::time::{Duration, Instant};

const CODE_TTL: Duration = Duration::from_secs(300);

/// Generate a 6-digit pairing code.
pub fn generate_pairing_code() -> String {
    let value: u32 = rand::rng().random_range(0..1_000_000);
    format!("{value:06}")
}

/// Build the QR/deep-link URI for a pairing session.
pub fn pairing_uri(code: &str, peer_id: &str) -> String {
    format!("mymouse://pair?code={code}&id={peer_id}")
}

/// Parse a pairing URI of the form `mymouse://pair?code=XXXXXX&id=...`.
pub fn parse_pairing_uri(uri: &str) -> Option<ParsedPairingUri> {
    let rest = uri.strip_prefix("mymouse://pair?")?;
    let mut code = None;
    let mut id = None;
    for part in rest.split('&') {
        let (k, v) = part.split_once('=')?;
        match k {
            "code" => code = Some(v.to_string()),
            "id" => id = Some(v.to_string()),
            _ => {}
        }
    }
    Some(ParsedPairingUri {
        code: code?,
        peer_id: id?,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedPairingUri {
    pub code: String,
    pub peer_id: String,
}

/// In-memory pairing session state. The Android app keeps one active session
/// while displaying/scanning a code. Persistence of trust is handled separately.
#[derive(Debug)]
pub struct PairingSession {
    code: String,
    peer_id: String,
    peer_name: String,
    expires_at: Instant,
    approved: bool,
}

impl PairingSession {
    pub fn new(peer_id: String, peer_name: String) -> Self {
        Self {
            code: generate_pairing_code(),
            peer_id,
            peer_name,
            expires_at: Instant::now() + CODE_TTL,
            approved: false,
        }
    }

    pub fn code(&self) -> String {
        self.code.clone()
    }

    pub fn peer_id(&self) -> String {
        self.peer_id.clone()
    }

    pub fn peer_name(&self) -> String {
        self.peer_name.clone()
    }

    pub fn uri(&self) -> String {
        pairing_uri(&self.code, &self.peer_id)
    }

    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    pub fn matches(&self, code: &str) -> bool {
        !self.is_expired() && self.code == code
    }

    /// Approve only when the code matches and session is still valid.
    pub fn approve_with_code(&mut self, code: &str) -> bool {
        if self.matches(code) {
            self.approved = true;
            true
        } else {
            false
        }
    }

    pub fn approved(&self) -> bool {
        self.approved
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_six_digits() {
        let code = generate_pairing_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn uri_round_trip() {
        let uri = pairing_uri("123456", "dev-1");
        let parsed = parse_pairing_uri(&uri).unwrap();
        assert_eq!(parsed.code, "123456");
        assert_eq!(parsed.peer_id, "dev-1");
    }

    #[test]
    fn session_approves_matching_code() {
        let mut session = PairingSession::new("dev-1".into(), "Phone".into());
        let code = session.code();
        assert!(session.approve_with_code(&code));
        assert!(session.approved());
    }
}
