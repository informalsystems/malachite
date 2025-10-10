//! FaB: Multiplexer for FaB-a-la-Tendermint-bounded-square
//!
//! The Multiplexer is responsible for multiplexing input data and returning appropriate events to the Round State Machine.
//!
//! Input sources:
//! - Proposals from the Driver
//! - Vote Keeper outputs (CertificateAny, CertificateValue, SkipRound)
//! - Step changes from the Round State Machine
//!
//! FaB Multiplexing Logic:
//!
//! | Step     | Vote Keeper Output     | Proposal Status   | Multiplexed Input to SM  | New Step  | FaB Line  | Notes                              |
//! |--------- |----------------------- | ----------------- | ------------------------ | --------- | --------- | ---------------------------------- |
//! | any      | CertificateValue(v)    | Valid Proposal(v) | CanDecide                | commit    | 72-74     | Decide v (no step restriction)     |
//! | prevote  | CertificateValue(v)    | No proposal       | EnoughPrevotesForRound   | prevote   | 69-70     | Schedule prevote timeout           |
//! | prevote  | CertificateAny         | *                 | EnoughPrevotesForRound   | prevote   | 69-70     | Schedule prevote timeout           |
//! | any      | SkipRound(r)           | *                 | SkipRound                | propose   | 95-96     | Skip to round r (no step restriction)|
//! | propose  | (no threshold)         | Valid Proposal    | Proposal                 | prevote   | 51-59     | Validate SafeProposal, then prevote|

use alloc::vec::Vec;

use malachitebft_core_state_machine::input::Input as RoundInput;
use malachitebft_core_state_machine::state::Step;
// FaB: Removed CommitCertificate and PolkaCertificate imports (3f+1 Tendermint concepts)
use malachitebft_core_types::{SignedProposal};
use malachitebft_core_types::{Context, Proposal, Round, Validator, Validity, ValidatorSet, Value, ValueId};
use malachitebft_core_votekeeper::keeper::Output as VKOutput;

use crate::Driver;

