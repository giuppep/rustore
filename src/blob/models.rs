use super::errors::{BlobError, BlobErrorKind, Result};
use chrono::{offset::Utc, DateTime};
use sha2::{Digest, Sha256};
use std::{
    env,
    fs::{self, File},
    io,
    path::Path,
    path::PathBuf,
};

/// Struct representing a reference to an entry in the blob store
#[derive(Debug, Clone)]
pub struct BlobRef {
    value: String,
}

/// Structure representing the metadata associated to a blob
#[derive(Debug)]
pub struct BlobMetadata {
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    pub created: DateTime<Utc>,
}

impl BlobRef {
    /// Creates a new BlobRef from a valid hex representation of the sha256 hash.
    ///
    /// # Errors
    ///
    /// The method will error if the input string
    /// - has length != 64
    /// - contains any char except lowercase letters and digits
    /// # Examples
    /// ```
    /// # use rustore::blob::BlobRef;
    /// let blob_ref = BlobRef::new("f29bc64a9d3732b4b9035125fdb3285f5b6455778edca72414671e0ca3b2e0de");
    /// assert!(blob_ref.is_ok())
    /// ```
    /// ```
    /// # use rustore::blob::BlobRef;
    /// let blob_ref = BlobRef::new("a_short_hash");
    /// assert!(blob_ref.is_err());
    /// // TODO
    /// // let blob_ref = BlobRef::new("....aninvalidhash.29bc64a9d3732b4b9035125fdb3285f5b6455778edca7");
    /// // assert!(blob_ref.is_err());
    /// ```

    pub fn new(value: &str) -> Result<BlobRef> {
        // TODO: validate the hash with a regex to avoid someone asking for e.g. `../../`
        match value.len() == 64 {
            true => Ok(BlobRef {
                value: String::from(value),
            }),
            false => Err(BlobError::Blob(BlobErrorKind::InvalidRefLength)),
        }
    }

    /// Returns a BlobRef instance from a hasher
    ///
    /// # Examples
    ///
    /// ```
    /// # use sha2::{Digest, Sha256};
    /// # use rustore::blob::BlobRef;
    /// let mut hasher = Sha256::new();
    /// hasher.update(b"hello world");
    /// let blob_ref = BlobRef::from_hasher(hasher);
    /// ```
    pub fn from_hasher(hasher: Sha256) -> BlobRef {
        BlobRef::new(&format!("{:x}", hasher.finalize())[..]).unwrap()
    }

    /// Creates a BlobRef from a document on disk.
    ///
    /// # Examples
    ///
    /// ```
    /// # use std::path::Path;
    /// # use rustore::blob::BlobRef;
    /// let path = Path::new("test/test_file.txt");
    /// let blob_ref = BlobRef::from_path(path);
    /// assert!(blob_ref.is_ok());
    /// assert_eq!(blob_ref.unwrap().reference(), "f29bc64a9d3732b4b9035125fdb3285f5b6455778edca72414671e0ca3b2e0de")
    /// ```
    /// Note that this *does not* add the file to the blob store, the user will have to
    /// manually save it to `blob_ref.to_path()`.
    pub fn from_path(path: &Path) -> Result<BlobRef> {
        let mut file = File::open(path)?;
        let mut hasher = BlobRef::hasher();

        io::copy(&mut file, &mut hasher)?;
        Ok(BlobRef::from_hasher(hasher))
    }

    /// Returns an instance of the hasher used to compute the blob reference for a file
    ///
    /// # Examples
    ///
    /// ```
    /// # use rustore::blob::BlobRef;
    /// # use sha2::{Digest, Sha256};
    /// let mut hasher = BlobRef::hasher();
    /// hasher.update(b"hello world");
    /// let result = hasher.finalize();
    /// assert_eq!(format!("{:x}", result), "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9")
    /// ```
    pub fn hasher() -> Sha256 {
        Sha256::new()
    }

