//! Lowering 層: Document IR → `LayoutNode` 変換
//!
//! `document` クレートの `DocNode`（セマンティックな論理構造）を
//! `LayoutNode`（物理的なレイアウト表現）に変換するクレートです。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! document (DocNode)
//!   ↓ [lowering]  ← このクレート
//! LayoutNode
//!   ↓ [layout::build_blocks → hlist::break_pages]
//! Vec<Page> (確定座標)
//!   ↓ [pdf_gen::render_pages]
//! PDF bytes
//! ```
//!
//! ## 責務
//!
//! - 見出しレベルに応じたフォントサイズ・フォント種別の決定（`heading` サブモジュール）
//! - 段落・インライン要素のスタイル付与（`paragraph` / `inline` サブモジュール）
//! - リストのインデント・マーカー生成（`list` サブモジュール）
//! - 数式の Unicode Mathematical Alphanumeric Symbols 変換（`math` サブモジュール）
//! - スタイルシート（`read_style` クレート）を `LoweringContext` 経由で受け取り、
//!   見出しレベルごとのフォントサイズ・フォーマット文字列・余白を適用する

use document::{DocNode, Document};
use miette::Diagnostic;
use read_style::Style as ReadStyle;
use thiserror::Error;
use tracing::debug;

mod figure;
mod float;
mod heading;
mod inline;
mod layout_node;
mod list;
mod math;
mod paragraph;
mod quote;
mod table;
mod template;
mod theorem;
mod title_page;

pub use layout_node::{LayoutNode, MathBlockRow, TableCellLayout, TableLayout, TableRowLayout, TextStyle};
pub use title_page::{TitlePageMetadata, lower_title_page};
pub use types::TableColumn;

/// Lowering（Document IR → `LayoutNode` 変換）で発生し得るエラー
///
/// 新しい lowering 失敗ケースを追加する際は本 enum にバリアントを追加してください。
/// `#[non_exhaustive]` を付与しているため、外部クレートでの `match` 漏れがコンパイルエラーになります。
#[derive(Debug, Error, Diagnostic)]
#[non_exhaustive]
pub enum LoweringError {
  /// 参照（`\ref{...}`）が pass2 で解決されないまま lowering に到達した場合に返されます。
  ///
  /// 評価器（`parser::evaluator`）の参照解決パスが、対応する `\label` を見つけられず
  /// `number` を `None` のまま渡してきたことを示します。
  #[error("未解決の参照が lowering に到達しました: ラベル `{label}`")]
  #[diagnostic(
    code(lowering::unresolved_reference),
    help(
      "対応する \\label が定義されているか確認してください。定義されている場合は評価器の参照解決パスにバグがある可能性があります。"
    )
  )]
  UnresolvedReference {
    /// 解決できなかったラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
    /// `\ref{...}` のソース位置（`InlineNode::Ref` から引き継ぐ）
    #[label("この参照が未解決です")]
    span: miette::SourceSpan,
  },

  /// 文献引用（`\cite{...}`）が CSL 整形ステージを経ずに lowering に到達した場合に返されます。
  ///
  /// `citation::process_citations`（lowering の前段）がラベルを採番していれば `label` は
  /// `Some` になります。`None` のまま到達したのは、整形ステージが呼ばれていないか
  /// 走査漏れがあることを示します。
  #[error("未整形の文献引用が lowering に到達しました: キー `{keys}`")]
  #[diagnostic(
    code(lowering::unresolved_citation),
    help(
      "lowering の前に citation::process_citations が実行されているか確認してください。実行済みの場合は CSL 整形ステージにバグがある可能性があります。"
    )
  )]
  UnresolvedCitation {
    /// 採番できなかった引用キー列（`\cite{a,b}` は `a, b`）
    keys: String,
    /// `\cite{...}` のソース位置（`InlineNode::Cite` から引き継ぐ）
    #[label("この引用が未整形です")]
    span: miette::SourceSpan,
  },
}

