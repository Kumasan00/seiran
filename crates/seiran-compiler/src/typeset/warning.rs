//! 組版が検出した、ユーザーが直せる非致命的問題（#382）
//!
//! 組版を止めない問題は error ではなく severity(Warning) の leaf diagnostic にして、成功した
//! `Compilation` と一緒に返す（`compiler::Warnings`）。`tracing::warn!` は開発者向け観測に限り、
//! 同じ問題を診断と tracing の両方では出さない。

use miette::Diagnostic;
use thiserror::Error;

/// 組版段の警告。
///
/// ページの指し方は**印字ページラベル**（前付けはローマ数字など `style.page_numbering` に従う）で、
/// 物理 index からの変換は [`super::pagination`] が行う。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum TypesetWarning {
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

/// 脚注番号の列を `1, 2` の形へ整形する（[`TypesetWarning::FootnoteOverflow`] のメッセージ用）
fn join_numbers(numbers: &[u32]) -> String { return numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(", "); }

#[cfg(test)]
mod tests {
  use miette::{Diagnostic, Severity};

  use super::TypesetWarning;

  #[test]
  fn footnote_overflow_lists_all_numbers_on_the_line() {
    // Arrange
    let warning = TypesetWarning::FootnoteOverflow {
      page: "iii".to_owned(),
      numbers: vec![3, 4],
    };

    // Act
    let message = warning.to_string();

    // Assert
    assert!(message.contains("iii ページ"), "印字ラベルが本文に出るはず: {message}");
    assert!(message.contains("脚注 3, 4"), "行の脚注番号が列挙されるはず: {message}");
  }

  #[test]
  fn warnings_declare_warning_severity_and_typeset_codes() {
    // Arrange
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

    // Act & Assert
    for warning in &warnings {
      assert_eq!(warning.severity(), Some(Severity::Warning), "警告 severity を宣言しているはず");
      let code = warning.code().expect("診断 code を持つはず").to_string();
      assert!(code.starts_with("typeset::footnote::"), "検出段の code を名乗るはず: {code}");
    }
  }
}
