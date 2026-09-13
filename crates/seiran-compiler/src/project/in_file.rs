//! 設定ファイル（config.toml / style.toml）の値検証の違反に、実際に読んだファイルのパスを添える帰属 adapter [`InFile`]

use miette::Diagnostic;

/// 設定ファイルの値検証の違反 1 件に、実際に読んだファイルのパスを添える leaf diagnostic。
///
/// メッセージにだけパスを前置し、`code` / `severity` / `help` / `url` / `source_code` / `labels` /
/// `related` / `diagnostic_source` は内側の違反へ委譲する。`typeset::font::validation::FontValidationFailure`
/// （フォント種別を前置）や `compiler::source_diagnostic::SourceDiagnostic`（本文を補う）と同じ
/// **帰属 adapter** であって集約 wrapper ではない — 描画は leaf 1 件ぶんで、入れ子の診断ブロックを作らず、
/// 診断 code も内側のまま変わらない（#552）。
///
/// help の定型文は役割名（「config.toml の該当フィールド」）しか書けないので、`-c` や `style_path` で
/// 任意の名前を付けた実際のファイルはこの前置でしか分からない。
#[derive(Debug)]
pub(crate) struct InFile<E> {
  /// 違反が見つかった設定ファイルのパス（読込に使ったパスの表示）
  path: String,
  /// 違反の内容
  error: E,
}

impl<E> InFile<E> {
  /// 違反 `error` に、それを見つけた設定ファイルのパス `path` を添える。
  pub(crate) fn new(path: impl Into<String>, error: E) -> Self {
    return InFile {
      path: path.into(),
      error,
    };
  }

  /// 違反の内容を借用で返す。
  #[cfg(test)]
  pub(crate) fn error(&self) -> &E { return &self.error; }
}

impl<E: std::fmt::Display> std::fmt::Display for InFile<E> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(f, "{}: {}", self.path, self.error);
  }
}

/// `error` は cause ではなくこの診断自身の内容なので `source` には載せない（載せると miette が `╰─▶` で
/// 同じ文言をもう一度描画する）。cause chain は `error` が持つ外部エラーへそのまま素通しする。
impl<E: std::error::Error + 'static> std::error::Error for InFile<E> {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> { return self.error.source(); }
}

impl<E: Diagnostic + 'static> Diagnostic for InFile<E> {
  fn code(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.error.code(); }

  fn severity(&self) -> Option<miette::Severity> { return self.error.severity(); }

  fn help(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.error.help(); }

  fn url(&self) -> Option<Box<dyn std::fmt::Display + '_>> { return self.error.url(); }

  fn source_code(&self) -> Option<&dyn miette::SourceCode> { return self.error.source_code(); }

  fn labels(&self) -> Option<Box<dyn Iterator<Item = miette::LabeledSpan> + '_>> { return self.error.labels(); }

  fn related<'a>(&'a self) -> Option<Box<dyn Iterator<Item = &'a dyn Diagnostic> + 'a>> { return self.error.related(); }

  fn diagnostic_source(&self) -> Option<&dyn Diagnostic> { return self.error.diagnostic_source(); }
}

#[cfg(test)]
mod tests {
  use miette::Diagnostic;
  use thiserror::Error;

  use super::InFile;

  /// 値検証の違反を模したテスト用エラー
  #[derive(Debug, Error, Diagnostic)]
  #[error("'pdf.width': 正値である必要があります")]
  #[diagnostic(code(test::field), help("テスト用のヘルプ"))]
  struct FieldError;

  #[test]
  fn prefixes_the_file_path_and_delegates_the_rest() {
    let attributed = InFile::new("/project/custom.toml", FieldError);

    assert_eq!(attributed.to_string(), "/project/custom.toml: 'pdf.width': 正値である必要があります");
    assert_eq!(attributed.code().expect("code を委譲するはず").to_string(), "test::field");
    assert_eq!(attributed.help().expect("help を委譲するはず").to_string(), "テスト用のヘルプ");
    assert!(std::error::Error::source(&attributed).is_none(), "内側の文言を cause として重ねないはず");
  }
}
