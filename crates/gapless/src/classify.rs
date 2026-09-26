use std::fmt;

use tonic::{Code, Status};

/// Why a stream ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DisconnectReason {
    /// Terminated through Solami's account API (`DELETE /auth/connections/grpc/{id}`).
    Killed,
    /// Solami's send buffer filled because we read too slowly (or a replay was too deep).
    Backpressure,
    /// The account is at its concurrent-stream limit.
    StreamLimit,
    /// Too many reconnects from this IP (Solami's limit is 100 per 10 s).
    ReconnectLimit,
    /// Streaming bandwidth and balance ran out.
    BalanceExhausted,
    ServerShutdown,
    BackendUnavailable,
    /// The stream moved backends and couldn't be restored there.
    BackendMoved,
    SessionExpired,
    /// Clean end-of-stream with no status. Solami reports backpressure this way too, so the
    /// supervisor asks the connection history before settling on this.
    ServerClosed,
    /// Nothing arrived, not even a ping, within the stall timeout.
    Stalled,
    /// Dropped on purpose from our side.
    Cut,
    /// The HTTP/2 connection failed.
    Network,
    /// The subscription itself was refused (bad key, filter over limits).
    Rejected,
    /// The replay's `from_slot` had already left Solami's replay horizon: the window slides
    /// on while a subscription is on its way.
    OutOfHorizon,
    Other(String),
}

impl DisconnectReason {
    pub fn from_status(status: &Status) -> Self {
        let msg = status.message().to_ascii_lowercase();
        match status.code() {
            Code::Cancelled if msg.contains("terminated by user") => Self::Killed,
            Code::ResourceExhausted if msg.contains("backpressure") => Self::Backpressure,
            Code::ResourceExhausted if msg.contains("concurrent streams") => Self::StreamLimit,
            Code::ResourceExhausted if msg.contains("reconnections") => Self::ReconnectLimit,
            Code::ResourceExhausted if msg.contains("balance") || msg.contains("bandwidth") => {
                Self::BalanceExhausted
            }
            Code::Unavailable if msg.contains("shutting down") => Self::ServerShutdown,
            Code::Unavailable if msg.contains("backends unavailable") => Self::BackendUnavailable,
            Code::Unavailable if msg.contains("replacement backend") => Self::BackendMoved,
            Code::Unavailable if msg.contains("session expired") => Self::SessionExpired,
            Code::PermissionDenied | Code::InvalidArgument | Code::Unauthenticated => {
                Self::Rejected
            }
            // "broadcast from 450649411 is not available, last available: 450649417"
            Code::OutOfRange => Self::OutOfHorizon,
            Code::Unavailable | Code::Unknown | Code::Internal | Code::Cancelled => Self::Network,
            code => Self::Other(format!("{code:?}")),
        }
    }

    /// Maps `termination_reason` from `/auth/connections/grpc/history`.
    pub fn from_history(reason: &str) -> Self {
        match reason.to_ascii_lowercase().as_str() {
            "backpressure" => Self::Backpressure,
            "user_kill" | "killed" | "kill" | "user_terminated" | "terminated_by_user" => {
                Self::Killed
            }
            "session_expired" => Self::SessionExpired,
            "shutdown" | "server_shutdown" => Self::ServerShutdown,
            other => Self::Other(other.to_owned()),
        }
    }

    /// Retrying can't help: the key or filter is wrong, or the account is out of funds.
    pub fn is_fatal(&self) -> bool {
        matches!(self, Self::Rejected | Self::BalanceExhausted)
    }

    /// A short machine-friendly label.
    pub fn label(&self) -> &str {
        match self {
            Self::Killed => "killed",
            Self::Backpressure => "backpressure",
            Self::StreamLimit => "stream_limit",
            Self::ReconnectLimit => "reconnect_limit",
            Self::BalanceExhausted => "balance_exhausted",
            Self::ServerShutdown => "server_shutdown",
            Self::BackendUnavailable => "backend_unavailable",
            Self::BackendMoved => "backend_moved",
            Self::SessionExpired => "session_expired",
            Self::ServerClosed => "server_closed",
            Self::Stalled => "stalled",
            Self::Cut => "cut",
            Self::Network => "network",
            Self::Rejected => "rejected",
            Self::OutOfHorizon => "out_of_horizon",
            Self::Other(s) => s,
        }
    }
}

