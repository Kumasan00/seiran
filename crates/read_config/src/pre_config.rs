//! TOML ファイルからのデシリアライズ用設定構造体
//!
//! このモジュールは、TOML ファイルの形式に直接対応した構造体群を定義します。
//! `serde` による自動デシリアライズを用いて、TOML テキストを構造化データに変換する
//! 中間層（プリプロセス層）として機能します。
//!
//! ## 処理フロー
//!
//! ```text
//! config.toml（TOML テキスト）
//!   ↓
//! toml::from_slice()（デシリアライズ）
//!   ↓
//! PreConfig（TOML そのものの構造）
//!   ↓ + パス解決・バリデーション
//! processed_config::Config（処理済み・検証済み設定）
//! ```
//!
//! ## TOML ファイルの全体構造
//!
//! ```toml
//! name = "my_document"
//!
//! [pdf]
//! output_dir = "./output"
//! height = 297.0
//! width = 210.0
//! font_size = 12.0
//! line_height_factor = 1.5
//! margin_top = 20.0
//! margin_bottom = 20.0
//! margin_left = 20.0
//! margin_right = 20.0
//! background_r = 1.0
//! background_g = 1.0
//! background_b = 1.0
//!
//! [font_configs.serif]
//! font_name = "Noto Serif"
//! font_path = "./fonts/NotoSerif-Regular.ttf"
//! font_index = 0  # オプション
//! script = "latn"  # オプション
//! language = "eng"  # オプション
//!
//! [[font_configs.serif.features]]
//! tag = "liga"
//! value = 1
//!
//! # ... （他の 18 フォント種別）
//! ```
//!
//! ## 構造体の役割
//!
//! - [`PreConfig`] - TOML ファイル全体（最上位ノード）
//! - [`PrePdfConfig`] - PDF 設定セクション
//! - [`PreFontConfigs`] - 19 フォント設定群のコンテナ
//! - [`PreFontConfig`] - 単一フォント種別の設定
//! - [`PreVariationAxis`] - バリアブルフォント軸の設定値
//! - [`PreFontFeature`] - OpenType フィーチャータグと値
//!
//! ## 重要な注意点
//!
//! - **バリデーションなし**: このモジュールの構造体はバリデーションを行いません
//! - **変換なし**: パスやコード値の変換も行いません
//! - **ロジックなし**: TOML の形状に完全に投影した、データ保持のみの層です
//! - **検証・変換は `lib.rs` で実施**: パス正規化、バリデーション、型変換は親モジュールで実行

use std::path::PathBuf;

use serde::Deserialize;
use types::FontType;

/// TOML ファイル全体をデシリアライズした設定
///
/// TOML ファイルの最上位ノード構造を表します。
/// ここからは生データであり、バリデーションと変換は行われていません。
///
/// ## TOML での表現
///
/// ```toml
/// name = "my_document"
/// [pdf]
/// # ...
/// [font_configs]
/// # ...
/// ```
#[derive(Deserialize, Debug)]
pub(crate) struct PreConfig {
  /// ドキュメント名（PDF ファイル名の基盤）
  pub name: String,
  /// PDF ページ設定
  pub pdf: PrePdfConfig,
  /// 19 フォント種別の設定群
  pub font_configs: PreFontConfigs,
}

/// 19 フォント種別すべてのプリプロセス設定
///
/// `[font_configs]` セクション下の 19 種類のフォント設定を保持します。
/// 各フォント種別の設定は独立した `PreFontConfig` として格納されます。
///
/// ## TOML での表現
///
/// ```toml
/// [font_configs.serif]
/// [font_configs.serif_bold]
/// # ... 他の 17 フォント
/// ```
///
/// ## フォント種別一覧
///
/// - **Latin 体（12 種類）**
///   - `serif` × 4（標準、太字、イタリック、太字イタリック）
///   - `sans_serif` × 4（標準、太字、イタリック、太字イタリック）
///   - `monospace` × 4（標準、太字、イタリック、太字イタリック）
/// - **特殊（1 種類）**
///   - `math`（数式用）
/// - **日本語（6 種類）**
///   - `japanese_serif` × 2（標準、太字）
///   - `japanese_sans_serif` × 2（標準、太字）
///   - `japanese_monospace` × 2（標準、太字）
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfigs {
  /// Serif 標準フォント
  pub serif: PreFontConfig,
  /// Serif 太字フォント
  pub serif_bold: PreFontConfig,
  /// Serif イタリックフォント
  pub serif_italic: PreFontConfig,
  /// Serif 太字イタリックフォント
  pub serif_bold_italic: PreFontConfig,
  /// Sans Serif 標準フォント
  pub sans_serif: PreFontConfig,
  /// Sans Serif 太字フォント
  pub sans_serif_bold: PreFontConfig,
  /// Sans Serif イタリックフォント
  pub sans_serif_italic: PreFontConfig,
  /// Sans Serif 太字イタリックフォント
  pub sans_serif_bold_italic: PreFontConfig,
  /// Monospace 標準フォント
  pub monospace: PreFontConfig,
  /// Monospace 太字フォント
  pub monospace_bold: PreFontConfig,
  /// Monospace イタリックフォント
  pub monospace_italic: PreFontConfig,
  /// Monospace 太字イタリックフォント
  pub monospace_bold_italic: PreFontConfig,
  /// 数式用フォント
  pub math: PreFontConfig,
  /// 日本語 Serif 標準フォント
  pub japanese_serif: PreFontConfig,
  /// 日本語 Serif 太字フォント
  pub japanese_serif_bold: PreFontConfig,
  /// 日本語 Sans Serif 標準フォント
  pub japanese_sans_serif: PreFontConfig,
  /// 日本語 Sans Serif 太字フォント
  pub japanese_sans_serif_bold: PreFontConfig,
  /// 日本語 Monospace 標準フォント
  pub japanese_monospace: PreFontConfig,
  /// 日本語 Monospace 太字フォント
  pub japanese_monospace_bold: PreFontConfig,
}

