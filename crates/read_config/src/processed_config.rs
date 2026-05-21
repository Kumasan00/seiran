//! 処理済み・検証済み設定構造体
//!
//! このモジュールの構造体群は、TOML からデシリアライズされた後、
//! 以下の処理が完了した最終形式の設定を表します：
//!
//! - **パス解決**: 相対パスを絶対パスに、シンボリックリンクを解決
//! - **バリデーション**: すべての値が妥当な範囲内であることを確認
//! - **型変換**: スクリプト・OT 言語タグを `[u8; 4]`、BCP 47 言語を `String` に変換
//! - **正規化**: バリアブル軸、フィーチャータグを標準形式に統一
//!
//! アプリケーションはこれら処理済み設定を直接使用し、
//! バリデーションエラーの心配なく安全に処理できます。
//!
//! ## 処理済み状態の保証
//!
//! - **`Config`** - ドキュメント全体の設定（`lib.rs` で完全検証済み）
//! - **`FontConfigs`** - 19 フォント設定のコンテナ（値の正当性保証）
//! - **`FontConfig`** - 単一フォント設定（パス・値・型すべて検証済み）
//! - **`PdfConfig`** - PDF ページ設定（値の正当性保証）
//! - **`VariationAxis`** - バリアブル軸（軸値の範囲内性確認済み）
//! - **`Feature`** - OpenType フィーチャー（タグ長・値の妥当性確認済み）
//! - **`Margin`** - ページ余白（非負値・合計妥当性確認済み）

use std::path::PathBuf;

use types::FontMap;

/// PDF 生成に必要な完全な設定情報
///
/// すべてのパス、バリデーション、型変換が完了した最終形式の設定です。
/// [`crate::read_config`] で生成され、すべての値が検証済みであることが保証されます。
///
/// アプリケーションはこの構造体から設定を読み取り、PDF 生成パイプラインに渡します。
#[derive(Debug, Clone)]
pub struct Config {
  /// ドキュメントメタデータ（title / author / date / subject）
  pub document: DocumentConfig,
  /// 出力ファイル名・ディレクトリ
  pub output: OutputConfig,
  /// PDF ページレイアウト設定（検証済み）
  pub pdf: PdfConfig,
  /// 19 フォント種別すべての設定（検証済み）
  pub font_configs: FontConfigs,
  /// ソースファイル一覧（順次パースして 1 ドキュメントに結合、絶対パス正規化済み）
  pub sources: Vec<PathBuf>,
  /// スタイル設定ファイルへのパス（オプション、正規化済み）
  pub style_path: Option<PathBuf>,
  /// 参照設定ファイルへのパス（オプション、正規化済み）
  pub references_path: Option<PathBuf>,
}

/// PDF メタデータ
#[derive(Debug, Clone)]
pub struct DocumentConfig {
  /// ドキュメントタイトル（PDF メタデータの /Title）
  pub title: Option<String>,
  /// 著者名（PDF メタデータの /Author）
  pub author: Option<String>,
  /// 日付（ISO 8601 形式想定。PDF 出力時に必要に応じて D:YYYYMMDD 形式に変換）
  pub date: Option<String>,
  /// 主題（PDF メタデータの /Subject）
  pub subject: Option<String>,
}

/// 出力ファイル名・ディレクトリ
#[derive(Debug, Clone)]
pub struct OutputConfig {
  /// 出力ファイル名の基盤（拡張子なし。実際の PDF パスは `{output_dir}/{name}.pdf`）
  pub name: String,
  /// 出力ディレクトリの絶対パス（正規化済み）
  pub output_dir: PathBuf,
}

impl OutputConfig {
  /// `{output_dir}/{name}.pdf` の絶対パスを返す
  #[must_use]
  pub fn pdf_path(&self) -> PathBuf {
    let mut path = self.output_dir.clone();
    path.push(&self.name);
    path.set_extension("pdf");
    return path;
  }
}

/// 19 フォント種別すべての検証済み設定
///
/// PDF 生成に使用される 19 種類のフォント（Latin 12 + Math 1 + 日本語 6）の
/// 設定をまとめて管理します。各フォント種別は独立した `FontConfig` を持ち、
/// すべてのパス、値、型が検証済みです。
///
/// 内部的には [`FontMap<FontConfig>`] を使用しており、
/// [`iter()`](FontConfigs::iter) や [`get()`](FontConfigs::get) メソッドで
/// 効率よくアクセスできます。
pub type FontConfigs = FontMap<FontConfig>;

