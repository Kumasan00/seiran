//! 文書が数式フォントで組む字のうち、数式フォントの cmap に基本の字形が無いものの検出
//!
//! 字は lowering・boxing が数式フォントで組むのと同じ規則で HIR から決める。テキスト・記号・演算子名は書記素クラスタの
//! 先頭の字（数式用英数字は写した後の字）で、和文として振り分ける字は除く — 後続の結合文字・異体字セレクタは `HarfBuzz` が
//! 基底の字と合成・分解するか無視するので要求しない。アクセント記号・伸縮括弧・根号記号・`cases` / `matrix` の区切り括弧は
//! 振り分けを経ず数式フォントで組むので、そのまま要求する。`ssty`・`flac`・MATH の size variant・glyph assembly は基本の
//! 字形から派生するので問わない。証明の末尾に数式フォントで組む style.toml の `qed_mark` も、テキストと同じ規則で証明の位置に
//! 要求する。display 段の大型演算子（boxing が振り分けを経ず組む）も記号として振り分ける側で問うが、
//! 大型演算子の字に和文へ振り分ける字は無いので結果は同じ。

use miette::{Diagnostic, SourceSpan};
use thiserror::Error;

use crate::{
  document::{
    HirDocument, HirInline, HirInlineKind, HirMath, HirMathKind, HirNode, HirNodeKind, MathVariant, NodeId,
    SourceLocation, SourceMap, Typeface,
  },
  failures::Failures,
  project::{FontType, ProjectPath},
  source::SourceId,
  style::Style,
  typeset::{boxing, lowering},
};

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
  glyph: char,
  /// その字を書いたテキスト、またはその字を出すコマンド・環境の位置
  #[label("{label}")]
  span: SourceSpan,
  /// ラベルの文言（ソースに書いた字と組む字が違えば、その対応を示す）
  label: String,
  /// 位置が属するソース
  source_id: SourceId,
  /// 数式フォントのパス（config.toml の `math`）
  font_path: String,
}

impl MissingMathGlyph {
  /// 字 `glyph` が `location` で欠けた診断を、ラベル `label` で作る
  fn new(location: SourceLocation, glyph: char, label: String, font_path: &ProjectPath) -> Self {
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

/// 文書が数式フォントで組む字を文書順に `has_glyph` へ問い、偽を返した字を使った箇所ごとに集める
///
/// 同じ位置で同じ字を何度使っても 1 件にする。`style` は定理クラスの `qed_mark` を引くスタイル、`font_path` は診断に載せる
/// 数式フォントのパス。
///
/// # Errors
///
/// 欠けた字が 1 つでもあれば、その全件を文書順（ソースの宣言順・ソース内の出現順）で返す。
pub(super) fn check(
  document: &HirDocument,
  style: &Style,
  has_glyph: &dyn Fn(char) -> bool,
  font_path: &ProjectPath,
) -> Result<(), Failures<MissingMathGlyph>> {
  let mut checker = Checker {
    style,
    has_glyph,
    source_map: document.source_map(),
    font_path,
    missing: Vec::new(),
  };
  for nodes in document.source_nodes() {
    checker.nodes(nodes);
  }
  return match Failures::from_vec(checker.missing) {
    Some(failures) => Err(failures),
    None => Ok(()),
  };
}

/// HIR を文書順に走査し、欠けた字の診断を積む
struct Checker<'a> {
  /// 定理クラスの `qed_mark` を引くスタイル
  style: &'a Style,
  /// 数式フォントが基本の字形を持つか
  has_glyph: &'a dyn Fn(char) -> bool,
  /// 診断位置を引く位置表
  source_map: &'a SourceMap,
  /// 診断に載せる数式フォントのパス
  font_path: &'a ProjectPath,
  /// 見つかった欠落（文書順）
  missing: Vec<MissingMathGlyph>,
}

impl Checker<'_> {
  /// ブロックノード列を走査する
  fn nodes(&mut self, nodes: &[HirNode]) {
    for node in nodes {
      self.node(node);
    }
    return;
  }

