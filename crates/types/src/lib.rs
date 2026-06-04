//! 共通型定義モジュール
//!
//! このモジュールは、Seiran の核となる **19 フォント種別システム** と、
//! PDF 生成全体で使用される共通型を定義します。
//!
//! ## `FontMap<T>`
//!
//! [`FontMap`] は 19 フォント種別それぞれに値を対応付ける汎用コンテナです。
//! 内部実装は `HashMap<FontType, T>` で、イテレーションは [`FontType::ALL`] の
//! 順序で返されます。
//!
//! ## 19 フォント種別システム
//!
//! Seiran では、すべてのテキストを以下の 19 フォント種別に分類して処理します：
//!
//! | カテゴリ         | 種別数 | フォント                                              |
//! | -------------- | ------ | -------------------------------------------------- |
//! | Serif (セリフ)  | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Sans Serif     | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Monospace      | 4      | 標準、太字、イタリック、太字イタリック                  |
//! | Math (数式)    | 1      | 数式用フォント（OpenType Math テーブル対応）            |
//! | Japanese       | 6      | Serif (標準/太字) + Sans Serif (標準/太字) + Monospace (標準/太字) |
//!
//! 各文字が解析される際、その属性（言語、スタイル、用途）に基づいて
//! 適切なフォント種別が決定されます。
//!
//! - **`FontType`**: すべてのフォント種別（19 個）
//! - **`FontKind`**: Latin フォント種別のみ（13 個、日本語フォント除外）
//!
//! 関連モジュール：
//! - 設定: `read_config` クレート
//! - フォント処理: `font` クレート
//! - テキスト処理: `parser` クレート、`text` モジュール

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// PDF 生成で使用される 19 フォント種別
///
/// 各テキスト文字は、その属性（言語、スタイル、用途）に基づいて
/// これら 19 種別のいずれかに分類されます。
///
/// # 構成
///
/// - **Serif（セリフ）4 種** - 伝統的な本文用フォント
/// - **Sans Serif（ゴシック）4 種** - 現代的で読みやすいフォント
/// - **Monospace（等幅）4 種** - プログラミングコード用フォント
/// - **Math（数式）1 種** - 数式レンダリング用フォント
/// - **Japanese（日本語）6 種** - 日本語対応フォント
///
/// # 用途
///
/// - **フォント設定取得**: `ProcessedConfig::font_configs.get(font_type)`
/// - **グリフシェイピング**: `HarfRustShapers` は 19 種別のシェーパーを保有
/// - **PDF 埋め込み**: 各フォント種別は独立した PDF フォントオブジェクトになる
/// - **テキスト解析**: 言語・スクリプト判定により自動選択
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontType {
  /// Serif 標準フォント（通常の太さ、通常のゆがみ）
  Serif,
  /// Serif 太字フォント（太字の太さ、通常のゆがみ）
  SerifBold,
  /// Serif イタリックフォント（通常の太さ、右に傾いた形）
  SerifItalic,
  /// Serif 太字イタリックフォント（太字の太さ、右に傾いた形）
  SerifBoldItalic,
  /// Sans Serif 標準フォント（通常の太さ、通常のゆがみ）
  SansSerif,
  /// Sans Serif 太字フォント（太字の太さ、通常のゆがみ）
  SansSerifBold,
  /// Sans Serif イタリックフォント（通常の太さ、右に傾いた形）
  SansSerifItalic,
  /// Sans Serif 太字イタリックフォント（太字の太さ、右に傾いた形）
  SansSerifBoldItalic,
  /// Monospace 標準フォント（等幅、通常の太さ）
  Monospace,
  /// Monospace 太字フォント（等幅、太字の太さ）
  MonospaceBold,
  /// Monospace イタリックフォント（等幅、右に傾いた形）
  MonospaceItalic,
  /// Monospace 太字イタリックフォント（等幅、太字で傾いた形）
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

impl std::fmt::Display for FontType {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let name = match self {
      FontType::Serif => "Serif",
      FontType::SerifBold => "Serif Bold",
      FontType::SerifItalic => "Serif Italic",
      FontType::SerifBoldItalic => "Serif Bold Italic",
      FontType::SansSerif => "Sans Serif",
      FontType::SansSerifBold => "Sans Serif Bold",
      FontType::SansSerifItalic => "Sans Serif Italic",
      FontType::SansSerifBoldItalic => "Sans Serif Bold Italic",
      FontType::Monospace => "Monospace",
      FontType::MonospaceBold => "Monospace Bold",
      FontType::MonospaceItalic => "Monospace Italic",
      FontType::MonospaceBoldItalic => "Monospace Bold Italic",
      FontType::Math => "Math",
      FontType::JapaneseSerif => "Japanese Serif",
      FontType::JapaneseSerifBold => "Japanese Serif Bold",
      FontType::JapaneseSansSerif => "Japanese Sans Serif",
      FontType::JapaneseSansSerifBold => "Japanese Sans Serif Bold",
      FontType::JapaneseMonospace => "Japanese Monospace",
      FontType::JapaneseMonospaceBold => "Japanese Monospace Bold",
    };
    return write!(f, "{name}");
  }
}

