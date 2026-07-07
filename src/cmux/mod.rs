//! GSM 07.10 CMUX (virtual serial channels over one physical port).

mod frame;
mod io;
mod path;
mod session;

pub use frame::{encode_uih, DecodedFrame, Deframer};
pub use io::CmuxPhysicalIo;
#[cfg(desktop)]
pub use io::SerialPortIo;
#[cfg(target_os = "android")]
pub use io::MobileCmuxIo;
pub use path::{mux_path, parse_mux_path};
pub use session::{CmuxSession, DlciChannel};
