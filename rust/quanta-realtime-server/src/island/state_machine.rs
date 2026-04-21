use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IslandState {
    Initializing,
    Running,
    Draining,
    Stopped,
}

impl fmt::Display for IslandState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

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
    fn valid_transitions() {
        let cases = [
            (IslandState::Initializing, IslandState::Running),
            (IslandState::Running, IslandState::Draining),
            (IslandState::Draining, IslandState::Stopped),
        ];
        for (from, to) in cases {
            assert_eq!(from.transition(to), Ok(to), "{from} -> {to} should succeed");
        }
    }

    #[test]
    fn all_invalid_transitions_rejected() {
        use IslandState::*;
        let invalid = [
            (Initializing, Initializing),
            (Initializing, Draining),
            (Initializing, Stopped),
            (Running, Initializing),
            (Running, Running),
            (Running, Stopped),
            (Draining, Initializing),
            (Draining, Running),
            (Draining, Draining),
            (Stopped, Initializing),
            (Stopped, Running),
            (Stopped, Draining),
            (Stopped, Stopped),
        ];
        for (from, to) in invalid {
            assert!(
                from.transition(to).is_err(),
                "{from} -> {to} should be invalid"
            );
        }
    }

    #[test]
    fn transition_error_display() {
        let err = TransitionError {
            from: IslandState::Running,
            to: IslandState::Initializing,
        };
        assert_eq!(
            err.to_string(),
            "invalid transition: Running -> Initializing"
        );
    }
}
