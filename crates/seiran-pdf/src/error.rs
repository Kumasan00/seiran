//! PDF 生成時のエラーを定義する。

use krilla::error::KrillaError;
use miette::Diagnostic;
use read_fonts::ReadError;
use thiserror::Error;

use crate::types::FontType;

/// PDF 生成中に発生するエラー。
#[derive(Debug, Error, Diagnostic)]
pub enum PdfGenError {
  /// バリアブルフォントに必要な軸設定が不足しています。
  #[error("バリアブルフォント {font_type:?} に variation_axes が指定されていません")]
  #[diagnostic(code(seiran_pdf::missing_variation_axes), help("設定ファイルに variation_axes を追加してください。"))]
  MissingVariationAxes {
    /// フォント種別。
    font_type: FontType,
  },
  /// フォントの生成に失敗しました。
  #[error("Krilla 用フォントの生成に失敗しました: {font_type:?}")]
  #[diagnostic(
    code(seiran_pdf::font_creation),
    help("font_index や variation_axes の設定、または入力フォントの妥当性を確認してください。")
  )]
  FontCreation {
    /// フォント種別。
    font_type: FontType,
  },
  /// フォントバイト列の解析に失敗しました（呼び出し元がすでに検証済みのはずの内部エラー）。
  #[error("フォントバイト列の解析に失敗しました: {font_type:?}")]
  #[diagnostic(
    code(seiran_pdf::font_parse),
    help("フォントファイルが破損していないか、font_index が正しいか確認してください。")
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
    code(seiran_pdf::variation_table_read),
    help("入力フォントが壊れていないか、font_index が正しいかを確認してください。")
  )]
  VariationTableRead {
    /// フォント種別。
    font_type: FontType,
    /// 元の読み込みエラー。
    #[source]
    source: ReadError,
  },
  /// ページ設定を生成できませんでした。
  #[error("ページ設定を生成できませんでした: width={width}, height={height}")]
  #[diagnostic(
    code(seiran_pdf::invalid_page_size),
    help("width と height が正の有限値であることを確認してください。")
  )]
  InvalidPageSize {
    /// ページ幅。
    width: f32,
    /// ページ高さ。
    height: f32,
  },
  /// 罫線用の矩形を生成できませんでした。
  #[error("罫線の矩形を生成できませんでした")]
  #[diagnostic(
    code(seiran_pdf::invalid_rule_rect),
    help("レイアウトから渡された width と height が正の値であることを確認してください。")
  )]
  InvalidRuleRect,
  /// 罫線用のパスを生成できませんでした。
  #[error("罫線のパスを生成できませんでした")]
  #[diagnostic(code(seiran_pdf::invalid_rule_path), help("罫線の矩形が正しく構築されているか確認してください。"))]
  InvalidRulePath,
  /// リンク注釈用の矩形を生成できませんでした。
  #[error("リンク注釈の矩形を生成できませんでした")]
  #[diagnostic(
    code(seiran_pdf::invalid_link_rect),
    help("リンク領域の x / y / width / height が正の有限値であることを確認してください。")
  )]
  InvalidLinkRect,
  /// PDF の最終化に失敗しました。
  #[error("PDF の最終化に失敗しました: {source}")]
  #[diagnostic(
    code(seiran_pdf::finalize_document),
    help("Krilla 側の内部エラーが発生しています。入力データを見直してください。")
  )]
  FinalizeDocument {
    /// 元の生成エラー。
    #[source]
    source: KrillaError,
  },
  /// 画像ファイルの形式が未対応です。
  #[error("画像ファイルの拡張子が未対応です: {path}")]
  #[diagnostic(
    code(seiran_pdf::unsupported_image_format),
    help("対応形式は PNG (.png), JPEG (.jpg / .jpeg), SVG (.svg) です。")
  )]
  UnsupportedImageFormat {
    /// 画像ファイルのパス。
    path: String,
  },
  /// SVG のパースに失敗しました。
  #[error("SVG のパースに失敗しました: {path}")]
  #[diagnostic(code(seiran_pdf::parse_svg), help("SVG ファイルが妥当な XML / SVG であることを確認してください。"))]
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
    code(seiran_pdf::draw_svg),
    help("krilla-svg が SVG の描画を完了できませんでした。SVG の内容を確認してください。")
  )]
  DrawSvg {
    /// 画像ファイルのパス。
    path: String,
  },
  /// 画像のデコードに失敗しました。
  #[error("画像のデコードに失敗しました: {path} ({reason})")]
  #[diagnostic(code(seiran_pdf::decode_image), help("画像ファイルが破損していないか確認してください。"))]
  DecodeImage {
    /// 画像ファイルのパス。
    path: String,
    /// krilla が返したメッセージ。
    reason: String,
  },
  /// 画像の描画サイズが不正です。
  #[error("画像の描画サイズが不正です: width={width}, height={height}")]
  #[diagnostic(
    code(seiran_pdf::invalid_image_size),
    help("width と height が正の有限値であることを確認してください。")
  )]
  InvalidImageSize {
    /// 描画幅。
    width: f32,
    /// 描画高さ。
    height: f32,
  },
  /// ラスタ画像のダウンサンプリング時の処理（デコード / リサイズ / 再エンコード）に失敗しました。
  #[error("画像のリサイズに失敗しました: {path}")]
  #[diagnostic(
    code(seiran_pdf::resize_image),
    help("元画像が破損していないか、または \\image[downsample=false] でリサイズを無効化してください。")
  )]
  ResizeImage {
    /// 画像ファイルのパス。
    path: String,
    /// 元の image クレートのエラー。
    #[source]
    source: image::ImageError,
  },
  /// `draw_publication_image` が `ResourceBundle.image_bytes` にない画像を参照しました。
  #[error("リソースに存在しない画像パスです（内部エラー）: {path}")]
  #[diagnostic(
    code(seiran_pdf::image_not_in_manifest),
    help("seiran 側の ImageManifest 収集ロジックに不具合があります。issue を報告してください。")
  )]
  ImageNotInManifest {
    /// 画像ファイルのパス。
    path: String,
  },
}
