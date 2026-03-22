use std::fmt;

/// Lifecycle states for a simulation island.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandState {
    /// Island is loading its WASM module and initial state.
    Initializing,
    /// Island is actively ticking.
    Running,
    /// Island has been asked to drain (finish current tick, then stop).
    Draining,
    /// Island has fully stopped; thread has been joined.
    Stopped,
}

impl fmt::Display for IslandState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initializing => write!(f, "Initializing"),
            Self::Running => write!(f, "Running"),
            Self::Draining => write!(f, "Draining"),
            Self::Stopped => write!(f, "Stopped"),
        }
    }
}

/// Error returned when an invalid state transition is attempted.
#[derive(Debug, PartialEq, Eq)]
pub struct TransitionError {
    pub from: IslandState,
    pub to: IslandState,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid transition: {} -> {}", self.from, self.to)
    }
}

impl std::error::Error for TransitionError {}

impl IslandState {
    /// Attempt a state transition. Returns the new state or an error.
    ///
    /// Valid transitions:
    ///   Initializing -> Running
    ///   Running      -> Draining
    ///   Draining     -> Stopped
    pub fn transition(self, to: IslandState) -> Result<IslandState, TransitionError> {
        let valid = matches!(
            (self, to),
            (Self::Initializing, Self::Running)
                | (Self::Running, Self::Draining)
                | (Self::Draining, Self::Stopped)
        );
        if valid {
            Ok(to)
        } else {
            Err(TransitionError { from: self, to })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_initializing_to_running() {
        assert_eq!(
            IslandState::Initializing.transition(IslandState::Running),
            Ok(IslandState::Running)
        );
    }

    #[test]
    fn valid_running_to_draining() {
        assert_eq!(
            IslandState::Running.transition(IslandState::Draining),
            Ok(IslandState::Draining)
        );
    }

    #[test]
    fn valid_draining_to_stopped() {
        assert_eq!(
            IslandState::Draining.transition(IslandState::Stopped),
            Ok(IslandState::Stopped)
        );
    }

    // All 9 invalid transitions
    #[test]
    fn invalid_initializing_to_initializing() {
        assert!(IslandState::Initializing
            .transition(IslandState::Initializing)
            .is_err());
    }

    #[test]
    fn invalid_initializing_to_draining() {
        assert!(IslandState::Initializing
            .transition(IslandState::Draining)
            .is_err());
    }

    #[test]
    fn invalid_initializing_to_stopped() {
        assert!(IslandState::Initializing
            .transition(IslandState::Stopped)
            .is_err());
    }

    #[test]
    fn invalid_running_to_initializing() {
        assert!(IslandState::Running
            .transition(IslandState::Initializing)
            .is_err());
    }

    #[test]
    fn invalid_running_to_running() {
        assert!(IslandState::Running
            .transition(IslandState::Running)
            .is_err());
    }

    #[test]
    fn invalid_running_to_stopped() {
        assert!(IslandState::Running
            .transition(IslandState::Stopped)
            .is_err());
    }

    #[test]
    fn invalid_draining_to_initializing() {
        assert!(IslandState::Draining
            .transition(IslandState::Initializing)
            .is_err());
    }

    #[test]
    fn invalid_draining_to_running() {
        assert!(IslandState::Draining
            .transition(IslandState::Running)
            .is_err());
    }

    #[test]
    fn invalid_draining_to_draining() {
        assert!(IslandState::Draining
            .transition(IslandState::Draining)
            .is_err());
    }

    // Stopped is terminal — all transitions from it are invalid
    #[test]
    fn invalid_stopped_to_any() {
        for target in [
            IslandState::Initializing,
            IslandState::Running,
            IslandState::Draining,
            IslandState::Stopped,
        ] {
            assert!(IslandState::Stopped.transition(target).is_err());
        }
    }

    #[test]
    fn transition_error_display() {
        let err = TransitionError {
            from: IslandState::Running,
            to: IslandState::Initializing,
        };
        assert_eq!(err.to_string(), "invalid transition: Running -> Initializing");
    }
}