impl<Ctx> Driver<Ctx>
where
    Ctx: Context,
{
    // FaB: Process a received proposal for FaB-a-la-Tendermint-bounded-square
    ///
    /// In FaB, proposal processing:
    ///
    /// 1. Check if we have a valid proposal + 4f+1 prevotes for it → CanDecide
    /// 2. Otherwise, if it's a valid proposal for current round → validate SafeProposal
    ///    - If SafeProposal passes → SafeProposal input (prevote for new value)
    ///    - If SafeProposal fails → UnsafeProposal input (prevote for old value)
    /// 3. Invalid or out-of-round proposals → None (ignore)
    ///
    /// # Arguments
    /// * `proposal` - The proposal to process
    /// * `validity` - Whether the proposal passed application validity check
    /// * `certificate` - Optional certificate of prevotes (None during backwards compat transition)
    pub(crate) fn multiplex_proposal(
        &mut self,
        proposal: Ctx::Proposal,
        validity: Validity,
        certificate: Option<Vec<malachitebft_core_types::SignedVote<Ctx>>>,
    ) -> Option<RoundInput<Ctx>> {
        // FaB: Should only receive proposals for our height
        assert_eq!(self.height(), proposal.height());

        // FaB: Check that there is an ongoing round
        if self.round_state.round == Round::Nil {
            return None;
        }

        // FaB: Ignore invalid proposals
        if !validity.is_valid() {
            return None;
        }

        // FaB: Check if we can decide (FaB lines 72-74)
        // Condition: valid proposal + 4f+1 prevotes for same value → DECIDE
        if self.round_state.decision.is_none() {
            // FaB: Try to build a certificate for this value
            if let Some(certificate) = self
                .vote_keeper
                .build_certificate(proposal.round(), &proposal.value().id())
            {
                // FaB: We have 4f+1 prevotes for this value → CanDecide!
                return Some(RoundInput::CanDecide {
                    proposal,
                    certificate,
                });
            }
        }

        // FaB: If proposal is not for current round, ignore it
        if self.round_state.round != proposal.round() {
            return None;
        }

        // FaB: Valid proposal for current round → validate SafeProposal (FaB lines 51-67)
        // FaB: Driver validates SafeProposal and state machine reacts accordingly
        if self.validate_safe_proposal(&proposal, &certificate) {
            // FaB: SafeProposal validation passed
            Some(RoundInput::SafeProposal(proposal))
        } else {
            // FaB: SafeProposal validation FAILED
            Some(RoundInput::UnsafeProposal(proposal))
        }
    }

    pub(crate) fn store_and_multiplex_proposal(
        &mut self,
        signed_proposal: SignedProposal<Ctx>,
        validity: Validity,
        certificate: Option<Vec<malachitebft_core_types::SignedVote<Ctx>>>,
    ) -> Option<RoundInput<Ctx>> {
        // Should only receive proposals for our height.
        assert_eq!(self.height(), signed_proposal.height());

        let proposal = signed_proposal.message.clone();

        // Store the proposal, its validity, and certificate
        // FaB Phase 3A: Certificate is now cached for later reprocessing
        self.proposal_keeper
            .store_proposal(signed_proposal, validity, certificate.clone());

        // FaB: Phase 2 - pass certificate through for SafeProposal validation
        self.multiplex_proposal(proposal, validity, certificate)
    }

    // FaB: Removed store_and_multiplex_commit_certificate() - Tendermint 3f+1 concept
    // FaB: Removed store_and_multiplex_polka_certificate() - Tendermint 3f+1 concept
    // FaB: In FaB, certificates are 4f+1 prevotes built on-demand from vote_keeper
    // FaB: No need to store them separately

    // FaB: Process vote keeper outputs for FaB-a-la-Tendermint-bounded-square
    ///
    /// Vote keeper emits 3 types of outputs:
    /// - CertificateAny: 4f+1 prevotes total
    /// - CertificateValue(v): 4f+1 prevotes for value v (can decide if we have proposal)
    /// - SkipRound(r): f+1 prevotes from higher round r
    ///
    /// Step restrictions based on FaB algorithm:
    /// - CertificateAny/CertificateValue → EnoughPrevotesForRound: only at prevote step (line 69-70)
    /// - CertificateValue → CanDecide: any step (no restriction, line 72-74)
    /// - SkipRound: any step (no restriction, line 95-96)
    pub(crate) fn multiplex_vote_threshold(
        &mut self,
        new_threshold: VKOutput<ValueId<Ctx>>,
        threshold_round: Round,
    ) -> Option<(Round, RoundInput<Ctx>)> {
        match new_threshold {
            // FaB: 4f+1 prevotes for specific value v
            // FaB: Check if we have a matching proposal → CanDecide (lines 72-74)
            VKOutput::DecisionValue(ref value_id) => {
                // FaB: Check if we have a valid proposal for this value
                // FaB: CanDecide has no step restriction (line 72-74)
                if let Some((signed_proposal, validity, _certificate)) =
                    self.proposal_and_validity_for_round_and_value(threshold_round, value_id.clone())
                {
                    if validity.is_valid() {
                        // FaB: Build the certificate for this value
                        if let Some(certificate) =
                            self.vote_keeper.build_certificate(threshold_round, value_id)
                        {
                            // FaB: We can decide! (any step)
                            return Some((
                                threshold_round,
                                RoundInput::CanDecide {
                                    proposal: signed_proposal.message.clone(),
                                    certificate,
                                },
                            ));
                        }
                    }
                }

                // FaB: No valid proposal yet, but we have certificate
                // FaB: Only signal EnoughPrevotesForRound if at prevote step (line 69-70)
                if threshold_round == self.round() && self.round_state.step == Step::Prevote {
                    Some((threshold_round, RoundInput::EnoughPrevotesForRound))
                } else {
                    None
                }
            }

            // FaB: 4f+1 prevotes for any values (possibly distributed)
            // FaB: Signal state machine that we have enough prevotes for this round (line 69-70)
            // FaB: Only at prevote step
            VKOutput::CertificateAny => {
                // FaB: Special case - proposer at Prepropose step waiting for 4f+1 prevotes from prev round
                // FaB: Lines 39-49: proposer needs certificate from rounds >= previous round to propose
                let prev_round = Round::new(self.round().as_i64().saturating_sub(1) as u32);
                if self.round_state.step == Step::Prepropose && threshold_round >= prev_round {
                    // FaB: Proposer has 4f+1 prevotes from rounds >= previous round
                    // FaB: Build certificate from rounds >= prev_round (Lines 39, 45: "r >= round_p-1")
                    if let Some((certificate, cert_round)) = self.vote_keeper.build_certificate_from_rounds_gte(prev_round) {
                        let current_round = self.round();

                        // FaB: Check if certificate contains 2f+1 lock on any value
                        if let Some(value_id) = self.vote_keeper.find_lock_in_certificate(&certificate) {
                            // FaB: Has 2f+1 lock on value_id → LeaderProposeWithLock (line 40-43)
                            // FaB: Need to find the locked value from cached proposals
                            // FaB: Search in all rounds from prev_round to cert_round
                            for search_round in prev_round.as_i64()..=cert_round.as_i64() {
                                if let Some((signed_proposal, validity, _certificate)) =
                                    self.proposal_and_validity_for_round_and_value(Round::new(search_round as u32), value_id.clone())
                                {
                                    if validity.is_valid() {
                                        // FaB: Found the locked value → propose with lock
                                        return Some((
                                            current_round,
                                            RoundInput::LeaderProposeWithLock {
                                                value: signed_proposal.message.value().clone(),
                                                certificate,
                                                certificate_round: Round::new(search_round as u32),
                                            },
                                        ));
                                    }
                                }
                            }

                            // FaB: TODO: If we don't have the locked value cached, we should request it
                            // FaB: For now, fall through to no-lock case (safety violation risk!)
                            // warn!("Proposer has 2f+1 lock on value_id {:?} but doesn't have the value cached!", value_id);
                        }

                        // FaB: No 2f+1 lock (or couldn't find locked value) → LeaderProposeWithoutLock (line 45-49)
                        // FaB: Pass value=None to trigger GetValue request
                        return Some((
                            current_round,
                            RoundInput::LeaderProposeWithoutLock {
                                value: None,
                                certificate,
                            },
                        ));
                    }
                }

                // FaB: Check if we're at prevote step (line 69-70 condition)
                if threshold_round == self.round() && self.round_state.step == Step::Prevote {
                    Some((threshold_round, RoundInput::EnoughPrevotesForRound))
                } else {
                    None
                }
            }

            // FaB: f+1 prevotes from higher round → skip to that round (lines 95-96)
            // FaB: No step restriction
            VKOutput::MaxRoundPlus(new_round) => {
                // FaB Lines 92-96: Build f+1 certificate (not 4f+1!) justifying the skip
                // max_round+ requires f+1 voting power, NOT 4f+1
                if let Some(certificate) = self.vote_keeper.build_skip_round_certificate(new_round) {
                    Some((
                        new_round,
                        RoundInput::SkipRound {
                            round: new_round,
                            certificate,
                        },
                    ))
                } else {
                    // FaB: Can't build certificate, shouldn't happen
                    None
                }
            }
        }
    }

    // FaB: After a step change, check for cached proposals AND vote thresholds that need processing
    /// FaB: Simplified for FaB-a-la-Tendermint-bounded-square
    ///
    /// When entering Prepropose step, proactively check if we already have 4f+1 prevotes from previous round
    pub(crate) fn multiplex_step_change(&mut self, round: Round) -> Vec<(Round, RoundInput<Ctx>)> {
        let mut result = Vec::new();

        let step = self.round_state().step;

        // FaB: Special case - proposer entering Prepropose step (round > 0)
        // FaB: Proactively check if we already have 4f+1 prevotes from rounds >= previous round
        if step == Step::Prepropose && round > Round::ZERO {
            let prev_round = Round::new(round.as_i64().saturating_sub(1) as u32);

            // FaB Lines 39, 45: Check if we have 4f+1 prevotes from rounds >= prev_round
            if let Some((certificate, cert_round)) = self.vote_keeper.build_certificate_from_rounds_gte(prev_round) {
                // Check if certificate contains 2f+1 lock on any value
                if let Some(value_id) = self.vote_keeper.find_lock_in_certificate(&certificate) {
                    // FaB: Has 2f+1 lock on value_id → LeaderProposeWithLock (line 40-43)
                    // FaB: Need to find the locked value from cached proposals
                    // FaB: Search in all rounds from prev_round to cert_round
                    for search_round in prev_round.as_i64()..=cert_round.as_i64() {
                        if let Some((signed_proposal, validity, _certificate)) =
                            self.proposal_and_validity_for_round_and_value(Round::new(search_round as u32), value_id.clone())
                        {
                            if validity.is_valid() {
                                // FaB: Found the locked value → propose with lock
                                result.push((
                                    round,
                                    RoundInput::LeaderProposeWithLock {
                                        value: signed_proposal.message.value().clone(),
                                        certificate,
                                        certificate_round: Round::new(search_round as u32),
                                    },
                                ));
                                return result; // Exit early, we have the input
                            }
                        }
                    }
                }

                // FaB: No 2f+1 lock (or couldn't find locked value) → LeaderProposeWithoutLock
                // FaB: Pass value=None to trigger GetValue request
                result.push((
                    round,
                    RoundInput::LeaderProposeWithoutLock {
                        value: None,
                        certificate,
                    },
                ));
                return result; // Exit early, we have the input
            }
        }

        // FaB: Check if we have cached proposals for this round that need reprocessing
        let proposals = self.proposals_and_validities_for_round(round).to_vec();

        for (signed_proposal, validity, certificate) in proposals {
            let proposal = &signed_proposal.message;

            // FaB: At propose step, reprocess proposals (line 51-59)
            if step == Step::Propose {
                // FaB Phase 3A: Extract cached certificate for reprocessing
                // This preserves SafeProposal validation for round > 0 proposals
                if let Some(input) = self.multiplex_proposal(proposal.clone(), validity, certificate) {
                    result.push((self.round(), input))
                }
            }

            // FaB: No other step-specific processing needed
        }

        result
    }

    /// Validates SafeProposal predicate from FaB pseudocode lines 61-67.
    ///
    /// SafeProposal checks if a proposal is safe to prevote for based on its certificate S.
    ///
    /// Returns true if any of these conditions hold:
    /// 1. Certificate is None AND proposal is for round 0 (idiomatic Rust for "no certificate")
    /// 2. Certificate contains 2f+1 prevotes for value v'' from rounds >= round_p-1,
    ///    AND proposed value v matches v''
    /// 3. Certificate contains 4f+1 prevotes (any values) from rounds >= round_p-1
    ///
    /// # Arguments
    /// * `proposal` - The proposal to validate
    /// * `certificate` - Certificate of prevotes (None for round 0, Some(votes) for round > 0)
    ///
    /// # Returns
    /// `true` if SafeProposal passes, `false` otherwise
    pub(crate) fn validate_safe_proposal(
        &self,
        proposal: &Ctx::Proposal,
        certificate: &Option<Vec<malachitebft_core_types::SignedVote<Ctx>>>,
    ) -> bool {
        use malachitebft_core_types::{SignedVote, Vote};

        // FaB pseudocode line 64: Case 2b - "S == {} AND r == 0"
        // Round 0 must have None (idiomatic Rust for "no certificate needed")
        if proposal.round() == Round::ZERO {
            return certificate.is_none();
        }

        // Round > 0 must have a non-empty certificate
        let Some(cert) = certificate else {
            // FaB: Round > 0 with no certificate is invalid
            return false;
        };

        if cert.is_empty() {
            // FaB: Round > 0 with empty certificate is invalid
            return false;
        }

        let round_p = self.round();
        let min_round = Round::new(round_p.as_i64().saturating_sub(1).max(0) as u32);

        // FaB pseudocode lines 62-63: Case 1 - Check for 2f+1 lock on any value v''
        // "if ∃ v'', P ⊆ S. |P| >= 2f+1 AND v''!=nil AND
        //     (∀m ∈ P. m=<PREVOTE, h_p, r', id(v'')> AND r'>=round_p-1)"
        if let Some(locked_value_id) = self.vote_keeper.find_lock_in_certificate(cert) {
            // Check all votes in certificate are from recent rounds (r' >= round_p - 1)
            let all_recent = cert.iter().all(|vote| {
                vote.round() >= min_round && vote.height() == proposal.height()
            });

            if !all_recent {
                // FaB: Case 1 FAILED - certificate contains votes from old rounds
                return false;
            }

            // Check locked value matches proposed value
            // "return id(v'') == id(v) AND Valid(v)"
            if locked_value_id != proposal.value().id() {
                // FaB: Case 1 FAILED - proposal value doesn't match 2f+1 lock
                return false;
            }

            // FaB: Case 1 PASSED - 2f+1 lock on matching value - VALID
            return true;
        }

        // FaB pseudocode line 64: Case 2a - Check for 4f+1 prevotes from recent rounds
        // "|S| == 4f+1 AND (∀m ∈ S. m=<PREVOTE, h_p, r', *> AND r'>=round_p-1)"
        let weight: u64 = cert
            .iter()
            .filter_map(|vote| self.validator_set().get_by_address(vote.validator_address()))
            .map(|v| v.voting_power())
            .sum();
        let is_4f_plus_1 = self
            .threshold_params
            .certificate_quorum
            .is_met(weight, self.validator_set().total_voting_power());

        if is_4f_plus_1 {
            // All votes must be from rounds >= round_p - 1
            let all_recent = cert.iter().all(|vote| {
                vote.round() >= min_round && vote.height() == proposal.height()
            });

            if !all_recent {
                // FaB: Case 2a FAILED - certificate has 4f+1 but contains votes from old rounds
                return false;
            }

            // FaB: Case 2a PASSED - 4f+1 prevotes from recent rounds - VALID
            return true;
        }

        // FaB pseudocode line 66-67: Case 3 - Invalid certificate
        // "else return FALSE"
        // FaB: Case 3 - Invalid certificate (no 2f+1 lock, not 4f+1 recent prevotes)
        false
    }
}

// FaB: Removed all Tendermint threshold helper functions:
// - find_non_value_threshold() - not needed, vote_keeper emits outputs automatically
// - has_polka_value() - not needed in FaB
// - has_polka_nil() - not needed in FaB
// - has_polka_any() - not needed in FaB
// - has_precommit_any() - no precommits in FaB
//
// FaB: In FaB, the vote_keeper automatically emits CertificateAny, CertificateValue, or SkipRound
// when thresholds are reached, so manual threshold checking is not needed.
