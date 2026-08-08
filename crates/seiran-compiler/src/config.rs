//! ユーザ設定（config.toml）のデータモデルと読込・検証
//!
//! style.toml（見た目）は crate root の [`crate::style`] が所有する。

// `crate::config` 自身と同名の子 module にすると clippy::module_inception に抵触するため、
// 旧 `config` crate の `mod config;`（config.toml のデータモデル + 読込・検証）は
// `config_toml` に改名して吸収した（#307）。
mod config_toml;
mod layout;
mod policy;

#[doc(hidden)]
pub use config_toml::test_support;
// `ImageConfig` / `Margin` / `OutputConfig` / `PdfConfig` を facade に置いているのは、
// `compiler::publication` の `#[cfg(test)] mod tests` が `Config` を組み立てるのに名指しするため。
// `config_toml` は `config` 非公開の子 module なので、facade を通す以外に crate 内から届く経路がない。
#[allow(unused_imports)]
pub use config_toml::{Config, DocumentConfig, ImageConfig, Margin, OutputConfig, PdfConfig, read_config};
pub use layout::{LayoutValidationError, column_width, validate_layout};
// 意味解析（`crate::semantics`）へ渡す設定の投影。`CounterPolicy` / `TheoremPolicy` は
// `DocumentPolicy` のアクセサ戻り値としてのみ現れ、名指しする消費者がいないので出さない。
pub use policy::DocumentPolicy;
