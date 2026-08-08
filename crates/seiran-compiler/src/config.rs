//! `config` / `style` の横断バリデーションと、意味解析への投影
//!
//! config.toml のデータモデル・読込・検証は [`crate::project::config`]、style.toml は
//! [`crate::style`] の所有（#351）。この module に残っているのは、どちらか一方だけでは
//! 判定できない横断制約と投影で、後続の移設対象。

mod layout;
mod policy;

pub use layout::{LayoutValidationError, column_width, validate_layout};
// 意味解析（`crate::semantics`）へ渡す設定の投影。`CounterPolicy` / `TheoremPolicy` は
// `DocumentPolicy` のアクセサ戻り値としてのみ現れ、名指しする消費者がいないので出さない。
pub use policy::DocumentPolicy;
