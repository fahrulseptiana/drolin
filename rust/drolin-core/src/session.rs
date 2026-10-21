use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// The lifecycle of one guest process.
///
/// The type is deliberately closed: a caller cannot mark a session running
/// before it started, or restart one that is still running. Platform adapters
/// keep a `SessionState` and ask it to transition, rather than keeping a
/// boolean and a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SessionState {
    Idle,
    Starting,
    Running,
    Stopped { exit_code: Option<i32> },
    Failed { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum TransitionError {
    #[error("cannot {action} while the guest is {current}")]
    Invalid {
        action: &'static str,
        current: &'static str,
    },
}

impl SessionState {
    pub fn start(self) -> Result<Self, TransitionError> {
        match self {
            Self::Idle | Self::Stopped { .. } | Self::Failed { .. } => Ok(Self::Starting),
            other => Err(TransitionError::Invalid {
                action: "start",
                current: other.name(),
            }),
        }
    }

    pub fn mark_running(self) -> Result<Self, TransitionError> {
        match self {
            Self::Starting => Ok(Self::Running),
            other => Err(TransitionError::Invalid {
                action: "mark running",
                current: other.name(),
            }),
        }
    }

    pub fn stop(self, exit_code: Option<i32>) -> Result<Self, TransitionError> {
        match self {
            Self::Starting | Self::Running => Ok(Self::Stopped { exit_code }),
            other => Err(TransitionError::Invalid {
                action: "stop",
                current: other.name(),
            }),
        }
    }

    pub fn fail(self, reason: impl Into<String>) -> Result<Self, TransitionError> {
        match self {
            Self::Starting | Self::Running => Ok(Self::Failed {
                reason: reason.into(),
            }),
            other => Err(TransitionError::Invalid {
                action: "fail",
                current: other.name(),
            }),
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Stopped { .. } | Self::Failed { .. })
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Starting => "starting",
            Self::Running => "running",
            Self::Stopped { .. } => "stopped",
            Self::Failed { .. } => "failed",
        }
    }
}

impl fmt::Display for SessionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stopped { exit_code } => match exit_code {
                Some(code) => write!(f, "stopped (exit {code})"),
                None => f.write_str("stopped"),
            },
            Self::Failed { reason } => write!(f, "failed: {reason}"),
            other => f.write_str(other.name()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let state = SessionState::Idle.start().unwrap();
        let state = state.mark_running().unwrap();
        let state = state.stop(Some(0)).unwrap();
        assert_eq!(state, SessionState::Stopped { exit_code: Some(0) });
        assert!(state.is_terminal());
    }

    #[test]
    fn stopped_sessions_can_be_restarted() {
        let state = SessionState::Stopped { exit_code: Some(1) };
        assert_eq!(state.start().unwrap(), SessionState::Starting);
    }

    #[test]
    fn running_sessions_cannot_start_again() {
        let err = SessionState::Running.start().unwrap_err();
        assert_eq!(
            err,
            TransitionError::Invalid {
                action: "start",
                current: "running"
            }
        );
    }

    #[test]
    fn idle_sessions_cannot_stop() {
        assert!(SessionState::Idle.stop(Some(0)).is_err());
    }

    #[test]
    fn active_states_are_not_terminal() {
        assert!(SessionState::Starting.is_active());
        assert!(SessionState::Running.is_active());
        assert!(!SessionState::Idle.is_active());
    }
}
