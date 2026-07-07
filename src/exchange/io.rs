//! Platform IO hooks for exchange (purge / write).

use crate::error::Error;

/// IO hooks for platform-specific purge / write.
pub trait ExchangeIo {
    fn purge_rx(&self) -> Result<(), Error>;
    fn write_payload(&self, payload: &[u8]) -> Result<(), Error>;
}

/// Physical port write / purge (alias for unified backend trait surface).
pub trait PortBackend: ExchangeIo {
    fn write_physical(&self, path: &str, data: &[u8]) -> Result<(), Error>;
    fn purge_rx(&self, path: &str) -> Result<(), Error>;
}
