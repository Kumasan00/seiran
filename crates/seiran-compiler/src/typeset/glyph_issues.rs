//! 本文の枠の字の問題（欠字・異体字列の代替・候補の方向の不一致）の収集と診断
//!
//! シェーピング（`boxing::Shaper`）が、いま組んでいる要素か生成元（[`GlyphOrigin`]）を添えて [`GlyphIssues`] へ積み、
//! 組版を終えてから [`GlyphIssues::report`] がソース位置付きの診断（エラーの [`MissingGlyph`] /
//! [`FallbackDirectionMismatch`] と警告の [`VariationSequenceFallback`]）へ変える。同じ要素の同じ問題は何度組んでも
//! （脚注のページ単位採番で本文パスを反復しても）1 件にする。エラーは数式フォントの字の欠落（[`MissingMathGlyph`]）と
//! 同じ失敗の列 [`TextGlyphError`] で運ぶ。

use std::{cell::RefCell, fmt};

use itertools::Itertools;
use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  document::{NodeId, SourceLocation, SourceMap},
  project::{FontConfigs, FontFace, FontType, ProjectPath},
  source::SourceId,
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

/// 本文の枠の字の問題 1 件
#[derive(Debug, PartialEq, Eq)]
pub(in crate::typeset) struct GlyphIssue {
  /// 組んでいた要素か生成元
  pub origin: GlyphOrigin,
  /// 組んだ枠
  pub font_type: FontType,
  /// 問題の種類
  pub kind: IssueKind,
}

/// [`GlyphIssue`] の種類
#[derive(Debug, PartialEq, Eq)]
pub(in crate::typeset) enum IssueKind {
  /// 枠のどの候補にも字形が無いクラスタ
  Missing {
    /// クラスタの文字列
    cluster: String,
  },
  /// 異体字列をどの候補も持たず、基底の字形で組んだクラスタ
  VariationFallback {
    /// クラスタの文字列
    cluster: String,
    /// 基底の字形で組んだ候補の位置
    used: usize,
  },
  /// 書字方向が先頭の候補の実効方向と異なり、代替に使わなかった候補（クラスタではなく候補の設定の問題なので、要素ごとに
  /// 候補 1 つにつき 1 件）
  DirectionMismatch {
    /// 使わなかった候補の位置
    candidate: usize,
  },
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

  /// 積んだ問題を取り出し、エラーと警告の診断へ変える（`source_map` で要素の位置を、`configs` で枠の候補を引く）
  ///
  /// どちらも、要素のものは (ソース, 要素の開始位置) の昇順、生成内容のものはその後ろに [`Generated`] の宣言順で並べる。
  /// 同じ位置のものは積んだ順のまま。
  pub(super) fn report(
    &self,
    source_map: &SourceMap,
    configs: &FontConfigs,
  ) -> (Vec<TextGlyphError>, Vec<VariationSequenceFallback>) {
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
      return (
        generated_of(issue.origin),
        location.map(|location| return (location.source_id, location.span.start)),
      );
    });
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for (issue, location) in issues {
      let site = Site::new(issue.origin, location);
      let font_type = issue.font_type;
      match issue.kind {
        IssueKind::Missing { cluster } => {
          errors.push(TextGlyphError::Text(MissingGlyph {
            slot: font_type.as_toml_key(),
            cluster,
            generated: site.generated,
            tried: tried_candidates(configs, font_type),
            source_id: site.source_id,
            span: site.span,
          }));
        },
        IssueKind::DirectionMismatch { candidate } => {
          errors.push(TextGlyphError::Direction(FallbackDirectionMismatch {
            face: FontFace::new(font_type, candidate),
            primary: FontFace::primary(font_type),
            generated: site.generated,
            source_id: site.source_id,
            span: site.span,
          }));
        },
        IssueKind::VariationFallback { cluster, used } => {
          warnings.push(VariationSequenceFallback {
            cluster,
            used: FontFace::new(font_type, used),
            tried: tried_candidates(configs, font_type),
            generated: site.generated,
            source_id: site.source_id,
            span: site.span,
          });
        },
      }
    }
    return (errors, warnings);
  }
}

/// 生成内容なら、その生成元
fn generated_of(origin: GlyphOrigin) -> Option<Generated> {
  return match origin {
    GlyphOrigin::Element(_) => None,
    GlyphOrigin::Generated(generated) => Some(generated),
  };
}

/// 診断を置く場所 — 要素の開始位置か、位置を持たない生成内容の生成元
struct Site {
  /// 生成内容なら、その生成元
  generated: Option<Generated>,
  /// 要素が帰属するソース。生成内容は `None`
  source_id: Option<SourceId>,
  /// 要素の開始位置（`source_id` と同時に `None`）
  span: Option<SourceSpan>,
}

impl Site {
  /// 起点 `origin` と、要素の位置 `location`（生成内容は `None`）から作る
  fn new(origin: GlyphOrigin, location: Option<SourceLocation>) -> Self {
    return Site {
      generated: generated_of(origin),
      source_id: location.map(|location| return location.source_id),
      span: location.map(|location| return location.span.at_start().into()),
    };
  }
}

/// 枠 `font_type` の候補を `font_configs.<枠>[<添字>] = <パス>` の形で候補の順に並べる
fn tried_candidates(configs: &FontConfigs, font_type: FontType) -> String {
  return configs
    .candidates(font_type)
    .iter()
    .enumerate()
    .map(|(candidate, config)| {
      return tried_face(FontFace::new(font_type, candidate), &config.font_path);
    })
    .join(", ");
}