/// 単一フォント種別の検証済み・処理済み設定
///
/// パス正規化、バリデーション、型変換がすべて完了した状態です。
/// 以下が保証されます：
/// - `font_path` は絶対パスに正規化されている
/// - `font_index` は 0 以上の値
/// - `script`、`feature` タグは 4 文字の `[u8; 4]` に変換済み
/// - `language` は BCP 47 として妥当な文字列、または `None`
/// - `ot_language_tag` は 4 バイトに正規化済み（3 文字指定時は末尾スペースパディング）、または `None`
/// - 値の妥当性はすべてバリデーション済み
#[derive(Debug, Clone)]
pub struct FontConfig {
  /// `PDF FontDescriptor` で使用されるフォント名
  /// 19 フォント種別間で一意である必要があります
  pub font_name: String,
  /// フォントファイルへの絶対パス（正規化済み）
  /// シンボリックリンク解決済み、ファイル存在確認済み
  pub font_path: PathBuf,
  /// TTC（TrueType Collection）ファイル内のインデックス
  /// 通常は 0。複数フォントを含むコレクションの場合は 1 以上
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  /// 値が範囲内であることはバリデーション済み
  pub variation_axes: Option<Vec<VariationAxis>>,
  /// OpenType Script タグ（4 バイト、ISO 15924）
  /// 例：b"latn"（Latin）、b"arab"（Arabic）。ユーザの `script` 指定をそのまま反映
  pub script: Option<[u8; 4]>,
  /// BCP 47 言語タグ（harfrust の [`Language::from_str`] に渡す最終文字列）
  ///
  /// ユーザが `language` で指定した BCP 47 タグに加え、`ot_language` 指定時は
  /// 末尾に `-x-hbot<XXXX>` 予約サブタグを連結した形式となる
  /// （`language` 未指定で `ot_language` のみ指定された場合は `"und-x-hbot<XXXX>"`）。
  /// 例：`"ja-JP"`, `"und-x-hbotJAN "`, `"en-x-hbotENG "`
  ///
  /// [`Language::from_str`]: https://docs.rs/harfrust/latest/harfrust/struct.Language.html
  pub language: Option<String>,
  /// OpenType 言語システムタグ（4 バイトに正規化済み、3 文字指定時は末尾スペース）
  ///
  /// ユーザが `ot_language` を明示指定した場合のみ `Some`。GSUB/GPOS の言語サブテーブル
  /// 参照（`font` クレートの `validate_font`）に使用します。BCP 47 のみ指定時は `None` で、
  /// harfrust の内部処理に OT 言語タグ導出を委ねます。
  pub ot_language_tag: Option<[u8; 4]>,
  /// OpenType フィーチャー設定（4 バイトタグ + 値）
  /// 例："liga"（ligatures）、"smcp"（small capitals）
  pub features: Option<Vec<Feature>>,
}

/// OpenType フィーチャーの設定（タグと値のペア）
///
/// フォントで有効にするシェイピング機能を指定します。
/// タグは 4 バイトの OpenType フィーチャータグです（例：`b"liga"` = ligatures、`b"smcp"` = small capitals）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
  /// OpenType フィーチャータグ（4 バイト）
  pub tag: [u8; 4],
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// バリアブルフォント軸の設定値
///
/// 変数フォントの特定の軸を目標値に設定します。
/// 複数の軸を組み合わせることで、フォントの特定バリエーション
/// （例：Weight=700、Width=80）を選択できます。
#[derive(Debug, Clone, Copy)]
pub struct VariationAxis {
  /// 軸名（4 バイトの OpenType 軸タグ）
  /// 例：b"wght"（weight）、b"wdth"（width）
  pub name: [u8; 4],
  /// 目標値（実数）
  /// 例：weight 軸で 700（太字）、width 軸で 80（condensed）
  pub value: f64,
}

/// PDF ページレイアウトの検証済み・処理済み設定
///
/// ページサイズ、余白、フォント設定など、
/// PDF 出力全体のレイアウトを制御します。
/// すべての値が正の値・非負値として検証済みです。
///
/// 出力先パスは `Config::output` を参照（`OutputConfig::pdf_path()`）。
#[derive(Debug, Clone)]
pub struct PdfConfig {
  /// ページの高さ（mm）
  /// バリデーション済み（> 0）、余白と矛盾なし
  pub height: f32,
  /// ページの幅（mm）
  /// バリデーション済み（> 0）、余白と矛盾なし
  pub width: f32,
  /// ページ余白（上下左右）
  pub margin: Margin,
}

/// ページ余白（上下左右）
///
/// ページ内の有効テキスト配置領域を定義します。
/// すべて非負値（>= 0）で、合計がページサイズ未満であることが保証されます。
#[derive(Debug, Clone, Copy)]
pub struct Margin {
  /// 上余白（mm）（バリデーション済み、>= 0）
  pub top: f32,
  /// 下余白（mm）（バリデーション済み、>= 0）
  pub bottom: f32,
  /// 左余白（mm）（バリデーション済み、>= 0）
  pub left: f32,
  /// 右余白（mm）（バリデーション済み、>= 0）
  pub right: f32,
}
