use rand::Rng;
use std::time::{Duration, Instant};

const CODE_TTL: Duration = Duration::from_secs(300);

pub fn generate_code() -> String {
    let value: u32 = rand::rng().random_range(0..1_000_000);
    format!("{value:06}")
}

pub struct PairSession {
    code: String,
    peer_id: String,
    expires_at: Instant,
    approved: bool,
}

impl PairSession {
    pub fn new(peer_id: &str) -> Self {
        Self {
            code: generate_code(),
            peer_id: peer_id.to_string(),
            expires_at: Instant::now() + CODE_TTL,
            approved: false,
        }
    }

    pub fn pairing_uri(&self) -> String {
        format!("mymouse://pair?code={}&id={}", self.code, self.peer_id)
    }

    pub fn matches(&self, code: &str) -> bool {
        self.expires_at > Instant::now() && self.code == code
    }

    pub fn approve(&mut self) -> bool {
        if self.expires_at <= Instant::now() {
            return false;
        }
        self.approved = true;
        true
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
        let code = generate_code();
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| c.is_ascii_digit()));
    }
}
