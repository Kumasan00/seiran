//! TOML 設定ファイル（config.toml / style.toml）の解析そのものと、解析エラーを leaf diagnostic を組む
//! 部品へ分解する規則。設定ファイルの TOML 解析は [`parse_toml`] 1 つに閉じ、呼び出し側は
//! `toml::from_str` を直接呼ばない。失敗は診断部品 [`TomlErrorParts`] として返る。

use miette::{NamedSource, SourceSpan};
use serde::de::DeserializeOwned;

/// TOML 設定ファイル（config.toml / style.toml）の本文 `content` を `T` へ解析する。失敗は表示名
/// `display_path` 付きの診断部品 [`TomlErrorParts`] で返す。
///
/// 設定ファイルの TOML 解析はすべてここを通る。呼び出し側は `toml::from_str` を直接呼ばず、
/// 返った部品から自分の `ParseToml` variant を組む（#647）。
pub(crate) fn parse_toml<T: DeserializeOwned>(
  display_path: impl AsRef<str>,
  content: &str,
) -> Result<T, TomlErrorParts> {
  return toml::from_str(content).map_err(|error| return TomlErrorParts::new(display_path, content, error));
}

/// TOML 解析エラー 1 件を、`#[source_code]` / `#[label]` / `#[source]` を持つ leaf diagnostic の部品へ
/// 分解したもの。[`parse_toml`] が解析に失敗したときだけ作られる。
///
/// 位置を示すのは miette のラベル（`src` + `span`）だけにする。`toml::de::Error` の `Display` は input を
/// 持つと `TOML parse error at line N, column M` の自前スニペットを描画し、miette の `╰─▶` 行とラベルで
/// 位置が二重に出るため、`source` は input を消してから持つ（#647）。
///
/// 診断 code / help は設定ファイルの役割ごとに違うので、variant は各所有者
/// （`project::config::ReadConfigError::ParseToml` / `style::ReadStyleError::ParseToml`）が持ち、
/// ここは部品だけを返す。`Failures` で包むのも呼び出し側 — この module は crate 内の他 module に
/// 依存しない（`project` の依存の不変条件）。
#[derive(Debug)]
pub(crate) struct TomlErrorParts {
  /// 位置表示に使うソース全文と表示名
  pub(crate) src: NamedSource<String>,
  /// エラー箇所のソース内スパン（toml が位置を持たないときは先頭の空スパン）
  pub(crate) span: SourceSpan,
  /// 自前スニペットを抑止した元の toml エラー（メッセージだけを描画する）
  pub(crate) source: toml::de::Error,
}

impl TomlErrorParts {
  /// 表示名 `display_path` の `content` を解析して得た `error` を、診断の部品へ分解する。
  ///
  /// [`parse_toml`] だけが呼ぶ — TOML 解析エラーの位置付けが 1 か所に閉じる構造を、
  /// このコンストラクタを非公開にすることで保証する。
  fn new(display_path: impl AsRef<str>, content: &str, mut error: toml::de::Error) -> Self {
    // span は input とは別フィールドに保持されるので set_input(None) の後でも失われないが、読む順序は既存実装に揃える
    let span = error.span().map_or_else(
      || return SourceSpan::new(0.into(), 0),
      |range| return SourceSpan::new(range.start.into(), range.end.saturating_sub(range.start)),
    );
    // toml::de::Error::Display は input が設定されていると line/column の自前スニペットを描画する。
    // miette の #[label] と二重に位置情報が出るため、ここで input をクリアして抑止する。
    error.set_input(None);
    return TomlErrorParts {
      src: NamedSource::new(display_path, content.to_string()),
      span,
      source: error,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::{TomlErrorParts, parse_toml};

  /// 閉じ引用符の無い文字列（issue #647 の再現入力）
  const UNCLOSED_STRING: &str = "[page]\nmargin_top = \"10mm\n";

  #[test]
  fn suppresses_toml_builtin_snippet() {
    let parts = parse_toml::<toml::value::Table>("style.toml", UNCLOSED_STRING).expect_err("構文エラーのはず");

    let rendered = parts.source.to_string();
    assert!(!rendered.contains("TOML parse error at line"), "toml の自前スニペットを抑止するはず: {rendered}");
  }

  #[test]
  fn records_span_at_syntax_error() {
    let parts = parse_toml::<toml::value::Table>("style.toml", UNCLOSED_STRING).expect_err("構文エラーのはず");

    // 2 行目 19 桁目 = "[page]\n"（7 バイト）+ 18
    assert_eq!(parts.span.offset(), 25);
  }

  #[test]
  fn keeps_display_path_and_content() {
    let parts = parse_toml::<toml::value::Table>("custom/style.toml", UNCLOSED_STRING).expect_err("構文エラーのはず");

    assert_eq!(parts.src.name(), "custom/style.toml");
    assert_eq!(parts.src.inner(), UNCLOSED_STRING);
  }

  #[test]
  fn falls_back_to_empty_span_without_location() {
    let error = <toml::de::Error as serde::de::Error>::custom("位置を持たないエラー");

    let parts = TomlErrorParts::new("style.toml", "", error);

    assert_eq!(parts.span.offset(), 0);
    assert!(parts.span.is_empty());
  }
}
