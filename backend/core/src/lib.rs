//! iclear-core: everything the app can do without root privileges, plus the
//! SMC client shared with the helper.

pub mod apps;
pub mod battery;
pub mod cloud;
pub mod helper_client;
pub mod launchd;
pub mod memory;
pub mod sensors;
pub mod smc;
pub mod storage;
pub mod system;

pub use iclear_protocol as protocol;
