//! `config.toml` × `style.toml` の横断バリデーションと段幅の導出
//!
//! config.toml のデータモデル・読込・検証は [`crate::project::config`]、style.toml は
//! [`crate::style`] の所有（#351）。ここに残っているのは、どちらか一方だけでは判定できない
//! 横断制約だけで、後続で `typeset` へ移す。

mod layout;

pub use layout::{LayoutValidationError, column_width, validate_layout};
