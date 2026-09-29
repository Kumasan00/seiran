//! 組版が検出した、ユーザーが直せる非致命的問題

use itertools::Itertools;
use miette::Diagnostic;
use thiserror::Error;

use crate::typeset::font::FontWarning;

/// 組版段の警告。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum TypesetWarning {
  /// フォント資源の構築（解析・検証）で見つかった警告。
  ///
  /// `transparent` でメッセージ・code・help・severity をすべて内側へ委譲し、診断の出方を
  /// 変えない。
  #[error(transparent)]
  #[diagnostic(transparent)]
  Font(#[from] FontWarning),

  /// 行に付いた脚注群が、空のページでも版面に収まらなかった
  #[error("{page} ページの脚注 {} がページの高さを超えるため、はみ出したまま配置しました", join_numbers(.numbers))]
  #[diagnostic(
    code(typeset::footnote::overflow),
    severity(Warning),
    help(
      "style.toml の [footnote] の font_size / top_margin / rule_gap か [page] の余白を小さくするか、config.toml の [pdf] の用紙サイズを見直してください。"
    )
  )]
  FootnoteOverflow {
    /// はみ出しが起きたページの印字ラベル
    page: String,
    /// はみ出した脚注の表示番号（出現順）
    numbers: Vec<u32>,
  },

  /// 繰り越した脚注の 1 行だけでページの高さを超えた
  #[error("{page} ページの脚注 {number} は 1 行がページの高さを超えるため、はみ出したまま配置しました")]
  #[diagnostic(
    code(typeset::footnote::line_overflow),
    severity(Warning),
    help(
      "style.toml の [footnote] の font_size か [page] の余白を小さくするか、config.toml の [pdf] の用紙サイズを見直してください。"
    )
  )]
  FootnoteLineOverflow {
    /// はみ出しが起きたページの印字ラベル
    page: String,
    /// はみ出した脚注の表示番号
    number: u32,
  },
}

/// 脚注番号の列を `1, 2` の形へ整形する
fn join_numbers(numbers: &[u32]) -> String { return numbers.iter().join(", "); }

#[cfg(test)]
mod tests {
  use miette::{Diagnostic, Severity};

  use super::TypesetWarning;
  use crate::{
    project::{FontType, ProjectPath},
    typeset::font::FontWarning,
  };

  #[test]
  fn font_variant_forwards_severity_message_and_code() {
    let inner = FontWarning::MissingLayoutTable {
      font_type: FontType::Serif,
      path: ProjectPath::new("/project/font.ttf"),
      table: "GSUB",
    };
    let expected_message = inner.to_string();
    let expected_code = inner.code().expect("フォント警告は診断 code を持つはず").to_string();

    let warning = TypesetWarning::Font(inner);

    assert_eq!(warning.severity(), Some(Severity::Warning), "警告 severity を転送するはず");
    assert_eq!(warning.to_string(), expected_message, "メッセージを転送するはず");
    assert_eq!(
      warning.code().expect("code を転送するはず").to_string(),
      expected_code,
      "診断 code を転送するはず（typeset::font::script::missing_layout_table）"
    );
  }

  #[test]
  fn footnote_overflow_lists_all_numbers_on_the_line() {
    let warning = TypesetWarning::FootnoteOverflow {
      page: "iii".to_owned(),
      numbers: vec![3, 4],
    };
    let message = warning.to_string();
    assert!(message.contains("iii ページ"), "印字ラベルが本文に出るはず: {message}");
    assert!(message.contains("脚注 3, 4"), "行の脚注番号が列挙されるはず: {message}");
  }

  #[test]
  fn warnings_declare_warning_severity_and_typeset_codes() {
    let warnings = [
      TypesetWarning::FootnoteOverflow {
        page: "1".to_owned(),
        numbers: vec![1],
      },
      TypesetWarning::FootnoteLineOverflow {
        page: "1".to_owned(),
        number: 1,
      },
    ];

    for warning in &warnings {
      assert_eq!(warning.severity(), Some(Severity::Warning), "警告 severity を宣言しているはず");
      let code = warning.code().expect("診断 code を持つはず").to_string();
      assert!(code.starts_with("typeset::footnote::"), "検出段の code を名乗るはず: {code}");
    }
  }
}
