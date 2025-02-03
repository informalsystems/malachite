use bytes::Bytes;
use malachitebft_core_types::Round;
use malachitebft_proto as proto;
use malachitebft_starknet_p2p_proto::{self as p2p_proto};

use proto::Protobuf;

use crate::{Address, Hash, Height, ProposalCommitment, TransactionBatch};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalInit {
    pub height: Height,
    pub round: Round,
    pub valid_round: Round,
    pub proposer: Address,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProposalFin {
    pub state_diff_commitment: Hash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProposalPart {
    Init(ProposalInit),
    Transactions(TransactionBatch),
    ProposalCommitment(ProposalCommitment),
    Fin(ProposalFin),
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PartType {
    Init,
    Transactions,
    ProposalCommitment,
    Fin,
}

impl ProposalPart {
    pub fn part_type(&self) -> PartType {
        match self {
            Self::Init(_) => PartType::Init,
            Self::Transactions(_) => PartType::Transactions,
            Self::ProposalCommitment(_) => PartType::ProposalCommitment,
            Self::Fin(_) => PartType::Fin,
        }
    }

    pub fn to_sign_bytes(&self) -> Bytes {
        proto::Protobuf::to_bytes(self).unwrap()
    }

    pub fn size_bytes(&self) -> usize {
        self.to_sign_bytes().len() // TODO: Do this more efficiently
    }

    pub fn tx_count(&self) -> usize {
        match self {
            Self::Transactions(txes) => txes.len(),
            _ => 0,
        }
    }

    pub fn as_init(&self) -> Option<&ProposalInit> {
        if let Self::Init(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_transactions(&self) -> Option<&TransactionBatch> {
        if let Self::Transactions(v) = self {
            Some(v)
        } else {
            None
        }
    }

    pub fn as_fin(&self) -> Option<&ProposalFin> {
        if let Self::Fin(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

impl proto::Protobuf for ProposalPart {
    type Proto = p2p_proto::ProposalPart;

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn from_proto(proto: Self::Proto) -> Result<Self, proto::Error> {
        use p2p_proto::proposal_part::Messages;

        let message = proto
            .messages
            .ok_or_else(|| proto::Error::missing_field::<Self::Proto>("messages"))?;

        Ok(match message {
            Messages::Init(init) => ProposalPart::Init(ProposalInit {
                height: Height::new(init.height, 0),
                round: Round::new(init.round),
                valid_round: init.valid_round.into(),
                proposer: Address::from_proto(
                    init.proposer
                        .ok_or_else(|| proto::Error::missing_field::<Self::Proto>("proposer"))?,
                )?,
            }),

            Messages::Fin(fin) => ProposalPart::Fin(ProposalFin {
                state_diff_commitment: Hash::from_proto(fin.state_diff_commitment.ok_or_else(
                    || proto::Error::missing_field::<Self::Proto>("state_diff_commitment"),
                )?)?,
            }),

            Messages::Transactions(txes) => {
                let transactions = TransactionBatch::from_proto(txes)?;
                ProposalPart::Transactions(transactions)
            }

            Messages::ProposalCommitment(commitment) => {
                ProposalPart::ProposalCommitment(ProposalCommitment::from_proto(commitment)?)
            }
        })
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn to_proto(&self) -> Result<Self::Proto, proto::Error> {
        use p2p_proto::proposal_part::Messages;

        let message = match self {
            ProposalPart::Init(init) => Messages::Init(p2p_proto::ProposalInit {
                height: init.height.block_number,
                round: init.round.as_u32().expect("round should not be nil"),
                valid_round: init.valid_round.as_u32(),
                proposer: Some(init.proposer.to_proto()?),
            }),
            ProposalPart::Fin(fin) => Messages::Fin(p2p_proto::ProposalFin {
                state_diff_commitment: Some(fin.state_diff_commitment.to_proto()?),
            }),
            ProposalPart::Transactions(txes) => {
                Messages::Transactions(p2p_proto::TransactionBatch {
                    transactions: txes
                        .as_slice()
                        .iter()
                        .map(|tx| tx.to_proto())
                        .collect::<Result<Vec<_>, _>>()?,
                })
            }
            ProposalPart::ProposalCommitment(commitment) => {
                Messages::ProposalCommitment(commitment.to_proto()?)
            }
        };

        Ok(p2p_proto::ProposalPart {
            messages: Some(message),
        })
    }
}
