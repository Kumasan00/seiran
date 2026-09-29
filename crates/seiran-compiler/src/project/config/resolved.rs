//! 検証・パス解決・正規化を終えた設定構造体

use std::path::PathBuf;

use crate::{
  length::Length,
  project::{FontConfigs, ProjectPath},
};

/// PDF 生成に必要な完全な設定情報
#[derive(Debug, Clone)]
pub(crate) struct ProjectConfig {
  /// ドキュメントメタデータ
  pub document: DocumentConfig,
  /// 出力ファイル名・ディレクトリ
  pub output: OutputConfig,
  /// 用紙寸法としおり出力（検証済み）
  pub pdf: PdfConfig,
  /// ラスタ画像のダウンサンプリング設定（検証済み）
  pub image: ImageConfig,
  /// 19 フォント種別すべての設定（検証済み）
  pub font_configs: FontConfigs,
  /// ソースファイル一覧（順次パースして 1 ドキュメントに結合。`PathResolver` で解決済み）
  pub sources: Vec<ProjectPath>,
  /// スタイル設定ファイルへのパス（オプション、解決済み）
  pub style_path: Option<ProjectPath>,
  /// 参照設定ファイルへのパス（オプション、解決済み）
  pub references_path: Option<ProjectPath>,
}

/// 文書のメタデータ（`date` 以外は PDF メタデータにも入る）
#[derive(Debug, Clone)]
pub(crate) struct DocumentConfig {
  /// ドキュメントタイトル（PDF メタデータの /Title）
  pub title: Option<String>,
  /// 著者名（PDF メタデータの /Author）
  pub author: Option<String>,
  /// 日付（表紙・走り文に表示する文字列。PDF メタデータには入らない）
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
  /// 出力ディレクトリ（相対指定は `base_dir` を前置済み。字句的な正規化はしない）
  pub output_dir: PathBuf,
}

impl OutputConfig {
  /// `{output_dir}/{name}.pdf` のパスを返す
  ///
  /// `set_extension` は使わない — `name` の最後の `.` 以降を拡張子とみなして置き換えるので、
  /// `report-v1.2` が `report-v1.pdf` になる。
  #[must_use]
  pub(crate) fn pdf_path(&self) -> PathBuf { return self.output_dir.join(format!("{}.pdf", self.name)); }
}

/// PDF ページの物理設定（用紙寸法と PDF 出力）の検証済み・処理済み設定
///
/// 用紙上のどこを本文領域にするか（4 方向の余白）は見た目なので `style.toml` の `[page]`
/// （`style::page::PageStyle`）が所有する。
#[derive(Debug, Clone)]
pub(crate) struct PdfConfig {
  /// ページの高さ
  pub height: Length,
  /// ページの幅
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

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::OutputConfig;

  #[test]
  fn pdf_path_appends_the_extension_even_when_the_name_contains_a_dot() {
    let output = OutputConfig {
      name: "report-v1.2".to_string(),
      output_dir: PathBuf::from("/project/out"),
    };

    assert_eq!(output.pdf_path(), PathBuf::from("/project/out/report-v1.2.pdf"));
  }
}
