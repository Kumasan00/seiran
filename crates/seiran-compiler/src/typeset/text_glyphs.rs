//! 本文の枠のフォントで組めない字（欠字）の収集と診断
//!
//! シェーピング（`boxing::Shaper`）が、いま組んでいる要素か生成元（[`GlyphOrigin`]）を添えて [`GlyphIssues`] へ積み、
//! 組版を終えてから [`GlyphIssues::report`] がソース位置付きの診断（[`MissingGlyph`]）へ変える。同じ要素の同じ欠字は
//! 何度組んでも（脚注のページ単位採番で本文パスを反復しても）1 件にする。数式フォントの字の欠落（[`MissingMathGlyph`]）
//! と同じ失敗の列 [`TextGlyphError`] で運ぶ。

use std::{cell::RefCell, fmt};

use itertools::Itertools;
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  document::{NodeId, SourceLocation, SourceMap},
  project::{FontConfigs, FontFace, FontType},
  source::SourceId,
  typeset::{math_glyphs::MissingMathGlyph, pagination},
};

/// シェイプしている字形の出どころ — 著者が書いた要素か、組版が生成した内容か
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) enum GlyphOrigin {
  /// 要素（段落・見出し・表の行・脚注・ディスプレイ数式）
  Element(NodeId),
  /// 単一のソース要素に対応しない生成内容
  Generated(Generated),
}

/// 組版が生成する内容の種類（宣言順が診断の表示順）
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(in crate::typeset) enum Generated {
  /// 参考文献の見出しと文献の項目
  Bibliography,
  /// 表題ページ
  FrontMatter,
  /// 目次
  Toc,
  /// 巻末索引
  Index,
  /// ヘッダー・フッター
  RunningContent,
}

/// 診断の文言での生成元の名前
impl fmt::Display for Generated {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    return formatter.write_str(match self {
      Generated::Bibliography => "参考文献",
      Generated::FrontMatter => "表題ページ",
      Generated::Toc => "目次",
      Generated::Index => "索引",
      Generated::RunningContent => "走り文",
    });
  }
}

/// 本文の枠で組めなかったクラスタ 1 件
#[derive(Debug, PartialEq, Eq)]
pub(in crate::typeset) struct GlyphIssue {
  /// 組んでいた要素か生成元
  pub origin: GlyphOrigin,
  /// 組んだ枠
  pub font_type: FontType,
  /// クラスタの文字列
  pub cluster: String,
  /// 問題の種類
  pub kind: IssueKind,
}

/// [`GlyphIssue`] の種類
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::typeset) enum IssueKind {
  /// 枠のどの候補にも字形が無い
  Missing,
}

/// 組版全体で見つかった [`GlyphIssue`] の収集器（同じ値は 1 回だけ積む）
///
/// シェーパーは共有参照しか持たないので内部可変にする。組版は単一スレッドで進む。
#[derive(Debug, Default)]
pub(in crate::typeset) struct GlyphIssues(RefCell<Vec<GlyphIssue>>);

impl GlyphIssues {
  /// `issue` を積む。同じ値が既にあれば積まない
  pub(in crate::typeset) fn record(&self, issue: GlyphIssue) {
    let mut issues = self.0.borrow_mut();
    if !issues.contains(&issue) {
      issues.push(issue);
    }
  }

  /// 積んだ問題を取り出し、診断へ変える（`source_map` で要素の位置を、`configs` で枠の候補を引く）
  ///
  /// 要素のものは (ソース, 要素の開始位置) の昇順、生成内容のものはその後ろに [`Generated`] の宣言順で並べる。同じ位置の
  /// ものは積んだ順のまま。
  pub(super) fn report(&self, source_map: &SourceMap, configs: &FontConfigs) -> Vec<TextGlyphError> {
    let mut issues: Vec<(GlyphIssue, Option<SourceLocation>)> = self
      .0
      .take()
      .into_iter()
      .map(|issue| {
        let location = match issue.origin {
          GlyphOrigin::Element(id) => Some(source_map.location(id)),
          GlyphOrigin::Generated(_) => None,
        };
        return (issue, location);
      })
      .collect();
    // 要素のものは生成元が `None` なので、`None` < `Some` で生成内容より前に並ぶ
    issues.sort_by_key(|(issue, location)| {
      let generated = match issue.origin {
        GlyphOrigin::Element(_) => None,
        GlyphOrigin::Generated(generated) => Some(generated),
      };
      return (generated, location.map(|location| return (location.source_id, location.span.start)));
    });
    return issues
      .into_iter()
      .map(|(issue, location)| {
        return match issue.kind {
          IssueKind::Missing => TextGlyphError::Text(MissingGlyph::new(issue, location, configs)),
        };
      })
      .collect();
  }
}

