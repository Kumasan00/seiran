//! ユーザ設定（config.toml / style.toml）のデータモデルと読込・検証
//!
//! 物理・実体・メタデータを扱う `config` と、見た目を扱う `style` を非公開の
//! 子モジュールとして内包し、公開 API はこの root の `pub use` で 1 本のパスに揃える
//! （`config::Config` / `config::Style` 等）。エラー型は [`ConfigValidationError`] /
//! [`StyleValidationError`] と接頭辞で区別する。両者にまたがる横断バリデーション（段幅など、
//! 片方の設定ファイルだけでは検証できない制約）は非公開の `layout` 子モジュールに置き、
//! [`validate_layout`] として公開する。

mod config;
mod layout;
mod style;

#[doc(hidden)]
pub use config::test_support;
pub use config::{
  Config, ConfigValidationError, DocumentConfig, Feature, FontConfig, FontConfigs, ImageConfig, Margin, OutputConfig,
  PdfConfig, ReadConfigError, TextDirection, VariationAxis, read_config,
};
pub use layout::{LayoutValidationError, validate_layout};
pub use style::{
  Alignment, CaptionStyle, ColumnsStyle, CounterName, CounterStyle, Counters, FigureStyle, FootnoteNumbering,
  FootnoteStyle, HeadingStyle, HeadingStyles, HyperrefStyle, ListStyle, MathBlockStyle, MathScriptStyle, MathStyle,
  NumberSide, NumberStyle, PageNumbering, PageStyle, QuoteStyle, ReadStyleError, ReferenceStyle, RunningContentStyle,
  Style, StyleValidationError, TableStyle, TextBlockStyle, TheoremClass, TheoremPresentation, TheoremReset,
  TheoremStyle, Theorems, TitlePageStyle, TocStyle, default_for_class, default_for_level, parse_style, read_style,
};