/// Lowering のコンテキスト
///
/// 変換中に必要なスタイル設定（`read_style::Style`）への参照を保持します。
///
/// ライフタイム `'a` は `Style` の借用元（typically `build_pdf` でのスコープ）に紐づきます。
pub struct LoweringContext<'a> {
  /// スタイル設定への参照（`config/style.toml` 由来 + figment デフォルト）
  pub style: &'a ReadStyle,
  /// 本文段落の既定フォント種別
  ///
  /// 通常は `style.text.font_kind`。定理本体のように既定書体を差し替えたいブロックでは
  /// [`LoweringContext::with_body_font_kind`] で派生した文脈を本体の lowering に渡す
  /// （斜体の定理本文・ローマンの証明本文など）。段落・本体内リストへ伝播する。
  pub body_font_kind: types::FontKind,
  /// 段落先頭行の字下げ量
  ///
  /// 通常は `style.text.first_line_indent`。[`crate::paragraph::lower_paragraph`] が正のとき
  /// 段落先頭に水平カーンを前置し、先頭行だけを字下げする。リスト項目・表セルのように
  /// 内部段落へ字下げを波及させたくないブロックは [`LoweringContext::with_first_line_indent`]
  /// で 0 にリセットした文脈を本体に渡す。`quotation` ブロックは逆に正の値を設定する。
  pub first_line_indent: types::Length,
}

impl<'a> LoweringContext<'a> {
  /// 新しい `LoweringContext` を生成する
  ///
  /// # Arguments
  ///
  /// * `style` - `read_style::read_style()` の結果への参照
  #[must_use]
  pub fn new(style: &'a ReadStyle) -> Self {
    return LoweringContext {
      style,
      body_font_kind: style.text.font_kind,
      first_line_indent: style.text.first_line_indent,
    };
  }

  /// 本文段落の既定フォント種別だけを差し替えた派生文脈を返す
  ///
  /// `style` 参照は共有したまま `body_font_kind` のみ上書きする。定理本体を斜体・証明本体を
  /// ローマンで lower する際に、本体ノード列の lowering へ渡す。
  #[must_use]
  pub fn with_body_font_kind(&self, body_font_kind: types::FontKind) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind,
      first_line_indent: self.first_line_indent,
    };
  }

  /// 段落先頭行の字下げ量だけを差し替えた派生文脈を返す
  ///
  /// `style` 参照・`body_font_kind` は共有したまま `first_line_indent` のみ上書きする。
  /// リスト項目・表セル・定理本体などへ字下げを波及させないため 0 にリセットしたり、
  /// `quotation` ブロックで正の値を与えたりするのに使う。
  #[must_use]
  pub fn with_first_line_indent(&self, first_line_indent: types::Length) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind: self.body_font_kind,
      first_line_indent,
    };
  }

  /// 既定フォントサイズ（段落本文用、`style.text.font_size` に等しい）を pt 値で返すヘルパー
  #[must_use]
  pub fn default_font_size(&self) -> f32 { return self.style.text.font_size.to_pt(); }
}

/// Document IR をレイアウトノードに変換する（ドキュメント全体）
///
/// # Arguments
///
/// * `document` - `Document` 構造体
///
/// # Returns
///
/// レイアウトノードのリスト
///
/// # Errors
///
/// 内部で呼び出す [`lower_nodes`] が返すエラーをそのまま伝播します。
pub fn lower_document(ctx: &LoweringContext, document: &Document) -> Result<Vec<LayoutNode>, LoweringError> {
  return lower_nodes(ctx, &document.body);
}

