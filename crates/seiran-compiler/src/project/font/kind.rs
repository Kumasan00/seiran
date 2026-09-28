//! 最終的なフォント種別 [`FontType`]。

use strum::{IntoStaticStr, VariantArray};

/// 言語とスタイルが確定した 19 フォント種別
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, IntoStaticStr, VariantArray)]
#[strum(serialize_all = "snake_case")]
pub enum FontType {
  /// Serif 標準フォント
  Serif,
  /// Serif 太字フォント
  SerifBold,
  /// Serif イタリックフォント
  SerifItalic,
  /// Serif 太字イタリックフォント
  SerifBoldItalic,
  /// Sans Serif 標準フォント
  SansSerif,
  /// Sans Serif 太字フォント
  SansSerifBold,
  /// Sans Serif イタリックフォント
  SansSerifItalic,
  /// Sans Serif 太字イタリックフォント
  SansSerifBoldItalic,
  /// Monospace 標準フォント
  Monospace,
  /// Monospace 太字フォント
  MonospaceBold,
  /// Monospace イタリックフォント
  MonospaceItalic,
  /// Monospace 太字イタリックフォント
  MonospaceBoldItalic,
  /// 数式用フォント（OpenType Math テーブル対応）
  Math,
  /// 日本語用 Serif 標準フォント
  JapaneseSerif,
  /// 日本語用 Serif 太字フォント
  JapaneseSerifBold,
  /// 日本語用 Sans Serif 標準フォント
  JapaneseSansSerif,
  /// 日本語用 Sans Serif 太字フォント
  JapaneseSansSerifBold,
  /// 日本語用 Monospace 標準フォント
  JapaneseMonospace,
  /// 日本語用 Monospace 太字フォント
  JapaneseMonospaceBold,
}

impl FontType {
  /// 全フォント種別を宣言順に並べたスライス
  ///
  /// derive が全 variant を宣言順に生成するので、variant を足しても追記漏れは起きない。
  /// 利用側に `strum` のトレイトを import させないよう inherent の定数で包む。
  pub const ALL: &'static [FontType] = <Self as VariantArray>::VARIANTS;

  /// TOML でこのフォント種別を指す `snake_case` のキーを返す
  ///
  /// `[font_configs.<key>]` セクションのキーと一致し、診断メッセージで設定パスを
  /// 表示する際の正規表記としても使用されます（`Debug` フォーマットは `PascalCase` で
  /// ユーザの書いた TOML キーと一致しないため、エラーパスにはこちらを使ってください）。
  #[must_use]
  pub fn as_toml_key(self) -> &'static str { return self.into(); }
}
