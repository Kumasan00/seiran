//! ユーザ設定（config.toml / style.toml）のデータモデルと読込・検証

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
  NestedOrderedFormat, NumberSide, NumberStyle, PageNumbering, PageStyle, QuoteStyle, ReadStyleError, ReferenceStyle,
  RunningContentStyle, Style, StyleValidationError, TableStyle, TextBlockStyle, TheoremClass, TheoremPresentation,
  TheoremReset, TheoremStyle, Theorems, TitlePageStyle, TocStyle, default_for_class, default_for_level, parse_style,
  read_style,
};