  /// ブロックノード 1 つを走査する
  fn node(&mut self, node: &HirNode) {
    match &node.kind {
      HirNodeKind::Heading(heading) => self.inlines(&heading.title),
      HirNodeKind::Paragraph(inlines) => self.inlines(inlines),
      HirNodeKind::List(list) => {
        for item in &list.items {
          self.nodes(&item.body);
        }
      },
      HirNodeKind::MathBlock(math) => {
        let delimiters = lowering::delimiter_glyphs(math.kind);
        if let Some(left) = delimiters.left {
          self.direct(node.id, left);
        }
        for row in &math.rows {
          for cell in &row.cells {
            self.math_list(cell, None);
          }
        }
        if let Some(right) = delimiters.right {
          self.direct(node.id, right);
        }
      },
      HirNodeKind::Figure(figure) => {
        if let Some(caption) = &figure.caption {
          self.inlines(caption);
        }
      },
      HirNodeKind::Table(table) => {
        for row in table.head.iter().chain(table.rows.iter()) {
          for cell in &row.cells {
            self.inlines(&cell.content);
          }
        }
        if let Some(caption) = &table.caption {
          self.inlines(caption);
        }
      },
      HirNodeKind::Theorem(theorem) => {
        self.nodes(&theorem.body);
        if let Some(qed_mark) = self.style.theorems[theorem.class].qed_mark.as_deref() {
          for (glyph, _) in lowering::math_text_clusters(qed_mark) {
            if boxing::char_font_type(Typeface::Math, glyph) == FontType::Math {
              self.require(node.id, glyph, || {
                return "証明の終わりに style.toml の `qed_mark` として組みます".to_string();
              });
            }
          }
        }
      },
      HirNodeKind::Quote(quote) => self.nodes(&quote.body),
      HirNodeKind::Flush(flush) => self.nodes(&flush.body),
      HirNodeKind::CodeBlock(_) | HirNodeKind::PageBreak | HirNodeKind::Space(_) => {},
    }
    return;
  }

  /// インラインノード列を走査する
  fn inlines(&mut self, inlines: &[HirInline]) {
    for inline in inlines {
      match &inline.kind {
        HirInlineKind::Math(math) => self.math_list(math, None),
        HirInlineKind::Styled { children, .. }
        | HirInlineKind::Colored { children, .. }
        | HirInlineKind::Link { children, .. }
        | HirInlineKind::Footnote { body: children, .. } => self.inlines(children),
        HirInlineKind::Text(_)
        | HirInlineKind::Code(_)
        | HirInlineKind::Symbol(_)
        | HirInlineKind::LineBreak
        | HirInlineKind::NoIndent
        | HirInlineKind::Ref { .. }
        | HirInlineKind::Cite { .. }
        | HirInlineKind::Index { .. } => {},
      }
    }
    return;
  }

  /// 数式ノード列を、継承中の字形 variant `variant` で走査する
  fn math_list(&mut self, nodes: &[HirMath], variant: Option<MathVariant>) {
    for node in nodes {
      self.math(node, variant);
    }
    return;
  }

  /// 数式ノード 1 つを走査する（字形 variant はスクリプト・分数・根号・アクセント・括弧・線の中身へも継承する）
  fn math(&mut self, node: &HirMath, variant: Option<MathVariant>) {
    match &node.kind {
      HirMathKind::Text(text) | HirMathKind::Symbol { text, .. } => {
        for (written, _) in lowering::math_text_clusters(text) {
          self.routed(node.id, written, lowering::translate_math_char(written, variant));
        }
      },
      // 演算子名は字形 variant を読まず立体の ASCII のまま組み、語の区切りの半角空白は字ではなく細アキ
      HirMathKind::OperatorName { text, .. } => {
        for written in text.chars().filter(|&ch| return ch != ' ') {
          self.routed(node.id, written, written);
        }
      },
      HirMathKind::Space(_) => {},
      HirMathKind::Group(children) => self.math_list(children, variant),
      HirMathKind::Superscript(inner) | HirMathKind::Subscript(inner) => self.math(inner, variant),
      HirMathKind::Frac { numer, denom } => {
        self.math(numer, variant);
        self.math(denom, variant);
      },
      HirMathKind::Sqrt { index, radicand } => {
        self.direct(node.id, boxing::RADICAL_SIGN);
        if let Some(index) = index {
          self.math(index, variant);
        }
        self.math(radicand, variant);
      },
      HirMathKind::Accent { accent, base, .. } => {
        self.written(node.id, *accent, *accent);
        self.math(base, variant);
      },
      HirMathKind::Fenced { open, close, body } => {
        self.written(node.id, *open, *open);
        self.math(body, variant);
        self.written(node.id, *close, *close);
      },
      HirMathKind::Bar { body, .. } => self.math(body, variant),
      HirMathKind::Styled {
        variant: inner,
        children,
      } => self.math_list(children, Some(*inner)),
    }
    return;
  }

