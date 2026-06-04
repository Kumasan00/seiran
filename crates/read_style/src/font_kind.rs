//! TOML スタイル設定で指定するフォント種別の表現型。
//!
//! `types::FontKind` のミラー enum として定義されます。`read_style` クレートは
//! workspace 内の他クレートに依存しない独立クレートのため、`types::FontKind` を
//! 直接公開せず、本 enum を介して TOML 上でのフォント種別指定を受け付けます。
//!
//! ## 和欧の選択について
//!
//! ユーザーは「セリフ / サンセリフ / 等幅」「標準 / 太字 / 斜体 / 太字斜体」「数式」という
//! 役割（13 種）だけを指定し、和文フォント vs 欧文フォントの物理的な切替は
//! 後段の `layout` クレートが文字単位のスクリプト判定（ICU）に基づいて行います。
//! そのため、本 enum には `Japanese*` バリアントは存在しません。
//!
//! 利用側（`layout` クレート）でこの値を `types::FontKind` に変換します。

use garde::Validate;
use serde::{Deserialize, Serialize};

/// フォント種別（論理 13 種）。
///
/// TOML 上では `serde(rename_all = "snake_case")` により `snake_case` 文字列で指定します
/// （例: `"serif_bold"`, `"sans_serif_italic"`）。和文・欧文の物理フォント切替は
/// `layout` 側で文字スクリプトに基づいて自動的に行われるため、ここでは選択しません。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub enum FontKindConfig {
  /// セリフ（標準）
  Serif,
  /// セリフ太字
  SerifBold,
  /// セリフ斜体
  SerifItalic,
  /// セリフ太字斜体
  SerifBoldItalic,
  /// サンセリフ（標準）
  SansSerif,
  /// サンセリフ太字
  SansSerifBold,
  /// サンセリフ斜体
  SansSerifItalic,
  /// サンセリフ太字斜体
  SansSerifBoldItalic,
  /// 等幅（標準）
  Monospace,
  /// 等幅太字
  MonospaceBold,
  /// 等幅斜体
  MonospaceItalic,
  /// 等幅太字斜体
  MonospaceBoldItalic,
  /// 数式
  Math,
}

impl Default for FontKindConfig {
  fn default() -> Self { return FontKindConfig::Serif; }
}

#[cfg(test)]
mod tests {
  use figment2::{
    Figment,
    providers::{Format, Toml},
  };
  use serde::Deserialize;

  use super::FontKindConfig;

  /// `kind = "..."` のみを持つラッパで `FontKindConfig` 単体をデシリアライズする
  #[derive(Debug, Deserialize)]
  struct Wrapper {
    kind: FontKindConfig,
  }

  #[test]
  fn default_is_serif() {
    // Arrange / Act
    let kind = FontKindConfig::default();

    // Assert
    assert_eq!(kind, FontKindConfig::Serif);
  }

  #[test]
  fn deserializes_snake_case_serif_bold() {
    // Arrange
    let toml = "kind = \"serif_bold\"\n";

    // Act
    let wrapper: Wrapper = Figment::from(Toml::string(toml)).extract().expect("serif_bold should deserialize");

    // Assert
    assert_eq!(wrapper.kind, FontKindConfig::SerifBold);
  }

  #[test]
  fn deserializes_snake_case_sans_serif_italic() {
    // Arrange
    let toml = "kind = \"sans_serif_italic\"\n";

    // Act
    let wrapper: Wrapper = Figment::from(Toml::string(toml)).extract().expect("sans_serif_italic should deserialize");

    // Assert
    assert_eq!(wrapper.kind, FontKindConfig::SansSerifItalic);
  }

  #[test]
  fn rejects_unknown_variant() {
    // Arrange
    let toml = "kind = \"comic_sans\"\n";

    // Act
    let result: Result<Wrapper, _> = Figment::from(Toml::string(toml)).extract();

    // Assert
    assert!(result.is_err(), "未知のバリアントは拒否される");
  }

  #[test]
  fn rejects_removed_japanese_variant() {
    // Arrange: 旧 `japanese_*` バリアントは廃止されたので拒否されること
    let toml = "kind = \"japanese_serif\"\n";

    // Act
    let result: Result<Wrapper, _> = Figment::from(Toml::string(toml)).extract();

    // Assert
    assert!(result.is_err(), "japanese_serif は廃止されたので拒否される");
  }
}
