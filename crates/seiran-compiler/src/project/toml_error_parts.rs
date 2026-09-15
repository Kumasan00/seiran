//! TOML 設定ファイル（config.toml / style.toml）の解析エラーを、leaf diagnostic を組む部品へ分解する
//! [`TomlErrorParts`]

use miette::{NamedSource, SourceSpan};

/// TOML 解析エラー 1 件を、`#[source_code]` / `#[label]` / `#[source]` を持つ leaf diagnostic の部品へ
/// 分解したもの。
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
  pub(crate) fn new(display_path: impl AsRef<str>, content: &str, mut error: toml::de::Error) -> Self {
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
  use super::TomlErrorParts;

  /// 閉じ引用符の無い文字列（issue #647 の再現入力）
  const UNCLOSED_STRING: &str = "[page]\nmargin_top = \"10mm\n";

  /// 解析に失敗する `content` から `toml::de::Error` を作る。
  fn toml_error(content: &str) -> toml::de::Error {
    return toml::from_str::<toml::value::Table>(content).expect_err("このケースは TOML 解析に失敗するはず");
  }

  #[test]
  fn suppresses_toml_builtin_snippet() {
    let parts = TomlErrorParts::new("style.toml", UNCLOSED_STRING, toml_error(UNCLOSED_STRING));

    let rendered = parts.source.to_string();
    assert!(!rendered.contains("TOML parse error at line"), "toml の自前スニペットを抑止するはず: {rendered}");
  }

  #[test]
  fn records_span_at_syntax_error() {
    let parts = TomlErrorParts::new("style.toml", UNCLOSED_STRING, toml_error(UNCLOSED_STRING));

    // 2 行目 19 桁目 = "[page]\n"（7 バイト）+ 18
    assert_eq!(parts.span.offset(), 25);
  }

  #[test]
  fn keeps_display_path_and_content() {
    let parts = TomlErrorParts::new("custom/style.toml", UNCLOSED_STRING, toml_error(UNCLOSED_STRING));

    assert_eq!(parts.src.name(), "custom/style.toml");
    assert_eq!(parts.src.inner(), UNCLOSED_STRING);
  }
}