impl PreFontConfigs {
  /// フォント種別に対応する `PreFontConfig` を取得します。
  pub fn get(&self, font_type: FontType) -> &PreFontConfig {
    match font_type {
      FontType::Serif => &self.serif,
      FontType::SerifBold => &self.serif_bold,
      FontType::SerifItalic => &self.serif_italic,
      FontType::SerifBoldItalic => &self.serif_bold_italic,
      FontType::SansSerif => &self.sans_serif,
      FontType::SansSerifBold => &self.sans_serif_bold,
      FontType::SansSerifItalic => &self.sans_serif_italic,
      FontType::SansSerifBoldItalic => &self.sans_serif_bold_italic,
      FontType::Monospace => &self.monospace,
      FontType::MonospaceBold => &self.monospace_bold,
      FontType::MonospaceItalic => &self.monospace_italic,
      FontType::MonospaceBoldItalic => &self.monospace_bold_italic,
      FontType::Math => &self.math,
      FontType::JapaneseSerif => &self.japanese_serif,
      FontType::JapaneseSerifBold => &self.japanese_serif_bold,
      FontType::JapaneseSansSerif => &self.japanese_sans_serif,
      FontType::JapaneseSansSerifBold => &self.japanese_sans_serif_bold,
      FontType::JapaneseMonospace => &self.japanese_monospace,
      FontType::JapaneseMonospaceBold => &self.japanese_monospace_bold,
    }
  }
}

/// 単一フォント種別のプリセット設定情報
///
/// 各フォント種別に対する完全な設定を保持します。
/// このデータは後に `processed_config::FontConfig` に変換され、
/// パス正規化、バリデーション、型変換が行われます。
///
/// ## TOML での表現
///
/// ```toml
/// [font_configs.serif]
/// font_name = "Noto Serif"
/// font_path = "./fonts/NotoSerif-Regular.ttf"
/// font_index = 0  # オプション、デフォルト: 0
/// script = "latn"  # オプション、OpenType script tag
/// language = "eng"  # オプション、BCP 47 language
///
/// [font_configs.serif.variation_axes]
/// weight = 400
///
/// [[font_configs.serif.features]]
/// tag = "liga"
/// value = 1
/// ```
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontConfig {
  /// `PDF FontDescriptor` での基本フォント名（各フォント種別で一意）
  pub font_name: String,
  /// フォントファイルへのパス（相対または絶対）
  /// 後に正規化されて絶対パスに変換されます
  pub font_path: PathBuf,
  /// TTC（TrueType Collection）ファイル内のフォントインデックス
  /// デフォルトは 0（最初のフォント）
  pub font_index: Option<u32>,
  /// バリアブルフォント軸の設定値配列
  /// 例：weight = 700、width = 80 など
  pub variation_axes: Option<Vec<PreVariationAxis>>,
  /// OpenType Script Tag（ISO 15924 コード、4 文字）
  /// 例："latn"（Latin）、"arab"（Arabic）、"cyrl"（Cyrillic）
  pub script: Option<String>,
  /// BCP 47 言語タグ（3 or 4 文字）
  /// 例："eng"（English）、"ja"（日本語）、"ko"（韓国語）
  pub language: Option<String>,
  /// OpenType フィーチャー設定配列
  /// 例："liga"（ligatures）、"smcp"（small capitals）など
  pub features: Option<Vec<PreFontFeature>>,
}

