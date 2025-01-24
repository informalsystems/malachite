use std::time::Duration;

use malachitebft_core_types::TimeoutKind;

/// Timeouts
#[derive(Copy, Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Timeouts {
    /// How long we wait for a proposal block before prevoting nil
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub propose: Duration,

    /// How much timeout_propose increases with each round
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub propose_delta: Duration,

    /// How long we wait after receiving +2/3 prevotes for “anything” (ie. not a single block or nil)
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub prevote: Duration,

    /// How much the timeout_prevote increases with each round
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub prevote_delta: Duration,

    /// How long we wait after receiving +2/3 precommits for “anything” (ie. not a single block or nil)
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub precommit: Duration,

    /// How much the timeout_precommit increases with each round
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub precommit_delta: Duration,

    /// How long we wait after committing a block, before starting on the new
    /// height (this gives us a chance to receive some more precommits, even
    /// though we already have +2/3).
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub commit: Duration,

    /// How long we stay in preovte or precommit steps before starting
    /// the vote synchronization protocol.
    #[cfg_attr(feature = "serde", serde(with = "humantime_serde"))]
    pub step: Duration,
}

impl Timeouts {
    pub fn timeout_duration(&self, kind: TimeoutKind) -> Duration {
        match kind {
            TimeoutKind::Propose => self.propose,
            TimeoutKind::Prevote => self.prevote,
            TimeoutKind::Precommit => self.precommit,
            TimeoutKind::Commit => self.commit,
            TimeoutKind::PrevoteTimeLimit => self.step,
            TimeoutKind::PrecommitTimeLimit => self.step,
        }
    }

    pub fn delta_duration(&self, step: TimeoutKind) -> Option<Duration> {
        match step {
            TimeoutKind::Propose => Some(self.propose_delta),
            TimeoutKind::Prevote => Some(self.prevote_delta),
            TimeoutKind::Precommit => Some(self.precommit_delta),
            TimeoutKind::Commit => None,
            TimeoutKind::PrevoteTimeLimit => None,
            TimeoutKind::PrecommitTimeLimit => None,
        }
    }
}

impl Default for Timeouts {
    fn default() -> Self {
        Self {
            propose: Duration::from_secs(3),
            propose_delta: Duration::from_millis(500),
            prevote: Duration::from_secs(1),
            prevote_delta: Duration::from_millis(500),
            precommit: Duration::from_secs(1),
            precommit_delta: Duration::from_millis(500),
            commit: Duration::from_secs(0),
            step: Duration::from_secs(30),
        }
    }
}
