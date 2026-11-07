// Lets the state macros name `opencdi::` both here and in bins that include these files by path.
extern crate self as opencdi;

pub mod audio;
pub mod bios;
pub mod bus;
pub mod cdic;
pub mod cdrom;
pub mod dma;
pub mod dvc;
#[cfg(feature = "host")]
pub mod frontend;
#[cfg(all(windows, feature = "host"))]
pub mod gui;
pub mod input_script;
pub mod machine;
pub mod mcd212;
pub mod nvram;
pub mod savestate;
pub mod scc_timer;
pub mod slave;

pub use m68k::{CpuCore, CpuType, AddressBus};