/// 実体 `face`（パス `font_path`）を `font_configs.<実体> = <パス>` の形にする
fn tried_face(face: FontFace, font_path: &ProjectPath) -> String {
  return format!("font_configs.{face} = {}", font_path.as_ref().display());
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

/// 書字方向が先頭の候補の実効方向と異なるため、代替に使えなかった候補
#[derive(Debug, Error, Diagnostic)]
#[error("{face} の direction が {primary} の進行方向と異なるため、代替の候補として使えません{}", generated_note(*.generated))]
#[diagnostic(
  code(typeset::font::fallback_direction),
  help(
    "代替の候補は先頭の候補の進行方向で組みます。config.toml の font_configs.{face} の direction を外すか、{primary} と同じ方向にしてください。"
  )
)]
pub(crate) struct FallbackDirectionMismatch {
  /// 使えなかった候補
  face: FontFace,
  /// 枠の先頭の候補
  primary: FontFace,
  /// 生成内容なら、その生成元
  generated: Option<Generated>,
  /// 要素が帰属するソース。生成内容は `None`
  source_id: Option<SourceId>,
  /// 要素の開始位置（`source_id` と同時に `None`）
  #[label("この要素の字を組むために試しました")]
  span: Option<SourceSpan>,
}

/// 異体字列をどの候補も持たないため、基底の字形で組んだ箇所（本文の枠と数式フォントで共用）
#[derive(Debug, Error, Diagnostic)]
#[error(
  "異体字列「{cluster}」({}) の字形がどの候補にも無いため、基底の字形を {used} で組みました{}",
  code_points(.cluster),
  generated_note(*.generated)
)]
#[diagnostic(
  code(typeset::font::variation_sequence),
  severity(Warning),
  help(
    "config.toml の候補（{tried}）はどれもこの異体字列を持っていません。異体字の字形が必要なら、この異体字列を持つフォントを候補に足してください。"
  )
)]
pub(crate) struct VariationSequenceFallback {
  /// 異体字列を含むクラスタの文字列
  cluster: String,
  /// 基底の字形を組んだ実体
  used: FontFace,
  /// 試した候補（`font_configs.<実体> = <パス>` を候補の順に並べたもの）
  tried: String,
  /// 生成内容なら、その生成元
  generated: Option<Generated>,
  /// 要素が帰属するソース。生成内容は `None`
  source_id: Option<SourceId>,
  /// 要素の開始位置（`source_id` と同時に `None`）
  #[label("この要素で使っています")]
  span: Option<SourceSpan>,
}

impl VariationSequenceFallback {
  /// 数式フォント（パス `font_path`）で組んだクラスタ `cluster` の異体字列を、要素の位置 `location` で警告する
  pub(super) fn math(cluster: String, location: SourceLocation, font_path: &ProjectPath) -> Self {
    return VariationSequenceFallback {
      cluster,
      used: FontFace::MATH,
      tried: tried_face(FontFace::MATH, font_path),
      generated: None,
      source_id: Some(location.source_id),
      span: Some(location.span.at_start().into()),
    };
  }

  /// 帰属するソースを返す。生成内容の字は `None`
  #[must_use]
  pub(crate) fn source_id(&self) -> Option<SourceId> { return self.source_id; }

  /// 文書での並び順の鍵 — 要素のものは (ソース, 要素の開始位置) の昇順、生成内容のものはその後ろ
  pub(super) fn order_key(&self) -> (bool, Option<(SourceId, usize)>) {
    return (
      self.generated.is_some(),
      self.source_id.zip(self.span).map(|(source_id, span)| return (source_id, span.offset())),
    );
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

/// 数式フォントに基本の字形が無い字を数式で使った箇所
#[derive(Debug, PartialEq, Eq, Error, Diagnostic)]
#[error("数式フォントに {glyph} (U+{:04X}) のグリフがありません", u32::from(*.glyph))]
#[diagnostic(
  code(typeset::math::missing_glyph),
  help(
    "config.toml の `math` のフォント（{font_path}）はこの字を持っていません。この字を持つ数式フォントを指定してください。"
  )
)]
pub(crate) struct MissingMathGlyph {
  /// 数式フォントで組もうとした字（数式用英数字は写した後の字）
  pub(in crate::typeset) glyph: char,
  /// その字を書いたテキスト、またはその字を出すコマンド・環境の位置
  #[label("{label}")]
  pub(in crate::typeset) span: SourceSpan,
  /// ラベルの文言（ソースに書いた字と組む字が違えば、その対応を示す）
  pub(in crate::typeset) label: String,
  /// 位置が属するソース
  source_id: SourceId,
  /// 数式フォントのパス（config.toml の `math`）
  font_path: String,
}

impl MissingMathGlyph {
  /// 字 `glyph` が `location` で欠けた診断を、ラベル `label` で作る
  pub(in crate::typeset) fn new(location: SourceLocation, glyph: char, label: String, font_path: &ProjectPath) -> Self {
    return MissingMathGlyph {
      glyph,
      span: location.span.into(),
      label,
      source_id: location.source_id,
      font_path: font_path.as_ref().display().to_string(),
    };
  }

  /// この診断が帰属するソースを返す
  #[must_use]
  pub(crate) fn source_id(&self) -> SourceId { return self.source_id; }
}

/// 組版の字の問題のうちエラー 1 件 — 数式フォントの字の欠落、本文の枠の字の欠落、使えない代替の候補
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
  /// 書字方向が先頭の候補と異なる代替の候補
  #[error(transparent)]
  #[diagnostic(transparent)]
  Direction(FallbackDirectionMismatch),
}

impl TextGlyphError {
  /// 帰属するソースを返す。生成内容の字は `None`
  #[must_use]
  pub(crate) fn source_id(&self) -> Option<SourceId> {
    return match self {
      TextGlyphError::Math(error) => Some(error.source_id()),
      TextGlyphError::Text(error) => error.source_id,
      TextGlyphError::Direction(error) => error.source_id,
    };
  }
}
