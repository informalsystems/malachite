use bytes::Bytes;
use malachite_abci_p2p_types::Height;
use malachite_common::Extension;
use tendermint_proto::v0_38::abci;

use super::AbciClient;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Accepted,
    Other(i32),
}

impl From<i32> for Status {
    fn from(code: i32) -> Self {
        match code {
            1 => Status::Accepted,
            other => Status::Other(other),
        }
    }
}

pub struct AbciApp;

impl AbciApp {
    pub async fn extend_vote(
        abci_client: &mut AbciClient,
        request: abci::RequestExtendVote,
    ) -> Result<Option<Extension>, eyre::Error> {
        let abci_request = abci::Request {
            value: Some(abci::request::Value::ExtendVote(request)),
        };

        let response = abci_client.request_with_flush(abci_request).await?.value;

        match response {
            Some(abci::response::Value::ExtendVote(response)) => {
                Ok(Some(response.vote_extension.into()))
            }
            Some(abci::response::Value::Exception(e)) => {
                eyre::bail!("ABCI app raised an exception: {e:?}")
            }
            Some(other) => eyre::bail!("Received unexpected response from ABCI app: {other:?}"),
            None => Ok(None),
        }
    }

    pub async fn prepare_proposal(
        abci_client: &mut AbciClient,
        request: abci::RequestPrepareProposal,
    ) -> Result<Vec<Bytes>, eyre::Error> {
        let abci_request = abci::Request {
            value: Some(abci::request::Value::PrepareProposal(request)),
        };

        let response = abci_client.request_with_flush(abci_request).await?.value;

        match response {
            Some(abci::response::Value::PrepareProposal(prep)) => Ok(prep.txs),
            Some(abci::response::Value::Exception(e)) => {
                eyre::bail!("ABCI app raised an exception: {e:?}")
            }
            Some(other) => eyre::bail!("Received unexpected response from ABCI app: {other:?}"),
            None => eyre::bail!("No response from ABCI app"),
        }
    }

    pub async fn process_proposal(
        abci_client: &mut AbciClient,
        request: abci::RequestProcessProposal,
    ) -> Result<Status, eyre::Error> {
        let abci_request = abci::Request {
            value: Some(abci::request::Value::ProcessProposal(request)),
        };

        let response = abci_client.request_with_flush(abci_request).await?.value;

        match response {
            Some(abci::response::Value::ProcessProposal(response)) => {
                Ok(Status::from(response.status))
            }
            Some(abci::response::Value::Exception(e)) => {
                eyre::bail!("ABCI app raised an exception: {e:?}")
            }
            Some(other) => eyre::bail!("Received unexpected response from ABCI app: {other:?}"),
            None => eyre::bail!("No response from ABCI app"),
        }
    }

    pub async fn finalize_block(
        abci_client: &mut AbciClient,
        request: abci::RequestFinalizeBlock,
    ) -> Result<abci::ResponseFinalizeBlock, eyre::Error> {
        let abci_request = abci::Request {
            value: Some(abci::request::Value::FinalizeBlock(request)),
        };

        let response = abci_client.request_with_flush(abci_request).await?.value;

        match response {
            Some(abci::response::Value::FinalizeBlock(response)) => Ok(response),
            Some(abci::response::Value::Exception(e)) => {
                eyre::bail!("ABCI app raised an exception: {e:?}")
            }
            Some(other) => eyre::bail!("Received unexpected response from ABCI app: {other:?}"),
            None => eyre::bail!("No response from ABCI app"),
        }
    }

    pub async fn commit(abci_client: &mut AbciClient) -> Result<Option<Height>, eyre::Error> {
        let abci_request = abci::Request {
            value: Some(abci::request::Value::Commit(abci::RequestCommit {})),
        };

        let response = abci_client.request_with_flush(abci_request).await?.value;

        match response {
            Some(abci::response::Value::Commit(response)) => {
                if response.retain_height >= 0 {
                    Ok(Some(Height::new(response.retain_height as u64)))
                } else {
                    Ok(None)
                }
            }
            Some(abci::response::Value::Exception(e)) => {
                eyre::bail!("ABCI app raised an exception: {e:?}")
            }
            Some(other) => eyre::bail!("Received unexpected response from ABCI app: {other:?}"),
            None => eyre::bail!("No response from ABCI app"),
        }
    }
}
