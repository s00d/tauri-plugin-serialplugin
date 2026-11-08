//! GSM 07.10 CMUX (virtual serial channels over one physical port).

pub use serial_codec::cmux as frame;
mod io;
mod path;
mod session;

pub use frame::{encode_uih, DecodedFrame, Deframer};
pub use io::CmuxPhysicalIo;
pub use io::SerialPortIo;
pub use path::{is_data_dlci, mux_path, parse_mux_path};
pub use session::{cmux_n1, CmuxSession, CmuxTimers, DlciChannel};