  /// 振り分けを経ず数式フォントで組む字の列 `text` を、箇所 `site` で要求する
  fn direct(&mut self, site: NodeId, text: &str) {
    for glyph in text.chars() {
      self.written(site, glyph, glyph);
    }
    return;
  }

  /// テキストとして組む字 `glyph`（ソースの字 `written` を写したもの）を、数式フォントへ振り分けるときだけ要求する
  fn routed(&mut self, site: NodeId, written: char, glyph: char) {
    if boxing::char_font_type(Typeface::Math, glyph) == FontType::Math {
      self.written(site, written, glyph);
    }
    return;
  }

  /// ソースの字 `written` を写した字 `glyph`（写していなければ同じ字）を箇所 `site` で要求する
  fn written(&mut self, site: NodeId, written: char, glyph: char) {
    self.require(site, glyph, || {
      if written == glyph {
        return "ここで使っています".to_string();
      }
      return format!("`{written}` を {glyph} として組みます");
    });
    return;
  }

  /// 数式フォントで組む字 `glyph` の基本の字形を要求し、無ければ箇所 `site` の診断をラベル `label` で積む（同じ位置・
  /// 同じ字・同じラベルの診断は 1 件にする）
  fn require(&mut self, site: NodeId, glyph: char, label: impl FnOnce() -> String) {
    if (self.has_glyph)(glyph) {
      return;
    }
    let missing = MissingMathGlyph::new(self.source_map.location(site), glyph, label(), self.font_path);
    if !self.missing.contains(&missing) {
      self.missing.push(missing);
    }
    return;
  }
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use super::check;
  use crate::{document::HirDocument, frontend::test_support, project::ProjectPath, source::SourceId, style::Style};

  /// 診断のパスに載せる数式フォント（テストでは実在しなくてよい）
  fn font_path() -> ProjectPath { return ProjectPath::new("fonts/math.otf"); }

  /// ソース 1 本をパースして文書にする
  fn document(source: &str) -> HirDocument {
    let hir = test_support::parse_for_test(source, SourceId::new(0)).expect("パースに成功するはず");
    return HirDocument::assemble(vec![hir]);
  }

  /// `absent` の字だけを持たない数式フォントで検査し、診断を（字, 位置のソース文字列, ラベル）の文書順で返す
  fn missing(source: &str, absent: &[char]) -> Vec<(char, String, String)> {
    let Err(failures) = check(&document(source), &Style::default(), &|ch| return !absent.contains(&ch), &font_path())
    else {
      return Vec::new();
    };
    return failures
      .into_iter()
      .map(|error| {
        let start = error.span.offset();
        return (error.glyph, source[start..start + error.span.len()].to_string(), error.label);
      })
      .collect();
  }

  /// 何でも持つ数式フォントで検査し、問い合わせた字を順に返す
  fn queried(source: &str) -> Vec<char> {
    let asked = RefCell::new(Vec::new());
    let result = check(
      &document(source),
      &Style::default(),
      &|ch| {
        asked.borrow_mut().push(ch);
        return true;
      },
      &font_path(),
    );
    assert!(result.is_ok(), "全字を持つフォントなら欠落は無いはず");
    return asked.into_inner();
  }

  #[test]
  fn mapped_alphanumeric_is_reported_with_the_written_char() {
    let found = missing("$x$", &['\u{1D465}']);

    assert_eq!(found, vec![('\u{1D465}', "x".to_string(), "`x` を \u{1D465} として組みます".to_string())]);
  }

  #[test]
  fn unmapped_char_is_reported_at_its_text() {
    let found = missing("$\u{0E01}$", &['\u{0E01}']);

    assert_eq!(found, vec![('\u{0E01}', "\u{0E01}".to_string(), "ここで使っています".to_string())]);
  }

  #[test]
  fn command_glyphs_are_reported_at_their_command() {
    let source = "$\\leq\\sqrt{2}\\hat{1}\\paren{2}$";

    let found = missing(source, &['≤', '√', '\u{0302}', '(', ')']);

    let sites: Vec<(char, String)> = found.into_iter().map(|(glyph, site, _)| return (glyph, site)).collect();
    assert_eq!(
      sites,
      vec![
        ('≤', "\\leq".to_string()),
        ('√', "\\sqrt{2}".to_string()),
        ('\u{0302}', "\\hat{1}".to_string()),
        ('(', "\\paren{2}".to_string()),
        (')', "\\paren{2}".to_string()),
      ]
    );
  }

