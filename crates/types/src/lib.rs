//! 共通型定義モジュール
//!
//! このモジュールは、プロジェクト全体で使用される
//! グリフマッピング、アドバンス幅リスト、CID-GIDマッピング、
//! `ToUnicode` `CMap`などの共通型を定義します。

/// フォントの種類を表す列挙型
///
/// 設定から適切なフォント情報を取得するために使用されます。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontType {
  Serif,
  SerifBold,
  SerifItalic,
  SerifBoldItalic,
  SansSerif,
  SansSerifBold,
  SansSerifItalic,
  SansSerifBoldItalic,
  Monospace,
  MonospaceBold,
  MonospaceItalic,
  MonospaceBoldItalic,
  Math,
  JapaneseSerif,
  JapaneseSerifBold,
  JapaneseSansSerif,
  JapaneseSansSerifBold,
  JapaneseMonospace,
  JapaneseMonospaceBold,
}

impl FontType {
  pub const ALL: [FontType; 19] = [
    FontType::Serif,
    FontType::SerifBold,
    FontType::SerifItalic,
    FontType::SerifBoldItalic,
    FontType::SansSerif,
    FontType::SansSerifBold,
    FontType::SansSerifItalic,
    FontType::SansSerifBoldItalic,
    FontType::Monospace,
    FontType::MonospaceBold,
    FontType::MonospaceItalic,
    FontType::MonospaceBoldItalic,
    FontType::Math,
    FontType::JapaneseSerif,
    FontType::JapaneseSerifBold,
    FontType::JapaneseSansSerif,
    FontType::JapaneseSansSerifBold,
    FontType::JapaneseMonospace,
    FontType::JapaneseMonospaceBold,
  ];
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontKind {
  Serif,
  SerifBold,
  SerifItalic,
  SerifBoldItalic,
  SansSerif,
  SansSerifBold,
  SansSerifItalic,
  SansSerifBoldItalic,
  Monospace,
  MonospaceBold,
  MonospaceItalic,
  MonospaceBoldItalic,
  Math,
}