impl fmt::Display for DisconnectReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            Self::Killed => "killed through Solami's account API",
            Self::Backpressure => "Solami closed the stream for backpressure",
            Self::StreamLimit => "concurrent stream limit reached",
            Self::ReconnectLimit => "too many reconnects from this IP",
            Self::BalanceExhausted => "streaming bandwidth and balance exhausted",
            Self::ServerShutdown => "server restarting",
            Self::BackendUnavailable => "no geyser backend available",
            Self::BackendMoved => "stream moved backends and couldn't be restored",
            Self::SessionExpired => "session expired",
            Self::ServerClosed => "server ended the stream",
            Self::Stalled => "stream stalled",
            Self::Cut => "cut by the client",
            Self::Network => "network error",
            Self::Rejected => "subscription rejected",
            Self::OutOfHorizon => "the replay start had left Solami's replay horizon",
            Self::Other(s) => return write!(f, "{s}"),
        };
        f.write_str(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_replay_start_past_the_horizon() {
        let status = Status::out_of_range(
            "broadcast from 450649411 is not available, last available: 450649417",
        );
        let reason = DisconnectReason::from_status(&status);
        assert_eq!(reason, DisconnectReason::OutOfHorizon);
        assert!(!reason.is_fatal());
    }

    #[test]
    fn kill_through_the_account_api() {
        let status = Status::cancelled("stream terminated by user");
        assert_eq!(
            DisconnectReason::from_status(&status),
            DisconnectReason::Killed
        );
    }

    #[test]
    fn documented_resource_exhausted_messages() {
        let cases = [
            (
                "stream backpressure: client too slow, please reconnect",
                DisconnectReason::Backpressure,
            ),
            (
                "max concurrent streams (2) reached for your tier",
                DisconnectReason::StreamLimit,
            ),
            (
                "too many reconnections from your ip; limit is 100 per 10 seconds",
                DisconnectReason::ReconnectLimit,
            ),
            (
                "insufficient balance, please top up",
                DisconnectReason::BalanceExhausted,
            ),
        ];
        for (msg, want) in cases {
            assert_eq!(
                DisconnectReason::from_status(&Status::resource_exhausted(msg)),
                want,
                "{msg}"
            );
        }
    }

    #[test]
    fn documented_unavailable_messages() {
        let cases = [
            (
                "server is shutting down, please reconnect to another instance",
                DisconnectReason::ServerShutdown,
            ),
            (
                "all geyser backends unavailable, please retry in a moment",
                DisconnectReason::BackendUnavailable,
            ),
            (
                "failed to restore subscription on replacement backend, please reconnect",
                DisconnectReason::BackendMoved,
            ),
            (
                "session expired, please reconnect",
                DisconnectReason::SessionExpired,
            ),
            (
                "error reading a body from connection",
                DisconnectReason::Network,
            ),
        ];
        for (msg, want) in cases {
            assert_eq!(
                DisconnectReason::from_status(&Status::unavailable(msg)),
                want,
                "{msg}"
            );
        }
    }

    #[test]
    fn rejections_are_fatal() {
        let status = Status::permission_denied(
            "unfiltered/firehose subscriptions are not allowed on non-PAYG streams",
        );
        let reason = DisconnectReason::from_status(&status);
        assert_eq!(reason, DisconnectReason::Rejected);
        assert!(reason.is_fatal());
        assert!(!DisconnectReason::Backpressure.is_fatal());
    }

    #[test]
    fn history_reasons() {
        assert_eq!(
            DisconnectReason::from_history("backpressure"),
            DisconnectReason::Backpressure
        );
        assert_eq!(
            DisconnectReason::from_history("user_kill"),
            DisconnectReason::Killed
        );
        assert_eq!(
            DisconnectReason::from_history("client_disconnect"),
            DisconnectReason::Other("client_disconnect".into())
        );
    }
}
