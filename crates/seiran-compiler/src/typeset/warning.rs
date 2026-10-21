//! 組版が検出した、ユーザーが直せる非致命的問題

use itertools::Itertools;
use miette::{Diagnostic, LabeledSpan, SourceSpan};
use thiserror::Error;

use crate::{
  length::Length,
  project::ProjectPath,
  source::SourceId,
  typeset::{breaking::OverflowAxis, font::FontWarning},
};

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
    /// 脚注が帰属するソース（1 行の脚注は同じ段落にある）
    source_id: SourceId,
    /// 各 `\footnote` コマンドの開始位置（`numbers` と同順、文言は「脚注 n」）
    #[label(collection)]
    footnotes: Vec<LabeledSpan>,
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
    /// 脚注が帰属するソース
    source_id: SourceId,
    /// `\footnote` コマンドの開始位置
    #[label("この脚注です")]
    span: SourceSpan,
  },

  /// 行（分割できないコード・数式内の文章など）が段の幅か実効の本文下限を超えた
  #[error("{page} ページの行「{excerpt}」が{axis}に {:.1}pt はみ出したまま配置しました", .excess.to_pt())]
  #[diagnostic(
    code(typeset::overflow::line),
    severity(Warning),
    help(
      "分割できない長いコードや数式内の文章（\\text）を短く分けるか、style.toml の文字サイズ・[page] の余白か config.toml の [pdf] の用紙サイズを見直してください。"
    )
  )]
  LineOverflow {
    /// はみ出しが起きたページの印字ラベル
    page: String,
    /// 行の内容の要約
    excerpt: String,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
    /// 行を含む要素が帰属するソース。単一のソース要素に対応しない生成物の行は `None`
    source_id: Option<SourceId>,
    /// 行を含む要素の開始位置（`source_id` と同時に `None`）
    #[label("この要素の行です")]
    span: Option<SourceSpan>,
  },

  /// 画像が段の幅か実効の本文下限を超えた
  #[error("{page} ページの画像 {path} が{axis}に {:.1}pt はみ出したまま配置しました", .excess.to_pt())]
  #[diagnostic(
    code(typeset::overflow::image),
    severity(Warning),
    help(
      "\\image の width / height を小さくするか、style.toml の [page] の余白か config.toml の [pdf] の用紙サイズを見直してください。"
    )
  )]
  ImageOverflow {
    /// はみ出しが起きたページの印字ラベル
    page: String,
    /// 画像ファイルへのパス
    path: ProjectPath,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
    /// `\image` が帰属するソース
    source_id: SourceId,
    /// `\image` コマンドの開始位置
    #[label("この画像です")]
    span: SourceSpan,
  },

  /// ディスプレイ数式の本体が段の幅か実効の本文下限を超えた
  #[error("{page} ページのディスプレイ数式「{excerpt}」が{axis}に {:.1}pt はみ出したまま配置しました", .excess.to_pt())]
  #[diagnostic(
    code(typeset::overflow::math),
    severity(Warning),
    help(
      "数式を align などの複数行の環境で分けるか、style.toml の文字サイズ・[page] の余白か config.toml の [pdf] の用紙サイズを見直してください。"
    )
  )]
  MathOverflow {
    /// はみ出しが起きたページの印字ラベル
    page: String,
    /// 数式本体の要約
    excerpt: String,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
    /// 数式環境が帰属するソース
    source_id: SourceId,
    /// 数式環境の開始位置
    #[label("この数式です")]
    span: SourceSpan,
  },
}

impl TypesetWarning {
  /// 帰属するソースを返す。フォントの警告と生成物の行は `None`
  #[must_use]
  pub(crate) fn source_id(&self) -> Option<SourceId> {
    return match self {
      TypesetWarning::Font(_) => None,
      TypesetWarning::LineOverflow { source_id, .. } => *source_id,
      TypesetWarning::FootnoteOverflow { source_id, .. }
      | TypesetWarning::FootnoteLineOverflow { source_id, .. }
      | TypesetWarning::ImageOverflow { source_id, .. }
      | TypesetWarning::MathOverflow { source_id, .. } => Some(*source_id),
    };
  }
}

/// 脚注番号の列を `1, 2` の形へ整形する
fn join_numbers(numbers: &[u32]) -> String { return numbers.iter().join(", "); }

#[cfg(test)]
mod tests {
  use miette::{Diagnostic, LabeledSpan, Severity, SourceSpan};

  use super::TypesetWarning;
  use crate::{
    length::Length,
    project::{FontType, ProjectPath},
    source::SourceId,
    typeset::{breaking::OverflowAxis, font::FontWarning},
  };

  /// テストの警告が帰属するソース
  fn source() -> SourceId { return SourceId::new(0); }

