use crate::trust::is_valid_id;
use rand::Rng;
use std::time::{Duration, Instant};

const CODE_TTL: Duration = Duration::from_secs(300);

pub fn generate_code() -> String {
    let value: u32 = rand::rng().random_range(0..1_000_000);
    format!("{value:06}")
}

/// A pairing code is bound to one joining device before it can be approved.
/// Approval records explicit user consent; consuming the session records that
/// the bound peer actually presented the code and received its acknowledgement.
pub struct PairSession {
    code: String,
    /// Local device ID advertised in the pairing URI.
    peer_id: String,
    expected_peer_id: Option<String>,
    expected_fingerprint: Option<String>,
    expires_at: Instant,
    approved: bool,
    consumed: bool,
}

impl PairSession {
    pub fn new(peer_id: &str) -> Self {
        Self {
            code: generate_code(),
            peer_id: peer_id.to_string(),
            expected_peer_id: None,
            expected_fingerprint: None,
            expires_at: Instant::now() + CODE_TTL,
            approved: false,
            consumed: false,
        }
    }

    pub fn pairing_uri(&self) -> String {
        format!("mymouse://pair?code={}&id={}", self.code, self.peer_id)
    }

    /// The code remains matchable after approval so the approved peer can
    /// present it and receive `PairApprove`. It stops matching only when the
    /// one-time session is consumed or expires.
    pub fn matches(&self, code: &str) -> bool {
        !self.consumed && self.expires_at > Instant::now() && self.code == code
    }

    /// Bind the session to the one device that may use its code. Rebinding to
    /// a different identity is refused so a UI race cannot redirect approval.
    pub fn bind_peer(&mut self, peer_device_id: &str, fingerprint: &str) -> bool {
        if self.consumed || self.approved || self.expires_at <= Instant::now() {
            return false;
        }
        if !is_valid_id(peer_device_id) || !crate::is_hex_fingerprint(fingerprint) {
            return false;
        }
        let fingerprint = fingerprint.to_ascii_lowercase();
        if self
            .expected_peer_id
            .as_deref()
            .is_some_and(|bound| bound != peer_device_id)
        {
            return false;
        }
        if self
            .expected_fingerprint
            .as_deref()
            .is_some_and(|bound| !bound.eq_ignore_ascii_case(&fingerprint))
        {
            return false;
        }
        self.expected_peer_id = Some(peer_device_id.to_string());
        self.expected_fingerprint = Some(fingerprint);
        true
    }

    pub fn bound_peer_id(&self) -> Option<&str> {
        self.expected_peer_id.as_deref()
    }

    pub fn bound_fingerprint(&self) -> Option<&str> {
        self.expected_fingerprint.as_deref()
    }

    pub fn is_bound_to(&self, peer_device_id: &str, fingerprint: &str) -> bool {
        self.expected_peer_id.as_deref() == Some(peer_device_id)
            && self
                .expected_fingerprint
                .as_deref()
                .is_some_and(|bound| bound.eq_ignore_ascii_case(fingerprint))
    }

    pub fn approve(&mut self) -> bool {
        if self.consumed
            || self.approved
            || self.expires_at <= Instant::now()
            || self.expected_peer_id.is_none()
            || self.expected_fingerprint.is_none()
        {
            return false;
        }
        self.approved = true;
        true
    }

    pub fn consume(&mut self) -> bool {
        if self.consumed || !self.approved || self.expires_at <= Instant::now() {
            return false;
        }
        self.consumed = true;
        true
    }

    pub fn approved(&self) -> bool {
        self.approved
    }

    pub fn code(&self) -> &str {
        &self.code
    }

    pub fn peer_id(&self) -> &str {
        &self.peer_id
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fingerprint() -> String {
        "a".repeat(64)
    }

    #[test]
    fn code_is_six_digits() {
        let code = generate_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn bind_peer_accepts_only_one_identity() {
        let mut session = PairSession::new("pc-a");
        let fp = fingerprint();
        assert!(session.bind_peer("pc-b", &fp));
        assert_eq!(session.bound_peer_id(), Some("pc-b"));
        assert_eq!(session.bound_fingerprint(), Some(fp.as_str()));
        assert!(session.is_bound_to("pc-b", &fp.to_ascii_uppercase()));
        assert!(
            !session.bind_peer("pc-c", &fp),
            "a session must not be redirected to another peer"
        );
        assert!(
            !session.bind_peer("pc-b", &"b".repeat(64)),
            "a session must not be redirected to another certificate"
        );
        assert!(session.bind_peer("pc-b", &fp), "rebinding to the same peer is idempotent");
    }

    #[test]
    fn session_requires_exact_code_and_is_single_use() {
        let mut session = PairSession::new("pc-b");
        assert!(session.bind_peer("pc-c", &fingerprint()));
        let code = session.code.clone();
        let wrong = format!("{:06}", (code.parse::<u32>().unwrap() + 1) % 1_000_000);
        assert!(session.matches(&code));
        assert!(!session.matches(&wrong), "unrelated code must be rejected");
        assert!(session.approve());
        assert!(session.approved());
        assert!(
            session.matches(&code),
            "an approved session still accepts its code once for acknowledgement"
        );
        assert!(session.consume());
        assert!(!session.matches(&code), "a consumed session must not match again");
        assert!(!session.consume(), "a pairing session cannot be acknowledged twice");
        assert!(!session.approve(), "a consumed session cannot be approved again");
    }

    #[test]
    fn approval_requires_a_bound_peer() {
        let mut session = PairSession::new("pc-a");
        assert!(!session.approve(), "an unbound session must not be approvable");
        assert!(session.bind_peer("pc-b", &fingerprint()));
        assert!(session.approve());
    }
}
