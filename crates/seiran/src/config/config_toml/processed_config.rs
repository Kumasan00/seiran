//! 処理済み・検証済み設定構造体

use std::{path::PathBuf, str::FromStr};

use model::{FontMap, Length};
use thiserror::Error;

/// PDF 生成に必要な完全な設定情報
#[derive(Debug, Clone)]
pub struct Config {
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
pub struct DocumentConfig {
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

/// PDF ページレイアウトの検証済み・処理済み設定
#[derive(Debug, Clone)]
pub struct PdfConfig {
  /// ページの高さ（[`Length`]）
  pub height: Length,
  /// ページの幅（[`Length`]）
  pub width: Length,
  /// ページ余白（上下左右）
  pub margin: Margin,
  /// PDF のしおり（ブックマーク）を出力するか（既定 true）
  pub show_bookmarks: bool,
}

/// ラスタ画像のダウンサンプリング設定（検証済み）
#[derive(Debug, Clone, Copy)]
pub struct ImageConfig {
  /// ラスタ画像埋め込み時の最大 DPI（バリデーション済み、1〜2400）
  pub max_dpi: u32,
  /// ラスタ画像のダウンサンプリングを行うか
  pub downsample: bool,
}

/// ページ余白（上下左右）
#[derive(Debug, Clone, Copy)]
pub struct Margin {
  /// 上余白
  pub top: Length,
  /// 下余白
  pub bottom: Length,
  /// 左余白
  pub left: Length,
  /// 右余白
  pub right: Length,
}

/// 19 フォント種別すべての検証済み設定
pub type FontConfigs = FontMap<FontConfig>;

/// 単一フォント種別の検証済み・処理済み設定
#[derive(Debug, Clone)]
pub struct FontConfig {
  /// `PDF FontDescriptor` で使用する想定のフォント名（現状未使用）。
  ///
  /// `pdf_gen::types::FontFaceInput`（#307 で self-contained 化、旧 #276/#279 由来）が
  /// この値を受け取るフィールドを持たないため、書き込まれるだけで読まれていない。
  /// `config` crate が standalone だった間は `pub` フィールドとして外部消費を仮定でき
  /// `dead_code` を検出されなかったが、`seiran` の非公開 module へ吸収され可視性が変わった
  /// ことで検出されるようになった（削除はデータモデル変更のため本 move task の範囲外、
  /// 削除するかどうかは要 controller 判断のクリーンアップ候補）。
  #[allow(dead_code)]
  pub font_name: String,
  /// フォントファイルへの絶対パス（正規化済み）
  pub font_path: PathBuf,
  /// TTC（TrueType Collection）ファイル内のインデックス
  pub font_index: u32,
  /// バリアブルフォント軸の設定値
  pub variation_axes: Option<Vec<VariationAxis>>,
  /// OpenType / ISO 15924 script タグ（4 バイト、ユーザ指定の case をそのまま反映）
  ///
  /// `b"DFLT"` は harfrust で `dflt` に変換されるため、DFLT fallback を使う場合は指定を省略する。
  pub script: Option<[u8; 4]>,
  /// BCP 47 言語タグ（harfrust の [`Language::from_str`] に渡す最終文字列）
  ///
  /// `ot_language` 指定時は末尾に `-x-hbot<XXXX>` 予約サブタグを連結する
  /// （`language` 未指定で `ot_language` のみ指定された場合は `"und-x-hbot<XXXX>"`）。
  pub language: Option<String>,
  /// OpenType 言語システムタグ（4 バイトに正規化済み、3 文字指定時は末尾スペース）
  pub ot_language_tag: Option<[u8; 4]>,
  /// 書字方向（horizontal LTR/RTL、vertical TTB/BTB）
  ///
  /// `None` の場合は入力テキストから自動判定する。
  pub direction: Option<TextDirection>,
  /// OpenType フィーチャー設定（4 バイトタグ + 値）
  /// 例："liga"（ligatures）、"smcp"（small capitals）
  pub features: Option<Vec<Feature>>,
}

/// 書字方向（`harfrust::Direction` の Invalid 以外にマップ）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextDirection {
  /// 左から右（horizontal、ラテン文字・日本語横書き等）
  LeftToRight,
  /// 右から左（horizontal、アラビア文字・ヘブライ文字等）
  RightToLeft,
  /// 上から下（vertical、日本語縦書き等）
  TopToBottom,
  /// 下から上（vertical、極めて稀）
  BottomToTop,
}

/// [`TextDirection`] の `FromStr` 実装が受理しない方向文字列を渡されたときのエラー。
#[derive(Debug, Error)]
#[error(
  "direction は 'left-to-right' / 'right-to-left' / 'top-to-bottom' / 'bottom-to-top' のいずれかである必要があります"
)]
pub struct TextDirectionParseError;

impl FromStr for TextDirection {
  type Err = TextDirectionParseError;

  /// 書字方向文字列を [`TextDirection`] に変換します。
  fn from_str(value: &str) -> Result<Self, Self::Err> {
    return match value {
      "left-to-right" => Ok(Self::LeftToRight),
      "right-to-left" => Ok(Self::RightToLeft),
      "top-to-bottom" => Ok(Self::TopToBottom),
      "bottom-to-top" => Ok(Self::BottomToTop),
      _ => Err(TextDirectionParseError),
    };
  }
}

/// OpenType フィーチャーの設定（タグと値のペア）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Feature {
  /// OpenType フィーチャータグ（4 バイト）
  pub tag: [u8; 4],
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// バリアブルフォント軸の設定値
#[derive(Debug, Clone, Copy)]
pub struct VariationAxis {
  /// 軸名（4 バイトの OpenType 軸タグ）
  pub name: [u8; 4],
  /// 目標値（実数）
  pub value: f64,
}