impl FontType {
  /// 19 フォント種別すべてを順序付き配列として定義
  ///
  /// これは常に 19 要素を含み、順序は以下の通りです：
  ///
  /// 1-4:  Serif 系（標準、太字、イタリック、太字イタリック）
  /// 5-8:  Sans Serif 系
  /// 9-12: Monospace 系
  /// 13:   Math
  /// 14-19: 日本語系（Serif, Sans Serif, Monospace の標準と太字）
  ///
  /// # 用途
  ///
  /// - イテレーション: `for font_type in FontType::ALL { ... }`
  /// - インデックス検索: 配列インデックスで特定フォント種別にアクセス
  /// - フォント設定マッピング: 各要素に対応する設定を取得
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

/// フォントのスタイル分類（Latin / 日本語 判定前）
///
/// `FontType` の 19 種別から、フォントのスタイル情報のみを抽出した 13 種別です。
/// テキスト処理では、まず `FontKind` でスタイル（Standard/Bold/Italic など）を決定し、
/// その後、テキストの言語（Latin か日本語か）を判定して、具体的な `FontType` を決定します。
///
/// # `FontType` との関係
///
/// - **`FontType`**（19 種別）: 最終的なフォント種別、言語とスタイルが確定した状態
/// - **`FontKind`**（13 種別）: 中間段階、スタイル情報のみで言語未決定
///
/// # 使用フロー
///
/// 1. テキスト文字列を入力
/// 2. **`FontKind` を決定**: スタイル属性（Bold/Italic など）を解析
/// 3. **言語を判定**: 文字が Latin か日本語かを判定
/// 4. **`FontType` に変換**: `FontKind` + 言語判定 → 最終フォント種別
/// 5. **フォント適用**: 確定した `FontType` のフォントでレンダリング
///
/// # 構成
///
/// | カテゴリ    | 種別数 | 内容                                  |
/// | --------- | ------ | ------------------------------------ |
/// | Serif     | 4      | 標準、太字、イタリック、太字イタリック |
/// | Sans Serif| 4      | 標準、太字、イタリック、太字イタリック |
/// | Monospace | 4      | 標準、太字、イタリック、太字イタリック |
/// | Math      | 1      | 数式用フォント                       |
/// | **合計**  | **13** | **スタイル情報のみ（言語未確定）**  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FontKind {
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

/// 全フォント種別 (`FontType`) に対応する値を保持する汎用コンテナ
///
/// 内部的には `HashMap<FontType, T>` を使用しますが、イテレーション時は
/// [`FontType::ALL`] の順序で要素を返します。
///
/// 19 個の個別フィールドを手書きする代わりにこの型を使用することで、
/// フォント種別の追加・削除が `FontType` の定義変更だけで済むようになり、
/// ボイラープレートの `match` 分岐やカスタムイテレータが不要になります。
///
/// # 型パラメータ
///
/// * `T` - 各フォント種別に対応付ける値の型
///
/// # Examples
///
/// ```
/// use types::{FontMap, FontType};
///
/// let map = FontMap::from_all(FontType::ALL.iter().map(|ft| format!("{ft}")));
/// assert_eq!(map.get(FontType::Serif), "Serif");
/// assert_eq!(map.iter().count(), 19);
/// ```
#[derive(Debug, Clone)]
pub struct FontMap<T> {
  inner: HashMap<FontType, T>,
}

impl<T> FontMap<T> {
  /// `FontType::ALL` の順序に対応するイテレータから `FontMap` を構築します
  ///
  /// 渡されたイテレータの各要素を `FontType::ALL[0]`, `FontType::ALL[1]`, ...
  /// と順に対応付けて格納します。
  ///
  /// # Arguments
  ///
  /// * `values` - 19 個の値を順に返すイテレータ
  ///
  /// # Panics
  ///
  /// イテレータの要素数が `FontType::ALL` の要素数（19）と一致しない場合にパニックします。
  pub fn from_all(values: impl IntoIterator<Item = T>) -> Self {
    let inner: HashMap<FontType, T> = FontType::ALL.into_iter().zip(values).collect();
    assert_eq!(inner.len(), FontType::ALL.len(), "FontMap: 要素数が FontType::ALL と一致しません");
    return Self { inner };
  }