/// 本文の枠のどの候補にも字形が無い字を組んだ箇所
#[derive(Debug, Error, Diagnostic)]
#[error(
  "{slot} のどの候補にも「{cluster}」({}) のグリフがありません{}",
  code_points(.cluster),
  generated_note(*.generated)
)]
#[diagnostic(
  code(typeset::font::missing_glyph),
  help("config.toml の候補（{tried}）はどれもこの字を持っていません。この字を持つフォントを候補に足してください。")
)]
pub(crate) struct MissingGlyph {
  /// 組んだ枠の TOML キー
  slot: &'static str,
  /// 組めなかったクラスタの文字列
  cluster: String,
  /// 生成内容なら、その生成元
  generated: Option<Generated>,
  /// 試した候補（`font_configs.<枠>[<添字>] = <パス>` を候補の順に並べたもの）
  tried: String,
  /// 要素が帰属するソース。生成内容は `None`
  source_id: Option<SourceId>,
  /// 要素の開始位置（`source_id` と同時に `None`）
  #[label("この要素で使っています")]
  span: Option<SourceSpan>,
}

impl MissingGlyph {
  /// `issue` の診断を、要素の位置 `location`（生成内容は `None`）と枠の候補 `configs` から作る
  fn new(issue: GlyphIssue, location: Option<SourceLocation>, configs: &FontConfigs) -> Self {
    let tried = configs
      .candidates(issue.font_type)
      .iter()
      .enumerate()
      .map(|(candidate, config)| {
        let face = FontFace::new(issue.font_type, candidate);
        return format!("font_configs.{face} = {}", config.font_path.as_ref().display());
      })
      .join(", ");
    return MissingGlyph {
      slot: issue.font_type.as_toml_key(),
      cluster: issue.cluster,
      generated: match issue.origin {
        GlyphOrigin::Element(_) => None,
        GlyphOrigin::Generated(generated) => Some(generated),
      },
      tried,
      source_id: location.map(|location| return location.source_id),
      span: location.map(pagination::start_span),
    };
  }
}

/// クラスタの字を `U+XXXX` の列（空白区切り）にする
fn code_points(cluster: &str) -> String {
  return cluster.chars().map(|ch| return format!("U+{:04X}", u32::from(ch))).join(" ");
}

/// 生成内容の文言の注記（要素なら空）
fn generated_note(generated: Option<Generated>) -> String {
  return generated.map_or_else(String::new, |generated| return format!("（{generated}が生成した文字列）"));
}

/// 組版の字の欠落 1 件 — 数式フォントの字か、本文の枠の字
#[derive(Debug, Error, Diagnostic)]
pub(crate) enum TextGlyphError {
  /// 数式フォントに基本の字形が無い字
  #[error(transparent)]
  #[diagnostic(transparent)]
  Math(MissingMathGlyph),
  /// 本文の枠のどの候補にも字形が無い字
  #[error(transparent)]
  #[diagnostic(transparent)]
  Text(MissingGlyph),
}

impl TextGlyphError {
  /// 帰属するソースを返す。生成内容の字は `None`
  #[must_use]
  pub(crate) fn source_id(&self) -> Option<SourceId> {
    return match self {
      TextGlyphError::Math(error) => Some(error.source_id()),
      TextGlyphError::Text(error) => error.source_id,
    };
  }
}
