//! フォント種別の定義
//!
//! PDF 生成全体で参照される最終的なフォント種別 [`FontType`]（19 種別）と、
//! 言語判定前のスタイル分類 [`FontKind`]（13 種別）をまとめて定義します。

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
/// [`FontType`] の 19 種別から、フォントのスタイル情報のみを抽出した 13 種別です。
/// テキスト処理では、まず `FontKind` でスタイル（Standard/Bold/Italic など）を決定し、
/// その後、テキストの言語（Latin か日本語か）を判定して、具体的な `FontType` を決定します。
///
/// # `FontType` との関係
///
/// - **[`FontType`]**（19 種別）: 最終的なフォント種別、言語とスタイルが確定した状態
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