  /// 長さ 0 のラベル位置
  fn at(offset: usize) -> SourceSpan { return SourceSpan::from((offset, 0)); }

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
      source_id: source(),
      footnotes: vec![
        LabeledSpan::at(at(0), "脚注 3"),
        LabeledSpan::at(at(5), "脚注 4"),
      ],
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
        source_id: source(),
        footnotes: vec![LabeledSpan::at(at(0), "脚注 1")],
      },
      TypesetWarning::FootnoteLineOverflow {
        page: "1".to_owned(),
        number: 1,
        source_id: source(),
        span: at(0),
      },
      TypesetWarning::LineOverflow {
        page: "1".to_owned(),
        excerpt: "x".to_owned(),
        axis: OverflowAxis::Vertical,
        excess: Length::pt(1.0),
        source_id: None,
        span: None,
      },
      TypesetWarning::ImageOverflow {
        page: "1".to_owned(),
        path: ProjectPath::new("/project/fig.png"),
        axis: OverflowAxis::Vertical,
        excess: Length::pt(1.0),
        source_id: source(),
        span: at(0),
      },
      TypesetWarning::MathOverflow {
        page: "1".to_owned(),
        excerpt: "x".to_owned(),
        axis: OverflowAxis::Horizontal,
        excess: Length::pt(1.0),
        source_id: source(),
        span: at(0),
      },
    ];

    for warning in &warnings {
      assert_eq!(warning.severity(), Some(Severity::Warning), "警告 severity を宣言しているはず");
      let code = warning.code().expect("診断 code を持つはず").to_string();
      assert!(
        code.starts_with("typeset::footnote::") || code.starts_with("typeset::overflow::"),
        "検出段の code を名乗るはず: {code}"
      );
    }
  }

  #[test]
  fn line_overflow_names_page_excerpt_axis_and_amount() {
    let warning = TypesetWarning::LineOverflow {
      page: "3".to_owned(),
      excerpt: "abcdefghij…".to_owned(),
      axis: OverflowAxis::Horizontal,
      excess: Length::pt(1015.0),
      source_id: Some(source()),
      span: Some(at(0)),
    };

    assert_eq!(warning.to_string(), "3 ページの行「abcdefghij…」が横に 1015.0pt はみ出したまま配置しました");
  }

  #[test]
  fn image_overflow_names_page_path_axis_and_amount() {
    let warning = TypesetWarning::ImageOverflow {
      page: "1".to_owned(),
      path: ProjectPath::new("/project/fig.png"),
      axis: OverflowAxis::Vertical,
      excess: Length::pt(368.0),
      source_id: source(),
      span: at(0),
    };

    assert_eq!(warning.to_string(), "1 ページの画像 /project/fig.png が縦に 368.0pt はみ出したまま配置しました");
  }

  #[test]
  fn math_overflow_names_page_excerpt_axis_and_amount() {
    let warning = TypesetWarning::MathOverflow {
      page: "ii".to_owned(),
      excerpt: "x=1".to_owned(),
      axis: OverflowAxis::Horizontal,
      excess: Length::pt(12.34),
      source_id: source(),
      span: at(0),
    };

    assert_eq!(warning.to_string(), "ii ページのディスプレイ数式「x=1」が横に 12.3pt はみ出したまま配置しました");
  }

  #[test]
  fn source_id_is_some_only_for_warnings_with_a_location() {
    let font = TypesetWarning::Font(FontWarning::MissingLayoutTable {
      font_type: FontType::Serif,
      path: ProjectPath::new("/project/font.ttf"),
      table: "GSUB",
    });
    let generated_line = TypesetWarning::LineOverflow {
      page: "i".to_owned(),
      excerpt: "索引".to_owned(),
      axis: OverflowAxis::Horizontal,
      excess: Length::pt(1.0),
      source_id: None,
      span: None,
    };
    let image = TypesetWarning::ImageOverflow {
      page: "1".to_owned(),
      path: ProjectPath::new("/project/fig.png"),
      axis: OverflowAxis::Vertical,
      excess: Length::pt(1.0),
      source_id: SourceId::new(2),
      span: at(0),
    };

    assert_eq!(font.source_id(), None, "フォントの警告は設定ファイル由来");
    assert_eq!(generated_line.source_id(), None, "生成物の行はソースに帰属しない");
    assert_eq!(image.source_id(), Some(SourceId::new(2)));
  }

  #[test]
  fn footnote_overflow_labels_each_footnote() {
    let warning = TypesetWarning::FootnoteOverflow {
      page: "1".to_owned(),
      numbers: vec![3, 4],
      source_id: source(),
      footnotes: vec![
        LabeledSpan::at(at(2), "脚注 3"),
        LabeledSpan::at(at(9), "脚注 4"),
      ],
    };

    let labels: Vec<(usize, Option<String>)> = warning
      .labels()
      .expect("ラベルを持つはず")
      .map(|label| return (label.offset(), label.label().map(str::to_owned)))
      .collect();

    assert_eq!(
      labels,
      vec![
        (2, Some("脚注 3".to_owned())),
        (9, Some("脚注 4".to_owned()))
      ]
    );
  }
}
