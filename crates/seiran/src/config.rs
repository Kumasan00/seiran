//! ユーザ設定（config.toml / style.toml）のデータモデルと読込・検証

// `crate::config` 自身と同名の子 module にすると clippy::module_inception に抵触するため、
// 旧 `config` crate の `mod config;`（config.toml のデータモデル + 読込・検証）は
// `config_toml` に改名して吸収した（#307）。
mod config_toml;
mod layout;
mod policy;
mod project_source;
mod style;

#[doc(hidden)]
pub use config_toml::test_support;
// `ImageConfig` / `Margin` / `OutputConfig` / `PdfConfig` を facade に置いているのは、
// `build_pdf::publication` の `#[cfg(test)] mod tests` が `Config` を組み立てるのに名指しするため。
// `config_toml` は `config` 非公開の子 module なので、facade を通す以外に crate 内から届く経路がない。
#[allow(unused_imports)]
pub use config_toml::{
  Config, DocumentConfig, FontConfig, FontConfigs, ImageConfig, Margin, OutputConfig, PdfConfig, TextDirection,
  VariationAxis, read_config,
};
pub use layout::{LayoutValidationError, validate_layout};
// 意味解析（`crate::resolve`）へ渡す設定の投影。`CounterPolicy` / `TheoremPolicy` は
// `DocumentPolicy` のアクセサ戻り値としてのみ現れ、名指しする消費者がいないので出さない。
pub use policy::DocumentPolicy;
// `FilesystemProjectSource`/`MemoryProjectSource`/`ProjectPath`/`ProjectSource`/`SourceReadError`
// はすべて `crate::config::` 経由で使われる（`SourceReadError` は `crate::lib.rs` の root facade
// 再エクスポートが唯一の消費者 — `ProjectSource::read_text`/`read_bytes` の戻り値型に現れるため、
// trait を外部から実装可能にするには型そのものも公開しておく必要がある）。unused_imports の
// allow は不要。
pub use project_source::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, ProjectSource, SourceReadError};
// `CounterStyle` / `NestedOrderedFormat` / `NumberStyle` / `parse_style` を facade に置いているのは、
// `resolve::counter` / `typeset::lowering` 配下の `#[cfg(test)] mod tests` と `build_pdf` の
// テスト用子 module が名指しするため。`style` は `config` 非公開の子 module なので、facade を
// 通す以外に crate 内から届く経路がない。
#[allow(unused_imports)]
pub use style::{
  Alignment, CaptionStyle, CounterName, CounterStyle, Counters, FootnoteNumbering, FootnoteStyle, MathScriptStyle,
  NestedOrderedFormat, NumberSide, NumberStyle, PageNumbering, RunningContentStyle, Style, TheoremReset, TheoremStyle,
  TitlePageStyle, TocStyle, parse_style, read_style,
};