/// `DocNode` のリストをレイアウトノードに変換する
///
/// `build_pdf.rs` から呼ばれる lowering のエントリーポイント。
///
/// # Errors
///
/// いずれかの `DocNode` の変換中に [`LoweringError`] が発生した場合に返します。
pub fn lower_nodes(ctx: &LoweringContext, nodes: &[DocNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut result = Vec::new();
  // 見出しの文書順インデックス。`heading_anchor_key` で暗黙 destination キーを採番するのに使う
  // （目次エントリの内部リンク到達先と一致させるため、`document::collect_headings` の index と同じ規則）。
  let mut heading_index = 0;
  for node in nodes {
    result.extend(lower_node_indexed(ctx, node, heading_index)?);
    if node.is_heading() {
      heading_index += 1;
    }
  }
  debug!(input_node_count = nodes.len(), layout_node_count = result.len(), "lowering が完了しました");
  return Ok(result);
}

/// 単一の `DocNode` をレイアウトノードに変換する（見出しインデックス 0 固定の単体エントリ）
///
/// 見出しの暗黙キーが文書順に依存しないテスト・単発変換用。本文全体の変換は
/// [`lower_nodes`] が [`lower_node_indexed`] を介して連番キーを与える。
#[cfg(test)]
fn lower_node(ctx: &LoweringContext, node: &DocNode) -> Result<Vec<LayoutNode>, LoweringError> {
  return lower_node_indexed(ctx, node, 0);
}

/// 単一の `DocNode` をレイアウトノードに変換する
///
/// `heading_index` は見出しの文書順インデックス（[`document::heading_anchor_key`] に渡す）。
fn lower_node_indexed(
  ctx: &LoweringContext,
  node: &DocNode,
  heading_index: usize,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match node {
    DocNode::Heading {
      level,
      number,
      title,
      label,
    } => {
      return heading::lower_heading(ctx, *level, number, title, label.clone(), heading_index);
    },
    DocNode::Paragraph(inlines) => {
      return paragraph::lower_paragraph(ctx, inlines);
    },
    DocNode::List { ordered, items } => {
      return list::lower_list(ctx, *ordered, items);
    },
    DocNode::Theorem {
      class,
      number,
      title,
      body,
      of,
      label,
    } => {
      // 見出し（独立行）＋ クラス別 font_kind 本文 ＋ 上下マージン、proof は末尾に QED。
      // proof の `of` は pass2 で解決済みの cleveref 文字列（例「Theorem 1」）を見出しに織り込む。
      let of = of.as_ref().and_then(|target| target.number.as_deref());
      return theorem::lower_theorem(ctx, *class, number.as_deref(), title.as_deref(), body, of, label.as_deref());
    },
    DocNode::Quote { kind, body } => {
      return quote::lower_quote(ctx, *kind, body);
    },
    DocNode::Rule { width, height } => {
      return Ok(vec![LayoutNode::Rule {
        width: *width,
        height: *height,
      }]);
    },
    DocNode::PageBreak => {
      return Ok(vec![LayoutNode::PageBreak]);
    },
    DocNode::Space(length) => {
      return Ok(vec![LayoutNode::Kern { length: *length }]);
    },
    DocNode::Anchor(target) => {
      // CSL 整形ステージが付与した参照アンカー点（参考文献エントリの直前）。
      // ラベル付きブロックと同じ `AnchorMark::Label` でジャンプ先に解決させる。
      return Ok(vec![LayoutNode::Anchor(types::AnchorMark::Label(target.clone()))]);
    },
    DocNode::MathBlock {
      kind,
      rows,
      number,
      label,
    } => {
      let block = &ctx.style.math.block;
      let mut nodes = vec![
        LayoutNode::Vkern {
          length: block.top_margin,
        },
        math::lower_math_block(ctx, *kind, rows, number.as_deref()),
        LayoutNode::Vkern {
          length: block.bottom_margin,
        },
      ];
      // ラベル付き行（`equation` の `[label=...]`、`align` / `gather` の行末 `\label{...}`）の `\ref`
      // 到達先アンカーを先頭に付ける。複数行がラベルを持つ場合も、いずれもブロック先頭座標に解決される。
      for row in rows {
        if let Some(label) = &row.label {
          nodes = with_label_anchor(Some(label), nodes);
        }
      }
      // 環境単位ラベル（`split` / `multiline` の `[label=...]`）も同様にブロック先頭へ解決する
      nodes = with_label_anchor(label.as_deref(), nodes);
      return Ok(nodes);
    },
    DocNode::Figure {
      image_path,
      width,
      height,
      dpi,
      downsample,
      caption,
      caption_position,
      number,
      label,
    } => {
      let caption_arg = caption.as_deref().map(|inlines| (*caption_position, inlines));
      let overrides = figure::ImageOverrides {
        dpi: *dpi,
        downsample: *downsample,
      };
      let nodes = figure::lower_figure(ctx, image_path, *width, *height, overrides, caption_arg, number)?;
      return Ok(with_label_anchor(label.as_deref(), nodes));
    },
    DocNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      number,
      breakable,
      label,
    } => {
      let caption_arg = caption.as_deref().map(|inlines| (*caption_position, inlines));
      let nodes = table::lower_table(ctx, columns, widths, head, rows, caption_arg, number, *breakable)?;
      return Ok(with_label_anchor(label.as_deref(), nodes));
    },
  }
}

