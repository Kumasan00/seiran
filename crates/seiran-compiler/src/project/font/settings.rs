//! フォント処理の入力契約となる、検証済み・処理済みのフォント設定。
//!
//! TOML に対応する未検証型（`PreFontConfig` 等）と、そこから検証済み値を構築する処理は
//! 兄弟 module `project::config` が持つ（#336、#352）。`crate::typeset::font` はこの module の
//! 型だけを入力として受け取り、設定ファイルの形を知らない。

use std::{path::PathBuf, str::FromStr};

use thiserror::Error;

use crate::project::font::map::FontMap;

/// 19 フォント種別すべての検証済み設定
pub(crate) type FontConfigs = FontMap<FontConfig>;

/// 単一フォント種別の検証済み・処理済み設定
#[derive(Debug, Clone)]
pub(crate) struct FontConfig {
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
pub(crate) enum TextDirection {
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
pub(crate) struct TextDirectionParseError;

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
pub(crate) struct Feature {
  /// OpenType フィーチャータグ（4 バイト）
  pub tag: [u8; 4],
  /// フィーチャーの値（通常は 0=無効、1=有効）
  pub value: u32,
}

/// バリアブルフォント軸の設定値
#[derive(Debug, Clone, Copy)]
pub(crate) struct VariationAxis {
  /// 軸名（4 バイトの OpenType 軸タグ）
  pub name: [u8; 4],
  /// 目標値（実数）
  pub value: f64,
}
