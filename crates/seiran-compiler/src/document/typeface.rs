//! 言語判定前の書体の指定 [`Typeface`]。

use serde::Deserialize;

/// 言語判定前の書体の指定
///
/// 組版時に文字のスクリプトと組み合わせて 19 種別の [`crate::project::FontType`] へ確定する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Typeface {
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
  /// 数式用フォント
  Math,
}