/// ラベル付きブロック（図・表・ディスプレイ数式）の先頭に `\ref` 到達先アンカーを付与する
///
/// `label` が `None`（参照対象でない）の場合はそのまま返す。見出しは [`heading::lower_heading`]
/// が `AnchorMark::Heading` を別途出すため、ここでは扱わない。
fn with_label_anchor(label: Option<&str>, nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  let Some(label) = label else {
    return nodes;
  };
  let mut result = Vec::with_capacity(nodes.len() + 1);
  result.push(LayoutNode::Anchor(types::AnchorMark::Label(label.to_string())));
  result.extend(nodes);
  return result;
}

#[cfg(test)]
mod tests {
  use document::{HeadingLevel, InlineNode, ListItem, MathEnvKind, MathNode, MathRow};
  use types::Length;

  use super::*;

  /// 1 行 1 セルの `equation` 相当 `DocNode::MathBlock` を作るテストヘルパ
  fn equation_block(number: Option<&str>, label: Option<&str>) -> DocNode {
    return DocNode::MathBlock {
      kind: MathEnvKind::Equation,
      rows: vec![MathRow {
        cells: vec![vec![MathNode::Text("a".to_string())]],
        number: number.map(str::to_string),
        label: label.map(str::to_string),
      }],
      number: None,
      // equation はラベルを行側（`MathRow::label`）に持つため、環境単位ラベルは None
      label: None,
    };
  }

  /// レイアウトノード木を再帰的に走査し、`LineBreak` が含まれるか調べるヘルパ
  fn contains_line_break(nodes: &[LayoutNode]) -> bool {
    return nodes.iter().any(|n| match n {
      LayoutNode::LineBreak => true,
      LayoutNode::VBox { children, .. } | LayoutNode::HBox { children, .. } | LayoutNode::Raise { children, .. } => {
        contains_line_break(children)
      },
      LayoutNode::Table(table) => table
        .head
        .iter()
        .chain(table.rows.iter())
        .any(|row| row.cells.iter().any(|cell| contains_line_break(&cell.content))),
      _ => false,
    });
  }

  #[test]
  fn test_lower_space() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Space(Length::pt(5.0));

