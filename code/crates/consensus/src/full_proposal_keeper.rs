use std::collections::BTreeMap;

use derive_where::derive_where;
use tracing::{debug, warn};

use malachite_common::{Context, Proposal, Round, SignedProposal, Validity, Value};

use crate::ProposedValue;

/// This module is responsible for collecting proposed values and consensus proposal messages for
/// a given (height, round).
/// When a new_value is received from the value builder the following entry is stored:
/// `FullProposal { Some(new_value.value, new_value.validity), None }`
///
/// When a new_proposal is received from consensus gossip the following entry is stored:
/// `FullProposal { None, Some(new_proposal) }`
///
/// When both proposal and values have been received, the entry for (height, round) should be:
/// `FullProposal { Some(value.value, value.validity), Some(proposal) }`
///
/// It is possible that a proposer sends two (builder_value, proposal) pairs for same `(height, round)`.
/// In this case, both are stored and we consider that the proposer is equivocating.
/// Currently, the actual equivocation is caught deeper in the consensus crate, through consensus actor
/// propagating both proposals.
///
/// Note: In the future when we support implicit proposal message:
/// - store_proposal() will never be called
/// - get_full_proposal() should only check the presence of `builder_value`

#[derive_where(Clone, Debug)]
struct Entry<Ctx: Context> {
    // Value if received from the builder and its validity.
    builder_value: Option<(Ctx::Value, Validity)>,
    // Proposal consensus message if received.
    proposal: Option<SignedProposal<Ctx>>,
}

impl<Ctx: Context> Entry<Ctx> {
    pub fn new(
        builder_value: Option<(Ctx::Value, Validity)>,
        proposal: Option<SignedProposal<Ctx>>,
    ) -> Self {
        Self {
            builder_value,
            proposal,
        }
    }
}

#[derive_where(Clone, Debug)]
pub struct FullProposal<'a, Ctx: Context> {
    // Proposal consensus message
    pub proposal: &'a SignedProposal<Ctx>,
    // Validity of the proposal
    pub validity: Validity,
}

impl<'a, Ctx: Context> FullProposal<'a, Ctx> {
    pub fn new(proposal: &'a SignedProposal<Ctx>, validity: Validity) -> Self {
        Self { proposal, validity }
    }
}

#[derive_where(Clone, Debug)]
pub struct FullProposalKeeper<Ctx: Context> {
    keeper: BTreeMap<(Ctx::Height, Round), Vec<Entry<Ctx>>>,
}

impl<Ctx: Context> FullProposalKeeper<Ctx> {
    pub fn new() -> Self {
        Self {
            keeper: BTreeMap::new(),
        }
    }

    pub fn get_full_proposal(
        &self,
        height: &Ctx::Height,
        round: Round,
        value: &Ctx::Value,
    ) -> Option<FullProposal<'_, Ctx>> {
        let entries = self
            .keeper
            .get(&(*height, round))
            .filter(|entries| !entries.is_empty())?;

        for entry in entries {
            match (&entry.builder_value, &entry.proposal) {
                (Some((_, validity)), Some(prop)) => {
                    if prop.value().id() == value.id() {
                        return Some(FullProposal::new(prop, *validity));
                    }
                }
                _ => {
                    return None;
                }
            }
        }

        None
    }

    pub fn store_proposal(&mut self, new_proposal: SignedProposal<Ctx>) {
        let key = (new_proposal.height(), new_proposal.round());
        let entries = self.keeper.get_mut(&key);

        match entries {
            None => {
                // First time we see something (a proposal) for this height and round
                // Create a full proposal with just the proposal
                let full_proposal = Entry::new(None, Some(new_proposal));
                self.keeper.insert(key, vec![full_proposal]);
            }
            Some(entries) => {
                // We have seen values and/ or proposals for this height and round.
                // Iterate over the vector of full proposals and determine if a new entry needs
                // to be appended or an existing one has to be modified.
                for entry in entries.iter_mut() {
                    let Entry {
                        builder_value,
                        proposal: existing_proposal,
                        ..
                    } = entry;

                    match (builder_value, existing_proposal) {
                        (Some((value, _)), None) => {
                            if value == new_proposal.value() {
                                // Found a matching value. Add the proposal
                                entry.proposal = Some(new_proposal);
                                return;
                            }
                        }
                        (_, Some(proposal)) => {
                            if proposal.value() == new_proposal.value() {
                                // Redundant proposal
                                return;
                            }
                        }
                        (_, _) => {
                            panic!("Should never have empty entries")
                        }
                    }
                }

                // Append new proposal
                entries.push(Entry::new(None, Some(new_proposal.clone())));
            }
        }
    }

    pub fn store_value(&mut self, new_value: ProposedValue<Ctx>) {
        let key = (new_value.height, new_value.round);
        let entries = self.keeper.get_mut(&key);

        match entries {
            None => {
                // First time we see something (a proposed value) for this height and round
                // Create a full proposal with just the proposal
                let full_proposal = Entry::new(Some((new_value.value, new_value.validity)), None);
                self.keeper.insert(key, vec![full_proposal]);
            }
            Some(entries) => {
                // We have seen proposals and/ or values for this height and round.
                // Iterate over the vector of full proposals and determine if a new entry needs
                // to be appended or an existing one has to be modified.
                for p in entries.iter_mut() {
                    let Entry {
                        builder_value: existing_value,
                        proposal,
                        ..
                    } = p;

                    match (existing_value, proposal) {
                        (None, Some(proposal)) => {
                            if proposal.value().id() == new_value.value.id() {
                                // Found a matching proposal. Change the entry at index i
                                p.builder_value = Some((new_value.value, new_value.validity));
                                return;
                            }
                        }
                        (Some((value, _)), _) => {
                            if value.id() == new_value.value.id() {
                                // Same value received before, nothing to do.
                                return;
                            }
                        }
                        (_, _) => {
                            panic!("Should never have empty entries")
                        }
                    }
                }

                // Append new value
                entries.push(Entry::new(
                    Some((new_value.value, new_value.validity)),
                    None,
                ));
            }
        }
    }

    pub fn remove_full_proposals(&mut self, height: Ctx::Height, round: Round) {
        // TODO - keep some heights back?
        debug!(%height, %round, "Removing full proposals");

        let result = self.keeper.remove_entry(&(height, round));
        match result {
            None => {
                warn!(%height, %round, "Full proposals absent");
            }
            Some((_key, removed)) => {
                debug!(%height, %round, "Removed {} full proposals", removed.len());
            }
        }
    }
}

impl<Ctx: Context> Default for FullProposalKeeper<Ctx> {
    fn default() -> Self {
        Self::new()
    }
}