  /// 指定されたフォント種別に対応する値への不変参照を返します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別に対応する `&T`
  ///
  /// # Panics
  ///
  /// 指定された `font_type` がマップに存在しない場合にパニックします。
  /// `from_all` で正しく構築されていれば発生しません。
  #[must_use]
  pub fn get(&self, font_type: FontType) -> &T { return &self.inner[&font_type]; }

  /// 指定されたフォント種別に対応する値への可変参照を返します
  ///
  /// # Arguments
  ///
  /// * `font_type` - 取得したいフォント種別
  ///
  /// # Returns
  ///
  /// 指定されたフォント種別に対応する `&mut T`
  ///
  /// # Panics
  ///
  /// 指定された `font_type` がマップに存在しない場合にパニックします。
  #[must_use]
  pub fn get_mut(&mut self, font_type: FontType) -> &mut T {
    #[allow(clippy::expect_used)]
    return self.inner.get_mut(&font_type).expect("FontMap: 指定された FontType が見つかりません");
  }

  /// `FontType::ALL` の順序で `(FontType, &T)` のペアを返すイテレータを取得します
  ///
  /// # Returns
  ///
  /// 19 フォント種別を定義順で反復するイテレータ
  #[must_use]
  pub fn iter(&self) -> FontMapIter<'_, T> {
    return FontMapIter {
      inner: &self.inner,
      index: 0,
    };
  }

  /// `FontType::ALL` の順序で `(FontType, &mut T)` のペアを返す可変イテレータを取得します
  ///
  /// # Returns
  ///
  /// 19 フォント種別を定義順で可変反復するイテレータ
  pub fn iter_mut(&mut self) -> FontMapIterMut<'_, T> {
    return FontMapIterMut {
      inner: &mut self.inner,
      index: 0,
    };
  }
}

/// `FontMap` の不変イテレータ
///
/// `FontType::ALL` の順序で `(FontType, &T)` を返します。
pub struct FontMapIter<'a, T> {
  inner: &'a HashMap<FontType, T>,
  index: usize,
}

impl<'a, T> Iterator for FontMapIter<'a, T> {
  type Item = (FontType, &'a T);

  fn next(&mut self) -> Option<Self::Item> {
    if self.index >= FontType::ALL.len() {
      return None;
    }
    let font_type = FontType::ALL[self.index];
    let value = &self.inner[&font_type];
    self.index += 1;
    return Some((font_type, value));
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = FontType::ALL.len().saturating_sub(self.index);
    return (remaining, Some(remaining));
  }
}

impl<T> ExactSizeIterator for FontMapIter<'_, T> {}

/// `FontMap` の可変イテレータ
///
/// `FontType::ALL` の順序で `(FontType, &mut T)` を返します。
pub struct FontMapIterMut<'a, T> {
  inner: &'a mut HashMap<FontType, T>,
  index: usize,
}

