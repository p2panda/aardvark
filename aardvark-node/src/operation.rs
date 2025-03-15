use std::hash::Hash as StdHash;
use std::time::SystemTime;

use anyhow::{Result, bail};
use p2panda_core::cbor::{decode_cbor, encode_cbor};
use p2panda_core::{Body, Extension, Extensions, Header, Operation, PrivateKey, PruneFlag};
use p2panda_store::LogStore;
use p2panda_stream::operation::{IngestResult, ingest_operation};
use serde::{Deserialize, Serialize};

use crate::document::Document;
use crate::store::{LogId, OperationStore};

/// Custom extensions for p2panda header.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AardvarkExtensions {
    /// If flag is true we can remove all previous operations in this log.
    ///
    /// This usually indicates that a "snapshot" has been inserted into the body of this operation,
    /// containing all required state to reconstruct the full version including all previous edits
    /// of this document.
    ///
    /// In our case of a text-editor, this would be the encoded payload of a state-based CRDT.
    #[serde(
        rename = "p",
        skip_serializing_if = "PruneFlag::is_not_set",
        default = "PruneFlag::default"
    )]
    pub prune_flag: PruneFlag,

    /// Operations can be organised in separate logs. With a "log id" we can declare where this
    /// operation belongs to.
    ///
    /// We organise two logs per author per document, one for "short lived" / ephemeral deltas
    /// (small text changes) and one for persisted snapshots (full document history). These are two
    /// distinct "log types".
    #[serde(rename = "t")]
    pub log_type: LogType,

    /// Identifier of the text document this operation relates to.
    ///
    /// Can be `None` if this operation indicates that we are creating a new document. In this case
    /// we take the hash of the header itself to derive the document id.
    #[serde(rename = "d")]
    pub document: Option<Document>,
}

#[derive(Copy, Clone, Default, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub enum LogType {
    Snapshot,
    #[default]
    Delta,
}

impl Extension<PruneFlag> for AardvarkExtensions {
    fn extract(header: &Header<Self>) -> Option<PruneFlag> {
        header
            .extensions
            .as_ref()
            .map(|extensions| extensions.prune_flag.clone())
    }
}

impl Extension<LogType> for AardvarkExtensions {
    fn extract(header: &Header<Self>) -> Option<LogType> {
        header
            .extensions
            .as_ref()
            .map(|extensions| extensions.log_type)
    }
}

impl Extension<Document> for AardvarkExtensions {
    fn extract(header: &Header<Self>) -> Option<Document> {
        // Check if header mentions an document.
        let document = header
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.document);
        if document.is_some() {
            return document;
        }

        // No document was mentioned, we must be creating a new document.
        //
        // If this is the first operation in the append-only log we use the hash of the header
        // itself to determine the document id.
        //
        // Subsequent operations will continue to mention it, if this is not the case we have an
        // invalid operation. In this case we return `None` here and our validation logic will
        // fail.
        match header.seq_num {
            0 => Some(header.hash().into()),
            _ => header
                .extensions
                .as_ref()
                .and_then(|extensions| extensions.document),
        }
    }
}

impl Extension<LogId> for AardvarkExtensions {
    fn extract(header: &Header<Self>) -> Option<LogId> {
        let log_type: Option<LogType> = header.extension();
        let document: Option<Document> = header.extension();

        if let (Some(log_type), Some(document)) = (log_type, document) {
            Some(LogId::new(log_type, &document))
        } else {
            None
        }
    }
}

/// Creates, signs and stores new operation in the author's append-only log.
///
/// If no document is specified we create a new operation in a new log. The resulting hash of the
/// header can be used to identify that new document.
pub async fn create_operation(
    store: &mut OperationStore,
    private_key: &PrivateKey,
    log_type: LogType,
    document: Option<Document>,
    body: Option<&[u8]>,
    prune_flag: bool,
) -> Result<Operation<AardvarkExtensions>> {
    let body = body.map(Body::new);

    let public_key = private_key.public_key();

    let latest_operation = match document {
        Some(ref document) => {
            let log_id = LogId::new(log_type, document);
            store.latest_operation(&public_key, &log_id).await?
        }
        None => None,
    };

    let (seq_num, backlink) = match latest_operation {
        Some((header, _)) => (header.seq_num + 1, Some(header.hash())),
        None => (0, None),
    };

    let timestamp = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)?
        .as_secs();

    let extensions = AardvarkExtensions {
        prune_flag: PruneFlag::new(prune_flag),
        log_type,
        document,
    };

    let mut header = Header {
        version: 1,
        public_key,
        signature: None,
        payload_size: body.as_ref().map_or(0, |body| body.size()),
        payload_hash: body.as_ref().map(|body| body.hash()),
        timestamp,
        seq_num,
        backlink,
        previous: vec![],
        extensions: Some(extensions),
    };
    header.sign(private_key);

    let document: Document = header.extension().expect("document id from our own logs");
    let log_id = LogId::new(log_type, &document);

    let result = ingest_operation(
        store,
        header.clone(),
        body.clone(),
        header.to_bytes(),
        &log_id,
        prune_flag,
    )
    .await?;

    let IngestResult::Complete(operation) = result else {
        panic!("we should never need to re-order our own operations")
    };

    Ok(operation)
}

/// Custom validation for our own operation headers.
pub fn validate_operation(
    operation: &Operation<AardvarkExtensions>,
    expected_document: &Document,
) -> Result<()> {
    let given_document: Option<Document> = operation.header.extension();
    match given_document {
        Some(given_document) => {
            if &given_document != expected_document {
                bail!(
                    "document id mismatch (expected: {}, received: {})",
                    expected_document,
                    given_document
                );
            }
        }
        None => {
            bail!("document id missing (expected: {})", expected_document);
        }
    }
    Ok(())
}

pub fn encode_gossip_operation<E>(header: Header<E>, body: Option<Body>) -> Result<Vec<u8>>
where
    E: Extensions + Serialize,
{
    let bytes = encode_cbor(&(header.to_bytes(), body.map(|body| body.to_bytes())))?;
    Ok(bytes)
}

pub fn decode_gossip_message(bytes: &[u8]) -> Result<(Vec<u8>, Option<Vec<u8>>)> {
    let result = decode_cbor(bytes)?;
    Ok(result)
}
