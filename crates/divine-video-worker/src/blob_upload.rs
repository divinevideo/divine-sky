// S3 blob upload — trait-based abstraction for uploading blobs to object storage

use anyhow::Result;
use async_trait::async_trait;

/// Trait for blob storage backends. Allows mocking in tests.
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Upload blob data to storage at key `{did}/{cid}`.
    async fn upload_blob(&self, did: &str, cid: &str, data: &[u8], mime_type: &str) -> Result<()>;

    /// Check if a blob already exists at key `{did}/{cid}`.
    async fn check_exists(&self, did: &str, cid: &str) -> Result<bool>;
}

/// S3-backed blob uploader.
pub struct BlobUploader {
    bucket: String,
    // In a real implementation this would hold an aws_sdk_s3::Client.
    // For now we keep the bucket name and delegate via the BlobStore trait.
}

impl BlobUploader {
    pub fn new(bucket: &str) -> Self {
        Self {
            bucket: bucket.to_string(),
        }
    }

    pub fn bucket(&self) -> &str {
        &self.bucket
    }
}

/// Build the S3 object key for a blob.
pub fn blob_key(did: &str, cid: &str) -> String {
    format!("{}/{}", did, cid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory mock blob store for testing.
    struct MockBlobStore {
        store: Mutex<HashMap<String, (Vec<u8>, String)>>,
    }

    impl MockBlobStore {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl BlobStore for MockBlobStore {
        async fn upload_blob(
            &self,
            did: &str,
            cid: &str,
            data: &[u8],
            mime_type: &str,
        ) -> Result<()> {
            let key = blob_key(did, cid);
            self.store
                .lock()
                .unwrap()
                .insert(key, (data.to_vec(), mime_type.to_string()));
            Ok(())
        }

        async fn check_exists(&self, did: &str, cid: &str) -> Result<bool> {
            let key = blob_key(did, cid);
            Ok(self.store.lock().unwrap().contains_key(&key))
        }
    }

    #[test]
    fn test_blob_key_format() {
        assert_eq!(
            blob_key("did:plc:abc123", "bafkreixyz"),
            "did:plc:abc123/bafkreixyz"
        );
    }

    #[tokio::test]
    async fn test_upload_constructs_correct_key() {
        let store = MockBlobStore::new();
        let did = "did:plc:user1";
        let cid = "bafkreiabc";
        let data = b"video bytes";

        store
            .upload_blob(did, cid, data, "video/mp4")
            .await
            .unwrap();

        let key = blob_key(did, cid);
        let inner = store.store.lock().unwrap();
        let (stored_data, stored_mime) = inner.get(&key).expect("blob should exist");
        assert_eq!(stored_data, data);
        assert_eq!(stored_mime, "video/mp4");
    }

    #[tokio::test]
    async fn test_check_exists_true() {
        let store = MockBlobStore::new();
        let did = "did:plc:user1";
        let cid = "bafkreiabc";

        store
            .upload_blob(did, cid, b"data", "application/octet-stream")
            .await
            .unwrap();

        assert!(store.check_exists(did, cid).await.unwrap());
    }

    #[tokio::test]
    async fn test_check_exists_false() {
        let store = MockBlobStore::new();
        assert!(!store
            .check_exists("did:plc:none", "bafkreinone")
            .await
            .unwrap());
    }

    #[test]
    fn test_blob_uploader_bucket() {
        let uploader = BlobUploader::new("my-bucket");
        assert_eq!(uploader.bucket(), "my-bucket");
    }
}