/// バリアブルフォント軸の単一設定値
///
/// 軸名と目標値のペアを表します。
/// 複数の軸を組み合わせることで、変数フォントの特定のバリエーションを選択します。
///
/// ## TOML での表現
///
/// ```toml
/// [font_configs.serif.variation_axes]
/// weight = 700      # 太さを 700（太字）に設定
/// width = 80        # 幅を 80%（Condensed）に設定
/// slnt = -12        # スラント角を -12 に設定
/// ```
///
/// ## 標準軸名（OpenType Registry）
///
/// | 軸名 | 説明 | 典型値 |
/// |------|------|--------|
/// | weight | 太さ | 100-900 |
/// | width | 横幅 | 50-200 |
/// | slant | スラント | -90-0 |
/// | opsz | 光学サイズ | 6-72 |
/// | GRAD | グラデーション | -200-150 |
/// | XTRA | x-トランスペアレント | 0-100 |
#[derive(Deserialize, Debug)]
pub(crate) struct PreVariationAxis {
  /// 軸名（4 文字の OpenType 軸タグ、例："wght"、"wdth"）
  pub name: String,
  /// 軸の目標値（実数）
  /// 後にバリデーションされ、フォントの最小値〜最大値の範囲内にあることが確認されます
  pub value: f64,
}

/// OpenType フィーチャータグと値のペア
///
/// フォント内で利用可能な Advanced Typography 機能（フィーチャー）を有効化/無効化します。
/// 複数のフィーチャーを組み合わせることで、細かいタイポグラフィ制御が可能になります。
///
/// ## TOML での表現
///
/// ```toml
/// [[font_configs.serif.features]]
/// tag = "liga"  # Ligatures を有効化
/// value = 1
///
/// [[font_configs.serif.features]]
/// tag = "smcp"  # Small capitals を有効化
/// value = 1
/// ```
///
/// ## よく使われるフィーチャータグ
///
/// | タグ | 説明 | 値 |
/// |------|------|-----|
/// | liga | リガチャ（fi → fi） | 0/1 |
/// | dlig | 随意リガチャ | 0/1 |
/// | smcp | 小文字大文字化 | 0/1 |
/// | c2sc | 大文字小文字化対応 | 0/1 |
/// | zero | スラッシュ付きゼロ | 0/1 |
/// | kern | カーニング | 0/1 |
/// | onum | 旧体数字 | 0/1 |
#[derive(Deserialize, Debug)]
pub(crate) struct PreFontFeature {
  /// フィーチャータグ（4 文字、例："liga"、"smcp"、"dlig"）
  /// 後に長さが 4 文字であることが検証されます
  pub tag: String,
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// PDF ページレイアウトのプリセット設定
///
/// PDF ページサイズ、余白、フォント設定、背景色など、
/// 出力 PDF のレイアウト全体を制御する設定を保持します。
///
/// ## TOML での表現
///
/// ```toml
/// [pdf]
/// output_dir = "./output"
/// height = 297.0          # ページ高さ（mm、A4）
/// width = 210.0           # ページ幅（mm、A4）
/// font_size = 12.0        # 基本フォント高さ
/// line_height_factor = 1.5  # 行間の倍率
/// margin_top = 20.0       # 上余白
/// margin_bottom = 20.0    # 下余白
/// margin_left = 20.0      # 左余白
/// margin_right = 20.0     # 右余白
/// background_r = 1.0      # 背景色 R（オプション）
/// background_g = 1.0      # 背景色 G（オプション）
/// background_b = 1.0      # 背景色 B（オプション）
/// ```
///
/// ## よくあるページサイズ例
///
/// | サイズ | 高さ | 幅 |
/// |--------|------|-----|
/// | A4 | 297.0 | 210.0 |
/// | Letter | 279.4 | 215.9 |
/// | A5 | 210.0 | 148.0 |
#[derive(Deserialize, Debug)]
pub(crate) struct PrePdfConfig {
  /// 出力ディレクトリ（PDF ファイルの保存先）
  pub output_dir: PathBuf,
  /// ページの高さ（mm 単位）
  /// 後に正の値であることが検証されます
  pub height: f32,
  /// ページの幅（mm 単位）
  /// 後に正の値であることが検証されます
  pub width: f32,
  /// デフォルトのフォントサイズ（ポイント）
  /// 後に正の値であることが検証されます
  pub font_size: f32,
  /// 行の高さの倍率（1.0 = シングルスペース、1.5 = 1.5 行分）
  /// 後に正の値であることが検証されます
  pub line_height_factor: f32,
  /// 上余白（mm 単位）
  /// 後に非負値であることが検証されます
  pub margin_top: f32,
  /// 下余白（mm 単位）
  /// 後に非負値であることが検証されます
  pub margin_bottom: f32,
  /// 左余白（mm 単位）
  /// 後に非負値であることが検証されます
  pub margin_left: f32,
  /// 右余白（mm 単位）
  /// 後に非負値であることが検証されます
  pub margin_right: f32,
  /// 背景色の赤成分（0.0-1.0、オプション）
  /// 値がない場合、背景色は設定されません
  pub background_r: Option<f32>,
  /// 背景色の緑成分（0.0-1.0、オプション）
  /// 値がない場合、背景色は設定されません
  pub background_g: Option<f32>,
  /// 背景色の青成分（0.0-1.0、オプション）
  /// 値がない場合、背景色は設定されません
  pub background_b: Option<f32>,
}