    // Act
    let result = lower_node(&ctx, &node).expect("Space の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Kern { length } => assert!((length.to_pt() - 5.0).abs() < f32::EPSILON),
      other => panic!("Expected Kern, got {other:?}"),
    }
  }

  #[test]
  fn test_lower_anchor() {
    // Arrange — DocNode::Anchor は LayoutNode::Anchor(AnchorMark::Label(..)) に 1:1 変換される
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Anchor("cite:foo".to_string());

    // Act
    let result = lower_node(&ctx, &node).expect("Anchor の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(&result[0], LayoutNode::Anchor(types::AnchorMark::Label(l)) if l == "cite:foo"));
  }

  #[test]
  fn test_lower_page_break() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::PageBreak;

    // Act
    let result = lower_node(&ctx, &node).expect("PageBreak の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    assert!(matches!(result[0], LayoutNode::PageBreak));
  }

  #[test]
  fn test_lower_rule() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Rule {
      width: Length::pt(100.0),
      height: Length::pt(1.0),
    };

    // Act
    let result = lower_node(&ctx, &node).expect("Rule の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Rule { width, height } => {
        assert!((width.to_pt() - 100.0).abs() < f32::EPSILON);
        assert!((height.to_pt() - 1.0).abs() < f32::EPSILON);
      },
      other => panic!("Expected Rule, got {other:?}"),
    }
  }

  #[test]
  fn unresolved_ref_in_paragraph_returns_error() {
    // 評価器の pass2 が走らずに number = None のまま lowering に到達した場合、
    // LoweringError::UnresolvedReference が返ることを確認する。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Paragraph(vec![InlineNode::Ref {
      label: "ch:intro".to_string(),
      number: None,
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }]);

    let err = lower_node(&ctx, &node).expect_err("未解決の Ref は LoweringError を返すべき");

    let LoweringError::UnresolvedReference { label, .. } = err else {
      panic!("UnresolvedReference が期待されます: {err:?}");
    };
    assert_eq!(label, "ch:intro");
  }

  #[test]
  fn unprocessed_cite_in_paragraph_returns_error() {
    // CSL 整形ステージ（citation::process_citations）を経ずに label = None のまま
    // lowering に到達した場合、LoweringError::UnresolvedCitation が返ることを確認する。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Paragraph(vec![InlineNode::Cite {
      keys: vec!["smith2020".to_string(), "jones2021".to_string()],
      label: None,
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }]);

    let err = lower_node(&ctx, &node).expect_err("未整形の Cite は LoweringError を返すべき");

    let LoweringError::UnresolvedCitation { keys, .. } = err else {
      panic!("UnresolvedCitation が期待されます: {err:?}");
    };
    assert_eq!(keys, "smith2020, jones2021");
  }

  #[test]
  fn lower_inline_math_replaces_placeholder() {
    // 統合テスト: 段落内の InlineMath(InlineNode) が "[Math]" プレースホルダではなく
    // 実際の LayoutNode 列に展開されること
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let para = DocNode::Paragraph(vec![InlineNode::InlineMath(vec![
      MathNode::Text("x".to_string()),
      MathNode::Superscript(Box::new(MathNode::Text("2".to_string()))),
    ])]);

    let nodes = lower_node(&ctx, &para).expect("paragraph lowering は失敗しないはず");

    // [Math] というリテラル文字列は出てこない
    let placeholder = nodes.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "[Math]"));
    assert!(!placeholder, "[Math] プレースホルダは消えているはず: {nodes:?}");
    // Raise（上付き由来）が含まれる
    let has_raise = nodes.iter().any(|n| matches!(n, LayoutNode::Raise { .. }));
    assert!(has_raise, "上付き由来の Raise が含まれるはず: {nodes:?}");
  }

  #[test]
  fn lower_math_block_wraps_with_vkerns_and_emits_no_line_break() {
    // ディスプレイ数式は Vkern(top) → MathBlock → Vkern(bottom) で包まれ、LineBreak は出力しない
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = equation_block(None, None);

    let nodes = lower_node(&ctx, &node).expect("display math lowering は失敗しないはず");

    // 先頭: Vkern(top_margin)、中央: MathBlock、末尾: Vkern(bottom_margin)
    assert_eq!(nodes.len(), 3, "Vkern + MathBlock + Vkern の 3 要素: {nodes:?}");
    assert!(matches!(nodes.first(), Some(LayoutNode::Vkern { .. })), "先頭は Vkern であるべき: {nodes:?}");
    assert!(matches!(nodes.get(1), Some(LayoutNode::MathBlock { .. })), "中央は MathBlock であるべき: {nodes:?}");
    assert!(matches!(nodes.last(), Some(LayoutNode::Vkern { .. })), "末尾は Vkern であるべき: {nodes:?}");
    let has_line_break = nodes.iter().any(|n| matches!(n, LayoutNode::LineBreak));
    assert!(!has_line_break, "LineBreak は出力されないはず: {nodes:?}");
  }

  #[test]
  fn lower_math_block_carries_numbered_row() {
    // number = Some("1") のとき MathBlock の行に番号ボックスが乗る
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = equation_block(Some("1"), None);

    let nodes = lower_node(&ctx, &node).expect("display math lowering は失敗しないはず");

    let Some(LayoutNode::MathBlock { rows, .. }) = nodes.get(1) else {
      panic!("中央に MathBlock があるべき: {nodes:?}");
    };
    assert_eq!(rows.len(), 1, "equation は 1 行: {rows:?}");
    let number = rows[0].number.as_ref().expect("採番された行は番号ボックスを持つ");
    assert!(
      matches!(&number[0], LayoutNode::Text(t, _) if t == "(1)"),
      "番号ボックスは Text(\"(1)\"): {number:?}"
    );
  }

  #[test]
  fn lower_nodes_dispatches_each_variant_in_order() {
    // Arrange — 見出し / 段落 / リスト / 改ページを並べる
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Section, "1", vec![InlineNode::Text("H".to_string())]),
      DocNode::Paragraph(vec![InlineNode::Text("P".to_string())]),
      DocNode::List {
        ordered: false,
        items: vec![ListItem::new(vec![DocNode::Paragraph(vec![
          InlineNode::Text("L".to_string()),
        ])])],
      },
      DocNode::PageBreak,
    ];

    // Act
    let out = lower_nodes(&ctx, &nodes).expect("解決済みノードのみなので失敗しない");

    // Assert — 見出しとリスト項目で VBox が 2 つ以上、段落由来の Text("P")、末尾は PageBreak
    let vbox_count = out.iter().filter(|n| matches!(n, LayoutNode::VBox { .. })).count();
    assert!(vbox_count >= 2, "見出しとリスト項目で VBox が複数出る: {out:?}");
    assert!(out.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "P")), "段落 Text が出る: {out:?}");
    assert!(matches!(out.last(), Some(LayoutNode::PageBreak)), "末尾は PageBreak: {out:?}");
  }

  #[test]
  fn lower_document_delegates_to_lower_nodes() {
    // Arrange — Document の body がそのまま lower_nodes に渡ることを確認する
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let document = Document::new(vec![
      DocNode::heading(HeadingLevel::Chapter, "1", vec![InlineNode::Text("Intro".to_string())]),
      DocNode::Paragraph(vec![InlineNode::Text("Body".to_string())]),
    ]);

    // Act
    let out = lower_document(&ctx, &document).expect("失敗しない");

    // Assert — 見出し VBox と段落 Text の両方が出る
    assert!(out.iter().any(|n| matches!(n, LayoutNode::VBox { .. })));
    assert!(out.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "Body")));
  }

  #[test]
  fn block_boundaries_use_no_bare_line_break() {
    // Arrange — インライン \\（LineBreak）を一切含まないブロック構成
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Section, "1", vec![InlineNode::Text("Heading".to_string())]),
      DocNode::Paragraph(vec![InlineNode::Text("Para".to_string())]),
      DocNode::List {
        ordered: true,
        items: vec![ListItem::new(vec![DocNode::Paragraph(vec![
          InlineNode::Text("Item".to_string()),
        ])])],
      },
    ];

    // Act
    let out = lower_nodes(&ctx, &nodes).expect("失敗しない");

    // Assert — ブロック境界は Vkern / VBox.margin_bottom で表され、裸の LineBreak は出ない
    assert!(!contains_line_break(&out), "段落内 \\\\ 以外で LineBreak は出力されない: {out:?}");
  }

  #[test]
  fn labeled_display_math_emits_label_anchor() {
    // Arrange — label 付きディスプレイ数式は先頭に AnchorMark::Label を出す
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = equation_block(Some("1"), Some("eq:foo"));

    // Act
    let nodes = lower_node(&ctx, &node).expect("失敗しない");

    // Assert — 先頭が Anchor(Label("eq:foo"))
    assert!(
      matches!(nodes.first(), Some(LayoutNode::Anchor(types::AnchorMark::Label(l))) if l == "eq:foo"),
      "先頭は Label アンカー: {nodes:?}"
    );
  }

  #[test]
  fn unlabeled_display_math_emits_no_anchor() {
    // Arrange — label なしのディスプレイ数式はアンカーを出さない
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = equation_block(Some("1"), None);

    // Act
    let nodes = lower_node(&ctx, &node).expect("失敗しない");

    // Assert — Anchor は含まれない
    assert!(!nodes.iter().any(|n| matches!(n, LayoutNode::Anchor(_))), "アンカーは出ない: {nodes:?}");
  }

  #[test]
  fn default_font_size_reflects_core_font_size() {
    // Arrange — text.font_size を 18pt に上書きする（#124 で [text] に集約）
    let mut style = read_style::Style::default();
    style.text.font_size = Length::pt(18.0);
    let ctx = LoweringContext::new(&style);

    // Act
    let out = lower_node(&ctx, &DocNode::Paragraph(vec![InlineNode::Text("x".to_string())])).expect("失敗しない");

    // Assert — default_font_size と段落 Text の font_size がともに 18pt
    assert!((ctx.default_font_size() - 18.0).abs() < f32::EPSILON);
    let LayoutNode::Text(_, text_style) = &out[0] else {
      panic!("先頭は Text であるべき: {out:?}");
    };
    assert!((text_style.font_size - 18.0).abs() < f32::EPSILON);
  }
}
