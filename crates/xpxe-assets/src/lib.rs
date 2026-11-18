//! 资产校验：当前步骤提供 `OpenPGP` 公钥装甲解析与指纹。
#![doc(test(attr(deny(warnings), forbid(unsafe_code))))]

mod pubkey;

pub use pubkey::{AssetsError, PublicKeyInfo, parse_public_key};