  #[test]
  fn block_delimiters_are_reported_at_their_environment() {
    let source = "\\begin{cases}\n1 & 2\n\\end{cases}\n\n\\begin{matrix}[delimiter=paren]\n1\n\\end{matrix}\n";

    let found = missing(source, &['{', '(', ')']);

    let sites: Vec<(char, bool)> = found
      .iter()
      .map(|(glyph, site, _)| {
        return (*glyph, site.starts_with("\\begin{cases}") || site.starts_with("\\begin{matrix}"));
      })
      .collect();
    assert_eq!(sites, vec![('{', true), ('(', true), (')', true)]);
  }

  #[test]
  fn repeated_missing_char_in_one_text_is_reported_once() {
    let found = missing("$\u{0E01}\u{0E01}$ $\u{0E01}$", &['\u{0E01}']);

    assert_eq!(found.len(), 2, "同じ位置の同じ字は 1 件、別の数式の同じ字は別の 1 件: {found:?}");
  }

  #[test]
  fn missing_chars_are_reported_in_document_order() {
    let found = missing("$\u{0E02}$ と $\u{0E01}$ と $\u{0E02}$", &['\u{0E01}', '\u{0E02}']);

    let glyphs: Vec<char> = found.into_iter().map(|(glyph, _, _)| return glyph).collect();
    assert_eq!(glyphs, vec!['\u{0E02}', '\u{0E01}', '\u{0E02}']);
  }

  #[test]
  fn unused_missing_chars_do_not_fail() {
    let found = missing("$x + 1$", &['\u{0E01}', '√']);

    assert_eq!(found, Vec::new());
  }

  #[test]
  fn japanese_chars_are_routed_away_from_the_math_font() {
    let asked = queried("$v_{速度}$");

    assert_eq!(asked, vec!['\u{1D463}']);
  }

  #[test]
  fn only_the_first_char_of_each_cluster_is_queried() {
    let asked = queried("$=\u{0338}$ $\\mathcalligraphic{A}$ $x\u{0302}$");

    assert_eq!(asked, vec!['=', '\u{1D49C}', '\u{1D465}']);
  }

  #[test]
  fn operator_names_are_queried_upright_without_the_word_space() {
    let asked = queried("$\\mathbold{\\limsup}$");

    assert_eq!(asked, vec!['l', 'i', 'm', 's', 'u', 'p']);
  }

  #[test]
  fn only_base_glyphs_are_queried() {
    let asked = queried("\\begin{equation}\n\\sum_{i} x^{2} \\hat{y}\n\\end{equation}\n");

    assert_eq!(asked, vec!['∑', '\u{1D456}', '\u{1D465}', '2', '\u{0302}', '\u{1D466}']);
  }

  #[test]
  fn math_in_nested_containers_is_checked() {
    let source = concat!(
      "\\section{見出し $\u{0E01}$}\n\n",
      "本文\\footnote{脚注 $\u{0E01}$}。\n\n",
      "\\begin{itemize}\n\\item{項目 $\u{0E01}$}\n\\end{itemize}\n\n",
      "\\begin{table}\n\\caption{表 $\u{0E01}$}\n\\row{$\u{0E01}$}\n\\end{table}\n",
    );

    let found = missing(source, &['\u{0E01}']);

    assert_eq!(found.len(), 5, "見出し・脚注・箇条書き・キャプション・表のセルの 5 箇所: {found:?}");
  }

  #[test]
  fn qed_mark_is_reported_at_the_proof() {
    let source = "\\begin{proof}\n本文\n\\end{proof}\n";

    let found = missing(source, &['□']);

    let sites: Vec<(char, bool, String)> = found
      .into_iter()
      .map(|(glyph, site, label)| return (glyph, site.starts_with("\\begin{proof}"), label))
      .collect();
    assert_eq!(sites, vec![('□', true, "証明の終わりに style.toml の `qed_mark` として組みます".to_string())]);
  }

  #[test]
  fn message_shows_the_glyph_and_its_code_point() {
    let Err(failures) = check(&document("$x$"), &Style::default(), &|_| return false, &font_path()) else {
      panic!("字を持たないフォントなら失敗するはず");
    };

    assert_eq!(failures.first().to_string(), "数式フォントに \u{1D465} (U+1D465) のグリフがありません");
  }
}
