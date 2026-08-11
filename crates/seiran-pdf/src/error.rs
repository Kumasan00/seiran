//! PDF 描画時のエラーを定義する。
//!
//! ここにあるのは「有効な [`Publication`](seiran_compiler::Publication) を渡されたうえで、なお
//! backend が失敗しうるもの」だけ（#378）。3 系統に限る:
//!
//! 1. 有効な `Publication` を backend の表現へ変換できない（krilla フォントの構築）
//! 2. 画像デコーダ / SVG renderer が入力を処理できない
//! 3. krilla が文書を最終化できない
//!
//! 描画命令の値そのものが壊れている（ページサイズが 0、矩形の幅が負、資源に無い画像を指す等）
//! ケースは含まない — `Publication` のコンストラクタが構築時に弾いており、renderer 側では
//! `unreachable!` で顕在化させる。

use krilla::error::KrillaError;
use miette::Diagnostic;
use read_fonts::ReadError;
use seiran_compiler::FontType;
use thiserror::Error;

/// PDF 描画中に発生するエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum PdfRenderError {
  /// フォントバイト列を krilla へ渡す前の解析に失敗しました。
  #[error("フォントバイト列の解析に失敗しました: {font_type:?}")]
  #[diagnostic(
    code(pdf::font_parse),
    help("描画バックエンドがフォントを解釈できませんでした。別のフォントファイルで出力できるか確認してください。")
  )]
  FontParse {
    /// フォント種別。
    font_type: FontType,
    /// 元の解析エラー。
    #[source]
    source: ReadError,
  },
  /// バリアブルフォントの補助テーブルを読み込めませんでした。
  #[error("バリアブルフォントの補助テーブルを読み込めませんでした: {font_type:?}")]
  #[diagnostic(
    code(pdf::variation_table_read),
    help("描画バックエンドが 'fvar' テーブルを読めませんでした。別のフォントファイルで出力できるか確認してください。")
  )]
  VariationTableRead {
    /// フォント種別。
    font_type: FontType,
    /// 元の読み込みエラー。
    #[source]
    source: ReadError,
  },
  /// krilla フォントの生成に失敗しました。
  #[error("Krilla 用フォントの生成に失敗しました: {font_type:?}")]
  #[diagnostic(
    code(pdf::font_creation),
    help(
      "描画バックエンドがこのフォントを PDF へ埋め込めませんでした。別のフォントファイルで出力できるか確認してください。"
    )
  )]
  FontCreation {
    /// フォント種別。
    font_type: FontType,
  },
  /// SVG のパースに失敗しました。
  ///
  /// 組版段（`seiran_compiler` の `typeset::image`）は同じ `usvg` で自然寸法を取るだけなので、
  /// 描画用のパースがここで別に失敗しうる。
  #[error("SVG のパースに失敗しました: {path}")]
  #[diagnostic(code(pdf::parse_svg), help("SVG ファイルが妥当な XML / SVG であることを確認してください。"))]
  ParseSvg {
    /// 画像ファイルのパス。
    path: String,
    /// 元の usvg エラー。
    #[source]
    source: usvg::Error,
  },
  /// SVG のレンダリングに失敗しました。
  #[error("SVG のレンダリングに失敗しました: {path}")]
  #[diagnostic(
    code(pdf::draw_svg),
    help("krilla-svg が SVG の描画を完了できませんでした。SVG の内容を確認してください。")
  )]
  DrawSvg {
    /// 画像ファイルのパス。
    path: String,
  },
  /// 画像のデコードに失敗しました。
  ///
  /// 組版段は寸法ヘッダしか読まないため、ヘッダは妥当でも本体が壊れた画像はここで初めて分かる。
  #[error("画像のデコードに失敗しました: {path} ({reason})")]
  #[diagnostic(code(pdf::decode_image), help("画像ファイルが破損していないか確認してください。"))]
  DecodeImage {
    /// 画像ファイルのパス。
    path: String,
    /// krilla が返したメッセージ。
    reason: String,
  },
  /// ラスタ画像のダウンサンプリング時の処理（デコード / リサイズ / 再エンコード）に失敗しました。
  #[error("画像のリサイズに失敗しました: {path}")]
  #[diagnostic(
    code(pdf::resize_image),
    help("元画像が破損していないか、または \\image[downsample=false] でリサイズを無効化してください。")
  )]
  ResizeImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の image クレートのエラー。
    #[source]
    source: image::ImageError,
  },
  /// PDF の最終化に失敗しました。
  #[error("PDF の最終化に失敗しました: {source}")]
  #[diagnostic(code(pdf::finalize_document), help("Krilla が PDF を書き出せませんでした。"))]
  FinalizeDocument {
    /// 元の生成エラー。
    #[source]
    source: KrillaError,
  },
}
