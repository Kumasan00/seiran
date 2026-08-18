//! 処理済み・検証済み設定構造体

use std::path::PathBuf;

use crate::{length::Length, project::FontConfigs};

/// PDF 生成に必要な完全な設定情報
#[derive(Debug, Clone)]
pub(crate) struct ProjectConfig {
  /// ドキュメントメタデータ（title / author / date / subject）
  pub document: DocumentConfig,
  /// 出力ファイル名・ディレクトリ
  pub output: OutputConfig,
  /// PDF ページレイアウト設定（検証済み）
  pub pdf: PdfConfig,
  /// ラスタ画像のダウンサンプリング設定（検証済み）
  pub image: ImageConfig,
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
pub(crate) struct DocumentConfig {
  /// ドキュメントタイトル（PDF メタデータの /Title）
  pub title: Option<String>,
  /// 著者名（PDF メタデータの /Author）
  pub author: Option<String>,
  /// 日付（ISO 8601 形式想定。PDF 出力時に必要に応じて D:YYYYMMDD 形式に変換）
  pub date: Option<String>,
  /// 主題（PDF メタデータの /Subject）
  pub subject: Option<String>,
  /// ドキュメント全体の言語（BCP 47、PDF メタデータの /Lang）
  pub language: Option<String>,
  /// キーワード（PDF メタデータの /Keywords）
  pub keywords: Option<Vec<String>>,
}

/// 出力ファイル名・ディレクトリ
#[derive(Debug, Clone)]
pub(crate) struct OutputConfig {
  /// 出力ファイル名の基盤（拡張子なし。実際の PDF パスは `{output_dir}/{name}.pdf`）
  pub name: String,
  /// 出力ディレクトリの絶対パス（正規化済み）
  pub output_dir: PathBuf,
}

impl OutputConfig {
  /// `{output_dir}/{name}.pdf` の絶対パスを返す
  #[must_use]
  pub(crate) fn pdf_path(&self) -> PathBuf {
    let mut path = self.output_dir.clone();
    path.push(&self.name);
    path.set_extension("pdf");
    return path;
  }
}

/// PDF ページの物理設定（用紙寸法と PDF 出力）の検証済み・処理済み設定
///
/// 用紙上のどこを本文領域にするか（4 方向の余白）は見た目なので `style.toml` の `[page]`
/// （[`crate::style::PageStyle`]）が所有する（#389）。
#[derive(Debug, Clone)]
pub(crate) struct PdfConfig {
  /// ページの高さ（[`Length`]）
  pub height: Length,
  /// ページの幅（[`Length`]）
  pub width: Length,
  /// PDF のしおり（ブックマーク）を出力するか（既定 true）
  pub show_bookmarks: bool,
}

/// ラスタ画像のダウンサンプリング設定（検証済み）
#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageConfig {
  /// ラスタ画像埋め込み時の最大 DPI（バリデーション済み、1〜2400）
  pub max_dpi: u32,
  /// ラスタ画像のダウンサンプリングを行うか
  pub downsample: bool,
}
