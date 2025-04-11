use std::ops::RangeBounds;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use prost::Message;
use redb::ReadableTable;
use thiserror::Error;
use tracing::error;

use malachitebft_app_channel::app::types::codec::Codec;
use malachitebft_app_channel::app::types::core::{CommitCertificate, Round};
use malachitebft_app_channel::app::types::ProposedValue;
use malachitebft_proto::{Error as ProtoError, Protobuf};
use malachitebft_test::codec::proto as codec;
use malachitebft_test::codec::proto::ProtobufCodec;
use malachitebft_test::{proto, Address};
use malachitebft_test::{Height, TestContext, Value, ValueId};

mod keys;
use keys::{HeightKey, UndecidedValueKey};

#[derive(Clone, Debug)]
pub struct DecidedValue {
    pub value: Value,
    pub certificate: CommitCertificate<TestContext>,
}

fn decode_certificate(bytes: &[u8]) -> Result<CommitCertificate<TestContext>, ProtoError> {
    let proto = proto::CommitCertificate::decode(bytes)?;
    codec::decode_commit_certificate(proto)
}

fn encode_certificate(certificate: &CommitCertificate<TestContext>) -> Result<Vec<u8>, ProtoError> {
    let proto = codec::encode_commit_certificate(certificate)?;
    Ok(proto.encode_to_vec())
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("Database error: {0}")]
    Database(#[from] redb::DatabaseError),

    #[error("Storage error: {0}")]
    Storage(#[from] redb::StorageError),

    #[error("Table error: {0}")]
    Table(#[from] redb::TableError),

    #[error("Commit error: {0}")]
    Commit(#[from] redb::CommitError),

    #[error("Transaction error: {0}")]
    Transaction(#[from] redb::TransactionError),

    #[error("Failed to encode/decode Protobuf: {0}")]
    Protobuf(#[from] ProtoError),

    #[error("Failed to join on task: {0}")]
    TaskJoin(#[from] tokio::task::JoinError),
}

const CERTIFICATES_TABLE: redb::TableDefinition<HeightKey, Vec<u8>> =
    redb::TableDefinition::new("certificates");

const DECIDED_VALUES_TABLE: redb::TableDefinition<HeightKey, Vec<u8>> =
    redb::TableDefinition::new("decided_values");

const UNDECIDED_PROPOSALS_TABLE: redb::TableDefinition<UndecidedValueKey, Vec<u8>> =
    redb::TableDefinition::new("undecided_values");

struct Db {
    db: redb::Database,
}

impl Db {
    fn new(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Ok(Self {
            db: redb::Database::create(path).map_err(StoreError::Database)?,
        })
    }

    fn get_decided_value(&self, height: Height) -> Result<Option<DecidedValue>, StoreError> {
        let tx = self.db.begin_read()?;
        let value = {
            let table = tx.open_table(DECIDED_VALUES_TABLE)?;
            let value = table.get(&height)?;
            value.and_then(|value| Value::from_bytes(&value.value()).ok())
        };
        let certificate = {
            let table = tx.open_table(CERTIFICATES_TABLE)?;
            let value = table.get(&height)?;
            value.and_then(|value| decode_certificate(&value.value()).ok())
        };

        let decided_value = value
            .zip(certificate)
            .map(|(value, certificate)| DecidedValue { value, certificate });

        Ok(decided_value)
    }

    fn insert_decided_value(&self, decided_value: DecidedValue) -> Result<(), StoreError> {
        let height = decided_value.certificate.height;

        let tx = self.db.begin_write()?;
        {
            let mut values = tx.open_table(DECIDED_VALUES_TABLE)?;
            values.insert(height, decided_value.value.to_bytes()?.to_vec())?;
        }
        {
            let mut certificates = tx.open_table(CERTIFICATES_TABLE)?;
            certificates.insert(height, encode_certificate(&decided_value.certificate)?)?;
        }
        tx.commit()?;

        Ok(())
    }

    pub fn get_undecided_proposal(
        &self,
        height: Height,
        round: Round,
        value_id: ValueId,
    ) -> Result<Option<ProposedValue<TestContext>>, StoreError> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;

        let value = if let Ok(Some(value)) = table.get(&(height, round, value_id)) {
            Some(
                ProtobufCodec
                    .decode(Bytes::from(value.value()))
                    .map_err(StoreError::Protobuf)?,
            )
        } else {
            None
        };

        Ok(value)
    }

    fn get_undecided_proposals(
        &self,
        height: Height,
        round: Round,
    ) -> Result<Vec<ProposedValue<TestContext>>, StoreError> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;

        let mut proposals = Vec::new();
        for result in table.iter()? {
            let (key, value) = result?;
            let (h, r, _) = key.value();

            if h == height && r == round {
                let bytes = value.value();

                let proposal = ProtobufCodec
                    .decode(Bytes::from(bytes))
                    .map_err(StoreError::Protobuf)?;

                proposals.push(proposal);
            }
        }

        Ok(proposals)
    }

    fn get_our_undecided_proposals(
        &self,
        height: Height,
        round: Round,
        proposer: Address,
    ) -> Result<Vec<ProposedValue<TestContext>>, StoreError> {
        let tx: redb::ReadTransaction = self.db.begin_read()?;
        let table = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;

        let mut proposals = Vec::new();
        for result in table.iter()? {
            let (key, value) = result?;
            let (h, r, _) = key.value();

            if h == height && r == round {
                let bytes = value.value();

                let proposal: ProposedValue<TestContext> = ProtobufCodec
                    .decode(Bytes::from(bytes))
                    .map_err(StoreError::Protobuf)?;

                if proposal.proposer == proposer {
                    proposals.push(proposal);
                }
            }
        }

        Ok(proposals)
    }

    fn insert_undecided_proposal(
        &self,
        proposal: ProposedValue<TestContext>,
    ) -> Result<(), StoreError> {
        let key = (proposal.height, proposal.round, proposal.value.id());
        let value = ProtobufCodec.encode(&proposal)?;
        let tx = self.db.begin_write()?;
        {
            let mut table = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;
            table.insert(key, value.to_vec())?;
        }
        tx.commit()?;
        Ok(())
    }

    fn height_range<Table>(
        &self,
        table: &Table,
        range: impl RangeBounds<Height>,
    ) -> Result<Vec<Height>, StoreError>
    where
        Table: redb::ReadableTable<HeightKey, Vec<u8>>,
    {
        Ok(table
            .range(range)?
            .flatten()
            .map(|(key, _)| key.value())
            .collect::<Vec<_>>())
    }

    fn prune(&self, retain_height: Height) -> Result<Vec<Height>, StoreError> {
        let tx = self.db.begin_write().unwrap();
        let pruned = {
            // Remove all undecided proposals
            let mut undecided = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;
            let keys: Vec<_> = undecided
                .iter()?
                .filter_map(|result| result.ok().map(|(key, _)| key.value()))
                .collect();
            for key in keys {
                undecided.remove(key)?;
            }

            // Prune decided values and certificates up to the retain height
            let mut decided = tx.open_table(DECIDED_VALUES_TABLE)?;
            let mut certificates = tx.open_table(CERTIFICATES_TABLE)?;

            let keys = self.height_range(&decided, ..retain_height)?;
            for key in &keys {
                decided.remove(key)?;
                certificates.remove(key)?;
            }
            keys
        };
        tx.commit()?;

        Ok(pruned)
    }

    fn min_decided_value_height(&self) -> Option<Height> {
        let tx = self.db.begin_read().unwrap();
        let table = tx.open_table(DECIDED_VALUES_TABLE).unwrap();
        let (key, _) = table.first().ok()??;
        Some(key.value())
    }

    fn max_decided_value_height(&self) -> Option<Height> {
        let tx = self.db.begin_read().unwrap();
        let table = tx.open_table(DECIDED_VALUES_TABLE).unwrap();
        let (key, _) = table.last().ok()??;
        Some(key.value())
    }

    fn create_tables(&self) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;
        // Implicitly creates the tables if they do not exist yet
        let _ = tx.open_table(DECIDED_VALUES_TABLE)?;
        let _ = tx.open_table(CERTIFICATES_TABLE)?;
        let _ = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;
        tx.commit()?;
        Ok(())
    }

    fn remove_undecided_proposals_by_value_id(&self, value_id: ValueId) -> Result<(), StoreError> {
        let tx = self.db.begin_write()?;

        {
            let mut table = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;
            let keys: Vec<(Height, Round, ValueId)> = table
                .iter()?
                .filter_map(|result| {
                    let (key, _) = result.ok()?;
                    if key.value().2 == value_id {
                        Some(key.value())
                    } else {
                        None
                    }
                })
                .collect();

            for key in keys {
                table.remove(key)?;
            }
        }
        tx.commit()?;

        Ok(())
    }

    fn get_undecided_proposal_by_value_id(
        &self,
        value_id: ValueId,
    ) -> Result<Option<ProposedValue<TestContext>>, StoreError> {
        let tx = self.db.begin_read()?;
        let table = tx.open_table(UNDECIDED_PROPOSALS_TABLE)?;

        for result in table.iter()? {
            let (_, value) = result?;
            let proposal: ProposedValue<TestContext> = ProtobufCodec
                .decode(Bytes::from(value.value()))
                .map_err(StoreError::Protobuf)?;

            if proposal.value.id() == value_id {
                return Ok(Some(proposal));
            }
        }

        Ok(None)
    }
}

#[derive(Clone)]
pub struct Store {
    db: Arc<Db>,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let db = Db::new(path)?;
        db.create_tables()?;

        Ok(Self { db: Arc::new(db) })
    }

    pub async fn min_decided_value_height(&self) -> Option<Height> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.min_decided_value_height())
            .await
            .ok()
            .flatten()
    }

    pub async fn max_decided_value_height(&self) -> Option<Height> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.max_decided_value_height())
            .await
            .ok()
            .flatten()
    }

    pub async fn get_decided_value(
        &self,
        height: Height,
    ) -> Result<Option<DecidedValue>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.get_decided_value(height)).await?
    }

    pub async fn store_decided_value(
        &self,
        certificate: &CommitCertificate<TestContext>,
        value: Value,
    ) -> Result<(), StoreError> {
        let decided_value = DecidedValue {
            value,
            certificate: certificate.clone(),
        };

        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.insert_decided_value(decided_value)).await?
    }

    pub async fn store_undecided_proposal(
        &self,
        value: ProposedValue<TestContext>,
    ) -> Result<(), StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.insert_undecided_proposal(value)).await?
    }

    pub async fn get_undecided_proposal(
        &self,
        height: Height,
        round: Round,
        value_id: ValueId,
    ) -> Result<Option<ProposedValue<TestContext>>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.get_undecided_proposal(height, round, value_id))
            .await?
    }

    pub async fn get_undecided_proposals(
        &self,
        height: Height,
        round: Round,
    ) -> Result<Vec<ProposedValue<TestContext>>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.get_undecided_proposals(height, round)).await?
    }

    pub async fn get_our_undecided_proposals(
        &self,
        height: Height,
        round: Round,
        proposer: Address,
    ) -> Result<Vec<ProposedValue<TestContext>>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.get_our_undecided_proposals(height, round, proposer))
            .await?
    }

    pub async fn prune(&self, retain_height: Height) -> Result<Vec<Height>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.prune(retain_height)).await?
    }

    pub async fn remove_undecided_proposals_by_value_id(
        &self,
        value_id: ValueId,
    ) -> Result<(), StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.remove_undecided_proposals_by_value_id(value_id))
            .await?
    }

    pub async fn get_undecided_proposal_by_value_id(
        &self,
        value_id: ValueId,
    ) -> Result<Option<ProposedValue<TestContext>>, StoreError> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || db.get_undecided_proposal_by_value_id(value_id)).await?
    }
}