impl<'a, T> Iterator for FontMapIterMut<'a, T> {
  type Item = (FontType, &'a mut T);

  fn next(&mut self) -> Option<Self::Item> {
    if self.index >= FontType::ALL.len() {
      return None;
    }
    let font_type = FontType::ALL[self.index];
    self.index += 1;
    // SAFETY: 各 FontType は一意なので、同じキーに2回アクセスすることはない
    let value = self.inner.get_mut(&font_type)?;
    let value = unsafe { &mut *std::ptr::from_mut(value) };
    return Some((font_type, value));
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let remaining = FontType::ALL.len().saturating_sub(self.index);
    return (remaining, Some(remaining));
  }
}

impl<T> ExactSizeIterator for FontMapIterMut<'_, T> {}

impl<'a, T> IntoIterator for &'a FontMap<T> {
  type IntoIter = FontMapIter<'a, T>;
  type Item = (FontType, &'a T);

  fn into_iter(self) -> Self::IntoIter { return self.iter(); }
}

impl<'a, T> IntoIterator for &'a mut FontMap<T> {
  type IntoIter = FontMapIterMut<'a, T>;
  type Item = (FontType, &'a mut T);

  fn into_iter(self) -> Self::IntoIter { return self.iter_mut(); }
}

/// 見出しのレベル
///
/// LaTeX の見出しコマンドに対応する 6 段階の論理レベル。
/// `\part` を最上位とし、`\subparagraph` を最下位とする。
/// 見出し固有のフォント・余白・番号書式は `read_style::HeadingStyle` で持たせ、
/// このクレートではレベルの enum と基本変換のみを提供する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HeadingLevel {
  /// `\part` — 部（最上位の区分）
  Part = 0,
  /// `\chapter` — 章
  Chapter = 1,
  /// `\section` — 節
  Section = 2,
  /// `\subsection` — 小節
  Subsection = 3,
  /// `\paragraph` — 段落見出し
  Paragraph = 4,
  /// `\subparagraph` — 小段落見出し
  Subparagraph = 5,
}

impl HeadingLevel {
  /// 6 つのレベルすべてを宣言順で並べた配列
  pub const ALL: [HeadingLevel; 6] = [
    HeadingLevel::Part,
    HeadingLevel::Chapter,
    HeadingLevel::Section,
    HeadingLevel::Subsection,
    HeadingLevel::Paragraph,
    HeadingLevel::Subparagraph,
  ];
  /// `HeadingLevel::ALL` の要素数
  pub const COUNT: usize = 6;

  /// 数値インデックスを返す（0=Part, 5=Subparagraph）
  #[must_use]
  pub fn depth(self) -> u8 { return self as u8; }

  /// コマンド名からレベルを取得する
  #[must_use]
  pub fn from_command_name(name: &str) -> Option<Self> {
    return match name {
      "part" => Some(HeadingLevel::Part),
      "chapter" => Some(HeadingLevel::Chapter),
      "section" => Some(HeadingLevel::Section),
      "subsection" => Some(HeadingLevel::Subsection),
      "paragraph" => Some(HeadingLevel::Paragraph),
      "subparagraph" => Some(HeadingLevel::Subparagraph),
      _ => None,
    };
  }

  /// コマンド名を返す
  #[must_use]
  pub fn command_name(self) -> &'static str {
    return match self {
      HeadingLevel::Part => "part",
      HeadingLevel::Chapter => "chapter",
      HeadingLevel::Section => "section",
      HeadingLevel::Subsection => "subsection",
      HeadingLevel::Paragraph => "paragraph",
      HeadingLevel::Subparagraph => "subparagraph",
    };
  }
}

impl std::fmt::Display for HeadingLevel {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { return write!(f, "{}", self.command_name()); }
}
