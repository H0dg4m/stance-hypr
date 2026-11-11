//! Phone deck: a paired phone (or `stance deck test`) presses buttons that
//! run whitelisted stance actions on this PC. Noise-encrypted TCP over
//! Wi-Fi, or USB through `adb reverse`; pairing by QR.

pub mod actions;
pub mod client;
pub mod config;
pub mod net;
pub mod noise;
pub mod pair;
pub mod proto;
pub mod stream;
#[cfg(test)]
mod vectors;