    /// Converts the blob's reference into a path.
    ///
    /// # Examples
    ///
    /// ```
    /// # use rustore::blob::BlobRef;
    /// std::env::set_var("RUSTORE_DATA_PATH", "/tmp/rustore");
    /// let hash = "f29bc64a9d3732b4b9035125fdb3285f5b6455778edca72414671e0ca3b2e0de";
    /// let blob_ref = BlobRef::new(hash).unwrap();
    /// assert_eq!(blob_ref.to_path().to_str().unwrap(), "/tmp/rustore/f2/9b/c6/4a9d3732b4b9035125fdb3285f5b6455778edca72414671e0ca3b2e0de")
    /// ```
    ///
    /// # Panics
    ///
    /// This function assumes that the `RUSTORE_DATA_PATH` environment variable has been
    /// set to a valid path and panics otherwise.
    pub fn to_path(&self) -> PathBuf {
        let base_path = env::var("RUSTORE_DATA_PATH").unwrap();
        let path = Path::new(&base_path)
            .join(&self.value[0..2])
            .join(&self.value[2..4])
            .join(&self.value[4..6])
            .join(&self.value[6..]);

        path
    }

    /// Returns `true` if there is a file associated with the reference is in the blob store
    pub fn exists(&self) -> bool {
        let dir = self.to_path();
        dir.exists() && dir.read_dir().unwrap().next().is_some()
    }

    /// Deletes the file referenced by the BlobRef.
    pub fn delete(&self) -> Result<()> {
        fs::remove_dir_all(self.to_path()).map_err(BlobError::IO)
    }

    /// Get the full path to the file, including the filename
    ///
    /// # Errors
    ///
    /// Will return an error if
    /// - the directory cannot be read
    /// - the directory is empty
    fn file_path(&self) -> Result<PathBuf> {
        let mut entries = self.to_path().read_dir().map_err(BlobError::IO)?;
        if let Some(Ok(entry)) = entries.next() {
            return Ok(entry.path());
        };
        Err(BlobError::Blob(BlobErrorKind::NotFound))
    }

    /// Returns the mime type inferred from the file's magic number as a string.
    /// It defaults to "application/octet-stream" if it cannot determine the type.
    /// We use the [`infer`] crate to infer the mime type.
    pub fn mime(&self) -> Result<&str> {
        match infer::get_from_path(self.file_path()?).map_err(BlobError::IO)? {
            Some(mime) => Ok(mime.mime_type()),
            _ => Ok("application/octet-stream"),
        }
    }

    /// Get the content of the referenced file as a byte array.
    pub fn content(&self) -> Result<Vec<u8>> {
        fs::read(&self.file_path()?).map_err(BlobError::IO)
    }

    /// Returns some metadata for the file referenced to.
    pub fn metadata(&self) -> Result<BlobMetadata> {
        let file_path = self.file_path()?;
        let filename = file_path.file_name().unwrap().to_str().unwrap().to_string();

        let metadata = fs::metadata(file_path).map_err(BlobError::IO)?;
        Ok(BlobMetadata {
            mime_type: String::from(self.mime()?),
            filename,
            size: metadata.len(),
            created: metadata.created().unwrap().into(),
        })
    }

    /// Returns a string reference (hex representation of Sha256 hash) for the blob
    pub fn reference(&self) -> &str {
        &self.value
    }
}

impl std::fmt::Display for BlobRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BlobRef({})", &self.value[..10])
    }
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     const TEST_DATA_PATH: &str = "test/";
//     const TEST_FILE: &str = "test/test_file.txt";
//     const TEST_FILE_HASH: &str = "f29bc64a9d3732b4b9035125fdb3285f5b6455778edca72414671e0ca3b2e0de";
//     const TEST_FILE_PATH: &str =
//         "f2/9b/c6/4a9d3732b4b9035125fdb3285f5b6455778edca72414671e0ca3b2e0de";

//     #[test]
//     fn test_hashing() {
//         let path = Path::new(TEST_FILE);
//         let blob_ref = BlobRef::from_path(&path).unwrap();
//         assert_eq!(blob_ref.reference(), TEST_FILE_HASH)
//     }

//     #[test]
//     fn test_create_blob_ref() {
//         let valid_hash = TEST_FILE_HASH;
//         let invalid_hash = "this_is_too_short";

//         assert!(BlobRef::new(valid_hash).is_ok());
//         assert!(BlobRef::new(invalid_hash).is_err())
//     }

//     #[test]
//     fn test_get_dir() {
//         env::set_var("RUSTORE_DATA_PATH", TEST_DATA_PATH);

//         let hash = TEST_FILE_HASH;
//         let blob_ref = BlobRef::new(hash).unwrap();
//         let dir = blob_ref.to_path();

//         assert_eq!(
//             dir.to_str().unwrap(),
//             format!("{}{}", TEST_DATA_PATH, TEST_FILE_PATH)
//         )
//     }
// }