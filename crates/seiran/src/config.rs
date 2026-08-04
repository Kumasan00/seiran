//! ユーザ設定（config.toml / style.toml）のデータモデルと読込・検証

// `crate::config` 自身と同名の子 module にすると clippy::module_inception に抵触するため、
// 旧 `config` crate の `mod config;`（config.toml のデータモデル + 読込・検証）は
// `config_toml` に改名して吸収した（#307）。
mod config_toml;
mod layout;
mod project_source;
mod style;

#[doc(hidden)]
pub use config_toml::test_support;
// `crate::config` root の facade re-export（旧 `config` crate の公開 API 全量の維持）。
// `Config`/`FontConfigs`/`TextDirection`/`VariationAxis`/`read_config` は `crate::config::` 経由で
// 実際に広く使われる（`build_pdf`/`typeset`/`font` 等）が、`ConfigValidationError`/`Feature`/
// `ImageConfig`/`Margin`/`OutputConfig`/`PdfConfig`/`ReadConfigError` はこの facade 経由での
// 消費者が現状ない（`Config` の内部フィールド型としてのみ使われ、型名を名指しする箇所がない）。
// standalone crate だった間は外部消費を仮定でき unused_imports を検出されなかったが、`seiran`
// への吸収で可視性が変わり検出されるようになった。Task 5〜7 の precedent どおり、一部の名前だけ
// unused でも re-export 全体はグループのまま保ち、allow をブロック単位で付ける（#307）。
#[allow(unused_imports)]
pub use config_toml::{
  Config, ConfigValidationError, DocumentConfig, Feature, FontConfig, FontConfigs, ImageConfig, Margin, OutputConfig,
  PdfConfig, ReadConfigError, TextDirection, VariationAxis, read_config,
};
pub use layout::{LayoutValidationError, validate_layout};
// `FilesystemProjectSource`/`MemoryProjectSource`/`ProjectPath`/`ProjectSource`/`SourceReadError`
// はすべて `crate::config::` 経由で使われる（`SourceReadError` は `crate::lib.rs` の root facade
// 再エクスポートが唯一の消費者 — `ProjectSource::read_text`/`read_bytes` の戻り値型に現れるため、
// trait を外部から実装可能にするには型そのものも公開しておく必要がある）。unused_imports の
// allow は不要。
pub use project_source::{FilesystemProjectSource, MemoryProjectSource, ProjectPath, ProjectSource, SourceReadError};
// `Style`/`CounterName`/`Counters`/`TheoremReset`/`Theorems`/`Alignment`/`FootnoteNumbering`/
// `FootnoteStyle`/`NumberSide`/`PageNumbering`/`RunningContentStyle`/`TheoremStyle`/`TocStyle`/
// `CaptionStyle`/`TitlePageStyle`/`read_style` は `crate::config::` 経由で実際に使われるが、
// 残りの名前（`ColumnsStyle`/`FigureStyle`/`HeadingStyle`/`HeadingStyles`/`HyperrefStyle`/
// `ListStyle`/`MathBlockStyle`/`MathStyle`/`PageStyle`/`QuoteStyle`/`ReadStyleError`/
// `ReferenceStyle`/`StyleValidationError`/`TableStyle`/`TextBlockStyle`/`TheoremClass`/
// `TheoremPresentation`/`default_for_class`/`default_for_level`/`CounterStyle`/
// `NestedOrderedFormat`/`NumberStyle`/`parse_style`）はこの facade 経由での消費者が現状ない
// （個々の子 module 経由の直接 import や `Style`/`Counters` の内部フィールド型としてのみ
// 使われる）。理由は上の 2 ブロックと同じ（#307）。
#[allow(unused_imports)]
pub use style::{
  Alignment, CaptionStyle, ColumnsStyle, CounterName, CounterStyle, Counters, FigureStyle, FootnoteNumbering,
  FootnoteStyle, HeadingStyle, HeadingStyles, HyperrefStyle, ListStyle, MathBlockStyle, MathScriptStyle, MathStyle,
  NestedOrderedFormat, NumberSide, NumberStyle, PageNumbering, PageStyle, QuoteStyle, ReadStyleError, ReferenceStyle,
  RunningContentStyle, Style, StyleValidationError, TableStyle, TextBlockStyle, TheoremClass, TheoremPresentation,
  TheoremReset, TheoremStyle, Theorems, TitlePageStyle, TocStyle, default_for_class, default_for_level, parse_style,
  read_style,
};
