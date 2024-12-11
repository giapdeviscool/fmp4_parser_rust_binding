use o_thi_offline_search::index::Index as LibIndex;
use std::error::Error;
uniffi::include_scaffolding!("binding_ffi");

// #[uniffi::export]
fn add_test_two_numbers(a: u32, b: u32) -> u32 {
    a + b
}

#[derive(uniffi::Error, thiserror::Error, Debug)]
#[uniffi(flat_error)]
pub enum GenericError {
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Generic {err}")]
    Generic { err: String },
    #[error("not found")]
    NotFound,
}

impl From<String> for GenericError {
    fn from(err: String) -> Self {
        Self::Generic { err }
    }
}

impl GenericError {
    pub fn from_error<T: Error>(err: T) -> Self {
        Self::Generic {
            err: stringify_error_chain(&err),
        }
    }
}

fn stringify_error_chain<T: Error>(error: &T) -> String {
    let mut result = format!("Error: {}\n", error);

    let mut source = error.source();
    while let Some(src) = source {
        result += &format!("Caused by: {}\n", src);
        source = src.source();
    }
    result
}

#[uniffi::export]
pub fn create_index(path: &str, csv: &str) -> Result<(), GenericError> {
    o_thi_offline_search::index::Index::create(path, csv).map_err(GenericError::from_error)?;
    Ok(())
}

#[uniffi::export]
pub fn open_index(path: String) -> Result<FfiIndex, GenericError> {
    let lib_index =
        o_thi_offline_search::index::Index::open(&path).map_err(GenericError::from_error)?;
    Ok(FfiIndex { lib_index })
}
#[derive(uniffi::Object)]
pub struct FfiIndex {
    lib_index: LibIndex,
}

#[uniffi::export]
impl FfiIndex {
    pub fn search_index(&self, input: &str, limit: u64) -> Result<String, GenericError> {
        let lib_index = &self.lib_index;
        let result = lib_index.search(input, limit as usize)?;
        Ok(result)
    }
}
