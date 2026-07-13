//! ユーザ設定（config.toml / style.toml）のデータモデルと読込・検証
//!
//! 物理・実体・メタデータを扱う [`read_config`] と、見た目を扱う [`read_style`] を
//! 子モジュールとして内包します。両モジュールはそれぞれ `ValidationError` 型を持ち
//! 名前が衝突するため、`pub mod` で名前空間として公開し `config::read_config::Config`
//! / `config::read_style::Style` の形で参照します。

pub mod read_config;
pub mod read_style;
