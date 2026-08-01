// SPDX-License-Identifier: BSD-3-Clause
// Pure authorization state for Recovery's local provisioning session.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AuthFailure {
    Unauthorized,
    Replay,
    Expired,
}

pub struct RecoverySession {
    token: String,
    expires_at_ms: u64,
    last_nonce: u64,
    active: bool,
}

impl RecoverySession {
    pub fn new(token: String, now_ms: u64, ttl_ms: u64) -> Self {
        Self {
            token,
            expires_at_ms: now_ms.saturating_add(ttl_ms),
            last_nonce: 0,
            active: true,
        }
    }

    pub fn authorize(
        &mut self,
        supplied_token: Option<&str>,
        supplied_nonce: Option<&str>,
        now_ms: u64,
    ) -> Result<(), AuthFailure> {
        if !self.active || now_ms >= self.expires_at_ms {
            self.active = false;
            self.token.clear();
            return Err(AuthFailure::Expired);
        }

        let token = supplied_token.ok_or(AuthFailure::Unauthorized)?;
        if !constant_time_eq(self.token.as_bytes(), token.as_bytes()) {
            return Err(AuthFailure::Unauthorized);
        }

        let nonce = supplied_nonce
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|value| *value > 0)
            .ok_or(AuthFailure::Unauthorized)?;
        if nonce <= self.last_nonce {
            return Err(AuthFailure::Replay);
        }

        self.last_nonce = nonce;
        Ok(())
    }

    pub fn expire(&mut self) {
        self.active = false;
        self.token.clear();
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn next_nonce(&self) -> u64 {
        self.last_nonce.saturating_add(1)
    }
}

fn constant_time_eq(expected: &[u8], supplied: &[u8]) -> bool {
    let mut difference = expected.len() ^ supplied.len();
    for (index, expected_byte) in expected.iter().enumerate() {
        difference |= usize::from(*expected_byte ^ supplied.get(index).copied().unwrap_or(0));
    }
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOKEN: &str = "6f6e652d74696d652d7265636f766572792d73657373696f6e";

    #[test]
    fn missing_and_invalid_authorization_are_rejected() {
        let mut session = RecoverySession::new(TOKEN.to_string(), 1_000, 60_000);
        assert_eq!(
            session.authorize(None, Some("1"), 1_001),
            Err(AuthFailure::Unauthorized)
        );
        assert_eq!(
            session.authorize(Some("wrong"), Some("1"), 1_001),
            Err(AuthFailure::Unauthorized)
        );
        assert_eq!(
            session.authorize(Some(TOKEN), None, 1_001),
            Err(AuthFailure::Unauthorized)
        );
        assert_eq!(
            session.authorize(Some(TOKEN), Some("not-a-number"), 1_001),
            Err(AuthFailure::Unauthorized)
        );
    }

    #[test]
    fn consumed_nonce_cannot_be_replayed() {
        let mut session = RecoverySession::new(TOKEN.to_string(), 1_000, 60_000);
        assert_eq!(session.authorize(Some(TOKEN), Some("7"), 1_001), Ok(()));
        assert_eq!(session.next_nonce(), 8);
        assert_eq!(
            session.authorize(Some(TOKEN), Some("7"), 1_002),
            Err(AuthFailure::Replay)
        );
        assert_eq!(
            session.authorize(Some(TOKEN), Some("6"), 1_002),
            Err(AuthFailure::Replay)
        );
        assert_eq!(session.authorize(Some(TOKEN), Some("9"), 1_002), Ok(()));
    }

    #[test]
    fn expired_and_explicitly_closed_sessions_reject_every_token() {
        let mut expired = RecoverySession::new(TOKEN.to_string(), 1_000, 500);
        assert_eq!(
            expired.authorize(Some(TOKEN), Some("1"), 1_500),
            Err(AuthFailure::Expired)
        );
        assert_eq!(expired.token(), "");

        let mut closed = RecoverySession::new(TOKEN.to_string(), 1_000, 500);
        closed.expire();
        assert_eq!(closed.token(), "");
        assert_eq!(
            closed.authorize(Some(TOKEN), Some("1"), 1_001),
            Err(AuthFailure::Expired)
        );
    }
}
