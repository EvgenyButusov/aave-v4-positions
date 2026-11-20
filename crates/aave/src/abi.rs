//! One module per ABI, because each declares its own copy of the structs they share.

pub mod hub;
pub mod oracle;
pub mod spoke;
#[cfg(test)]
mod tests;
