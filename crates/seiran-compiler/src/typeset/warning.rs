//! 組版が検出した、ユーザーが直せる非致命的問題（#382）
//!
//! 組版を止めない問題は error ではなく severity(Warning) の leaf diagnostic にして、成功した
//! `Compilation` と一緒に返す（`compiler::Warnings`）。`tracing::warn!` は開発者向け観測に限り、
//! 同じ問題を診断と tracing の両方では出さない。

use miette::Diagnostic;
use thiserror::Error;

use crate::typeset::font::FontWarning;

/// 組版段の警告。
///
/// フォント資源の構築で見つかった警告（[`FontWarning`]）も、組版 phase の中で起きるので
/// この型が包む（#535）。`compiler` は警告型を 1 つだけ名指しし、`typeset` の内部が
/// フォント → 本体の 2 段に分かれていることを知らない。
///
/// ページの指し方は**印字ページラベル**（前付けはローマ数字など `style.page_numbering` に従う）で、
/// 物理 index からの変換は [`super::pagination`] が行う。
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum TypesetWarning {
  /// フォント資源の構築（解析・検証）で見つかった警告。
  ///
  /// `transparent` でメッセージ・code・help・severity をすべて内側へ委譲し、診断の出方を
  /// 変えない（`TypesetError::Font` と同じ形）。
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

/// 脚注番号の列を `1, 2` の形へ整形する（[`TypesetWarning::FootnoteOverflow`] のメッセージ用）
fn join_numbers(numbers: &[u32]) -> String { return numbers.iter().map(u32::to_string).collect::<Vec<_>>().join(", "); }

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
    // Arrange — フォント検証の警告をそのまま包む
    let inner = FontWarning::MissingLayoutTable {
      font_type: FontType::Serif,
      path: ProjectPath::new("/project/font.ttf"),
      table: "GSUB",
    };
    let expected_message = inner.to_string();
    let expected_code = inner.code().expect("フォント警告は診断 code を持つはず").to_string();

    // Act
    let warning = TypesetWarning::Font(inner);

    // Assert — transparent なので severity / メッセージ / code は内側そのまま
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
