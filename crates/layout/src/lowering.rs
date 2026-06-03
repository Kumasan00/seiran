//! Lowering 層: Document IR → `LayoutNode` 変換
//!
//! `parser` クレートの `DocNode`（セマンティックな論理構造）を
//! `LayoutNode`（物理的なレイアウト表現）に変換するモジュールです。
//!
//! ## アーキテクチャ上の位置づけ
//!
//! ```text
//! parser (DocNode)
//!   ↓ [lowering]  ← このモジュール
//! LayoutNode
//!   ↓ [layout_engine]
//! Item (Box/Glue/Penalty)
//!   ↓ [pdf_gen]
//! PDF bytes
//! ```
//!
//! ## 責務
//!
//! - 見出しレベルに応じたフォントサイズ・フォント種別の決定
//! - 見出し番号のフォーマット（「1.2.3」等）
//! - 段落スタイル（フォントサイズ、行間等）の付与
//! - リストのインデント・マーカー生成
//! - スタイルシート（`read_style` クレート）を `LoweringContext` 経由で受け取り、
//!   見出しレベルごとのフォントサイズ・フォーマット文字列・余白を適用する

use miette::Diagnostic;
use parser::document::{DocNode, Document, HeadingLevel, HeadingNumber, InlineNode, ListItem, MathNode, MathStyle};
use read_style::Style as ReadStyle;
use thiserror::Error;
use types::FontKind;

use crate::layout_node::{LayoutNode, Style};

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
    code(layout::lowering::unresolved_reference),
    help(
      "対応する \\label が定義されているか確認してください。定義されている場合は評価器の参照解決パスにバグがある可能性があります。"
    )
  )]
  UnresolvedReference {
    /// 解決できなかったラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
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
}

impl<'a> LoweringContext<'a> {
  /// 新しい `LoweringContext` を生成する
  ///
  /// # Arguments
  ///
  /// * `style` - `read_style::read_style()` の結果への参照
  #[must_use]
  pub fn new(style: &'a ReadStyle) -> Self { return LoweringContext { style }; }

  /// 既定フォントサイズ（段落本文用、`style.font_size` に等しい）を返すヘルパー
  #[must_use]
  pub fn default_font_size(&self) -> f32 { return self.style.font_size; }
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
  for node in nodes {
    result.extend(lower_node(ctx, node)?);
  }
  return Ok(result);
}

/// 単一の `DocNode` をレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] 各バリアントの変換関数を本実装にする（現在は TODO スタブ）
fn lower_node(ctx: &LoweringContext, node: &DocNode) -> Result<Vec<LayoutNode>, LoweringError> {
  match node {
    DocNode::Heading {
      level,
      number,
      title,
      ..
    } => {
      return lower_heading(ctx, *level, number, title);
    },
    DocNode::Paragraph(inlines) => {
      return lower_paragraph(ctx, inlines);
    },
    DocNode::List { ordered, items } => {
      return lower_list(ctx, *ordered, items);
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
    DocNode::Space(pt) => {
      return Ok(vec![LayoutNode::Kern { point: *pt }]);
    },
    DocNode::DisplayMath { body, .. } => {
      return Ok(lower_display_math(ctx, body));
    },
    DocNode::Figure { body, .. } => {
      // TODO(figure-equation-impl): caption 抽出と図のレイアウト。前準備では body を素通し。
      return lower_nodes(ctx, body);
    },
    DocNode::Image { path, .. } => {
      // TODO(figure-equation-impl): 実画像埋め込み。前準備ではパスをテキスト出力。
      let style = Style {
        font_size: ctx.default_font_size(),
        font_kind: FontKind::Monospace,
      };
      return Ok(vec![LayoutNode::Text(format!("[Image: {path}]"), style)]);
    },
  }
}

/// `DocNode::DisplayMath`（`\begin{equation}...\end{equation}`）を `LayoutNode` 列に変換する
///
/// インライン数式と同じ [`lower_inline_math`] を流用しつつ、数式を独立した行に配置する。
/// 真の中央寄せには行幅の知識が必要なため、現段階では行頭からのレンダリングとし、
/// 前後に改行を挿入して段落から分離する。
fn lower_display_math(ctx: &LoweringContext, body: &[MathNode]) -> Vec<LayoutNode> {
  let font_size = ctx.default_font_size();
  let mut result = Vec::new();
  result.push(LayoutNode::LineBreak);
  result.extend(lower_inline_math(body, font_size));
  result.push(LayoutNode::LineBreak);
  return result;
}

// =============================================================================
// 見出しの変換
// =============================================================================

/// 見出しレベルに対応する `read_style::HeadingStyle` を返す
fn heading_style_for(style: &ReadStyle, level: HeadingLevel) -> &read_style::HeadingStyle {
  return match level {
    HeadingLevel::Part => &style.part,
    HeadingLevel::Chapter => &style.chapter,
    HeadingLevel::Section => &style.section,
    HeadingLevel::Subsection => &style.subsection,
    HeadingLevel::Paragraph => &style.paragraph,
    HeadingLevel::Subparagraph => &style.subparagraph,
  };
}

/// `HeadingStyle.format` テンプレートの `{number}` と `{title}` を実値で置換する
///
/// - `{number}` は `HeadingNumber::dotted()` の値で置換（Part: `1`、Chapter: `1`、Section: `1.2`、…）
/// - `{title}` は引数 `title` のプレーンテキストで置換
///
/// レベルごとに番号粒度を分ける必要はない。`HeadingNumber::from_context` が
/// レベルに応じて適切な要素列を返すため、`dotted()` だけで番号書式が決まる。
fn format_heading_text(number: &HeadingNumber, title: &str, template: &str) -> String {
  return template.replace("{number}", &number.dotted()).replace("{title}", title);
}

/// 見出しをレイアウトノードに変換する
///
/// 見出し番号 + タイトルをスタイル付きテキストとして `VBox` に配置します。
/// Part レベルの場合は `PageBreak` を先行して出力します。
///
/// ## TODO
///
/// - [ ] 見出し番号のフォントスタイル（色、太さ等）を細かくカスタマイズ可能にする
/// - [ ] PDF ブックマーク生成に必要な情報（ページ番号、座標等）をここで収集する
///   → または別パスで Document IR を走査して収集する
/// - [ ] 見出し前後のスペース（`margin_top` 等）を追加する
fn lower_heading(
  ctx: &LoweringContext,
  level: HeadingLevel,
  number: &HeadingNumber,
  title: &[InlineNode],
) -> Result<Vec<LayoutNode>, LoweringError> {
  let heading_style = heading_style_for(ctx.style, level);
  let style = Style {
    font_size: heading_style.font_size,
    font_kind: FontKind::SerifBold,
  };

  // タイトルのインライン要素を一旦プレーン化し、テンプレ展開で番号と結合する
  // TODO: 強調等のインライン要素のスタイルを保持したまま埋め込む（本実装タスク）
  let title_text = inline_nodes_to_plain_text(title)?;
  let heading_text = format_heading_text(number, &title_text, &heading_style.format);

  let mut result = Vec::new();

  if heading_style.page_break_before {
    result.push(LayoutNode::PageBreak);
  }

  result.push(LayoutNode::VBox {
    children: vec![LayoutNode::Text(heading_text, style)],
    margin_bottom: heading_style.bottom_margin,
  });

  if heading_style.page_break_after {
    result.push(LayoutNode::PageBreak);
  } else {
    result.push(LayoutNode::LineBreak);
  }

  return Ok(result);
}

// =============================================================================
// 段落の変換
// =============================================================================

/// 段落をレイアウトノードに変換する
///
/// 段落内のインライン要素を展開し、フラットな `LayoutNode::Text` のリストとして
/// レイアウトノードに変換します。段落間にはデフォルトのスペースを挿入します。
///
/// ## TODO
///
/// - [ ] インライン要素のスタイル変更（Emphasis → Italic 等）を反映する
/// - [ ] 段落先頭のインデント（字下げ）を追加する
/// - [ ] 段落間スペースをスタイル設定で定義可能にする
/// - [ ] 段落内テキストの結合最適化（evaluator.rs の `merge_text` に相当するロジック）
fn lower_paragraph(ctx: &LoweringContext, inlines: &[InlineNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let default_style = Style {
    font_size: ctx.default_font_size(),
    font_kind: FontKind::Serif,
  };

  let mut result = Vec::new();

  for inline in inlines {
    result.extend(lower_inline(ctx, inline, default_style)?);
  }

  // 段落末に改行 + カーンを追加（段落間スペース）
  // TODO: 段落間スペースをスタイル設定で制御する
  result.push(LayoutNode::LineBreak);
  result.push(LayoutNode::Kern {
    point: ctx.default_font_size(),
  });

  return Ok(result);
}

// =============================================================================
// インライン要素の変換
// =============================================================================

/// インライン要素をレイアウトノードに変換する
///
/// 親から継承されたスタイル（`parent_style`）を基に、インライン要素の種類に応じて
/// フォント種別やサイズを変更します。
///
/// ## TODO
///
/// - [ ] Emphasis / Strong のネスト対応（イタリック内の強調 → ボールドイタリック等）
/// - [ ] スタイルスタック方式に変更して、任意深さのネストに対応する
#[allow(clippy::used_underscore_binding)]
fn lower_inline(
  _ctx: &LoweringContext,
  inline: &InlineNode,
  parent_style: Style,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match inline {
    InlineNode::Text(text) => {
      return Ok(vec![LayoutNode::Text(text.clone(), parent_style)]);
    },
    InlineNode::Emphasis(children) => {
      // TODO: ネスト対応（イタリック内の強調は通常体に戻す等）
      let italic_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::SerifItalic,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, italic_style)?);
      }
      return Ok(result);
    },
    InlineNode::Strong(children) => {
      let bold_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::SerifBold,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, bold_style)?);
      }
      return Ok(result);
    },
    InlineNode::Code(children) => {
      let mono_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::Monospace,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, mono_style)?);
      }
      return Ok(result);
    },
    InlineNode::SansSerif(children) => {
      let sans_style = Style {
        font_size: parent_style.font_size,
        font_kind: FontKind::SansSerif,
      };
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_inline(_ctx, child, sans_style)?);
      }
      return Ok(result);
    },
    InlineNode::InlineMath(math_nodes) => {
      return Ok(lower_inline_math(math_nodes, parent_style.font_size));
    },
    InlineNode::Symbol(ch) => {
      return Ok(vec![LayoutNode::Text(ch.to_string(), parent_style)]);
    },
    InlineNode::LineBreak => {
      return Ok(vec![LayoutNode::LineBreak]);
    },
    InlineNode::Ref { label, number } => {
      // 評価器の pass2 で参照解決が済んでいれば number は Some。未解決のまま
      // lowering に到達した場合は `LoweringError::UnresolvedReference` で報告する。
      let Some(resolved) = number.clone() else {
        return Err(LoweringError::UnresolvedReference {
          label: label.clone(),
        });
      };
      return Ok(vec![LayoutNode::Text(resolved, parent_style)]);
    },
  }
}

// =============================================================================
// リストの変換
// =============================================================================

/// リストをレイアウトノードに変換する
///
/// ## TODO
///
/// - [ ] リストのネスト対応（ネストレベルに応じたインデント量の変更）
/// - [ ] 順序付きリスト（enumerate）のマーカー生成（1., 2., 3. 等）
/// - [ ] 順序なしリストのマーカー生成（•, -, ▪ 等、ネストレベルで変更）
/// - [ ] マーカーのフォント・サイズをスタイル設定で制御する
/// - [ ] リスト前後のスペースをスタイル設定で制御する
fn lower_list(ctx: &LoweringContext, ordered: bool, items: &[ListItem]) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut result = Vec::new();
  let indent = 20.0; // TODO: スタイル設定から取得する

  for (i, item) in items.iter().enumerate() {
    // マーカーの生成
    // TODO: ネストレベルに応じたマーカーの変更
    let marker = if ordered {
      format!("{}. ", i + 1)
    } else {
      "• ".to_string()
    };

    let marker_style = Style {
      font_size: ctx.default_font_size(),
      font_kind: FontKind::Serif,
    };

    // インデント + マーカー + 内容
    let mut item_nodes = Vec::new();
    item_nodes.push(LayoutNode::Kern { point: indent });
    item_nodes.push(LayoutNode::Text(marker, marker_style));

    // アイテム内容を変換
    let content_nodes = lower_nodes(ctx, &item.content)?;
    item_nodes.extend(content_nodes);

    result.push(LayoutNode::VBox {
      children: item_nodes,
      margin_bottom: 4.0, // TODO: スタイル設定から取得する
    });
  }

  return Ok(result);
}

// =============================================================================
// 数式の変換
// =============================================================================

/// 上付きスクリプトのフォントサイズ倍率（親フォントサイズに対する比）
const SCRIPT_SIZE_FACTOR: f32 = 0.7;
/// 上付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で上方向）
const SUPERSCRIPT_RAISE_FACTOR: f32 = 0.4;
/// 下付きスクリプトのベースラインシフト（親フォントサイズに対する比、正で下方向）
const SUBSCRIPT_DROP_FACTOR: f32 = 0.2;

/// インライン数式（`$...$`）を `LayoutNode` 列に変換する
///
/// 数式中の文字はすべて `FontKind::Math` を割り当てつつ、入力コードポイントを
/// Unicode Mathematical Alphanumeric Symbols（U+1D400–U+1D7FF）へ変換することで
/// 数式フォントが持つ字形バリアントを直接呼び出す。デフォルト（スタイル指定なし）では
/// ASCII 英字のみ Mathematical Italic 化し、変数を italic で描画する。
/// 上付き・下付きは [`LayoutNode::Raise`] で縦シフトしつつ、フォントサイズを
/// [`SCRIPT_SIZE_FACTOR`] 倍に縮小して描画する。
fn lower_inline_math(math_nodes: &[MathNode], base_font_size: f32) -> Vec<LayoutNode> {
  let mut result = Vec::new();
  for node in math_nodes {
    result.extend(lower_math_node(node, base_font_size, None));
  }
  return result;
}

/// 単一の `MathNode` を `LayoutNode` 列に変換する
///
/// `style` パラメータは、外側の `MathNode::Styled` から継承する数式スタイル。
/// `None` のときはデフォルト挙動（ASCII 英字のみ italic 化）。`Some(_)` のときは
/// ASCII 英字・数字・Greek を該当スタイルへコードポイント変換する。
/// `Styled` バリアントは内側の `style` で完全上書きする（ネストは内側優先）。
fn lower_math_node(node: &MathNode, font_size: f32, style: Option<MathStyle>) -> Vec<LayoutNode> {
  match node {
    MathNode::Text(s) => {
      return lower_math_text(s, font_size, style);
    },
    MathNode::Symbol(ch) => {
      let translated = translate_math_char(*ch, style);
      let layout_style = Style {
        font_size,
        font_kind: FontKind::Math,
      };
      return vec![LayoutNode::Text(translated.to_string(), layout_style)];
    },
    MathNode::Group(children) => {
      let mut result = Vec::new();
      for child in children {
        result.extend(lower_math_node(child, font_size, style));
      }
      return result;
    },
    MathNode::Superscript(inner) => {
      let script_size = (font_size * SCRIPT_SIZE_FACTOR).max(6.0);
      let children = lower_math_node(inner.as_ref(), script_size, style);
      return vec![LayoutNode::Raise {
        dy: font_size * SUPERSCRIPT_RAISE_FACTOR,
        children,
      }];
    },
    MathNode::Subscript(inner) => {
      let script_size = (font_size * SCRIPT_SIZE_FACTOR).max(6.0);
      let children = lower_math_node(inner.as_ref(), script_size, style);
      return vec![LayoutNode::Raise {
        dy: -font_size * SUBSCRIPT_DROP_FACTOR,
        children,
      }];
    },
    MathNode::Frac { numer, denom } => {
      // インラインでは真の縦書き分数は無理なので、`a / b` の形式で代替する
      let slash_style = Style {
        font_size,
        font_kind: FontKind::Math,
      };
      let mut result = Vec::new();
      result.extend(lower_math_node(numer.as_ref(), font_size, style));
      result.push(LayoutNode::Text("/".to_string(), slash_style));
      result.extend(lower_math_node(denom.as_ref(), font_size, style));
      return result;
    },
    MathNode::Sqrt { index, radicand } => {
      let upright_style = Style {
        font_size,
        font_kind: FontKind::Math,
      };
      let mut result = Vec::new();
      if let Some(idx) = index {
        let script_size = (font_size * SCRIPT_SIZE_FACTOR).max(6.0);
        let idx_children = lower_math_node(idx.as_ref(), script_size, style);
        result.push(LayoutNode::Raise {
          dy: font_size * SUPERSCRIPT_RAISE_FACTOR,
          children: idx_children,
        });
      }
      result.push(LayoutNode::Text("√".to_string(), upright_style));
      result.extend(lower_math_node(radicand.as_ref(), font_size, style));
      return result;
    },
    MathNode::Command { name, args } => {
      // 未解決の数式コマンドはコマンド名と引数をリテラル表示してデバッグしやすくする
      let literal_style = Style {
        font_size,
        font_kind: FontKind::Math,
      };
      let mut result = Vec::new();
      result.push(LayoutNode::Text(format!("\\{name}"), literal_style));
      for arg in args {
        result.push(LayoutNode::Text("{".to_string(), literal_style));
        for child in arg {
          result.extend(lower_math_node(child, font_size, style));
        }
        result.push(LayoutNode::Text("}".to_string(), literal_style));
      }
      return result;
    },
    MathNode::Styled {
      style: inner_style,
      body,
    } => {
      let mut result = Vec::new();
      for child in body {
        result.extend(lower_math_node(child, font_size, Some(*inner_style)));
      }
      return result;
    },
    MathNode::AlignmentMark => {
      // インライン数式では `&` は意味を持たないので無視する
      return Vec::new();
    },
  }
}

/// 数式中のテキスト文字列を `LayoutNode` 列に変換する
///
/// 文字単位で [`translate_math_char`] を適用してから 1 つの `LayoutNode::Text` にまとめる。
/// `FontKind::Math` は常に維持する（テキストフォントへの切替は行わない）。
/// 数式中に和文が混入した場合のスクリプト別フォント切替は後段の
/// `split_text_by_script` / `resolve_font_type` が担うため、ここでは分割しない。
fn lower_math_text(text: &str, font_size: f32, style: Option<MathStyle>) -> Vec<LayoutNode> {
  if text.is_empty() {
    return Vec::new();
  }
  let translated: String = text.chars().map(|c| translate_math_char(c, style)).collect();
  let layout_style = Style {
    font_size,
    font_kind: FontKind::Math,
  };
  return vec![LayoutNode::Text(translated, layout_style)];
}

// =============================================================================
// Mathematical Alphanumeric Symbols へのコードポイント変換
// =============================================================================

/// 1 文字を `style` に応じた Mathematical Alphanumeric コードポイントへ変換する
///
/// - `style == None`: ASCII 英字のみ Mathematical Italic（小文字 `h` は U+210E PLANCK CONSTANT）。
///   数字・Greek・記号は素通し
/// - `style == Some(_)`: ASCII 英字・ASCII 数字・Greek 文字を該当スタイルのコードポイントに変換。
///   当該スタイル × 種別の組み合わせが Unicode に存在しない場合（例: `\mathmono` の Greek、
///   italic の数字）は素通しでフォント側のグリフ選択に委ねる
fn translate_math_char(ch: char, style: Option<MathStyle>) -> char {
  // 共通ヘルパ: ASCII 英字を base に応じてシフト
  let map_ascii = |ch: char, upper_base: u32, lower_base: u32, italic_h_exception: bool| -> char {
    match ch {
      'A'..='Z' => char::from_u32(upper_base + (ch as u32 - 'A' as u32)).unwrap_or(ch),
      'h' if italic_h_exception => '\u{210E}',
      'a'..='z' => char::from_u32(lower_base + (ch as u32 - 'a' as u32)).unwrap_or(ch),
      _ => ch,
    }
  };

  // 共通ヘルパ: ASCII 数字を base に応じてシフト
  let map_digit = |ch: char, base: u32| -> char {
    match ch {
      '0'..='9' => char::from_u32(base + (ch as u32 - '0' as u32)).unwrap_or(ch),
      _ => ch,
    }
  };

  // 共通ヘルパ: Greek 文字を base に応じてシフト
  let map_greek = |ch: char, base: u32| -> char {
    match greek_math_offset(ch) {
      Some(offset) => char::from_u32(base + offset).unwrap_or(ch),
      None => ch,
    }
  };

  match style {
    None => {
      // デフォルト: ASCII 英字のみ Mathematical Italic、h 例外、Greek/digits は素通し
      return map_ascii(ch, 0x1d434, 0x1d44e, true);
    },
    Some(MathStyle::Serif) => return ch,
    Some(MathStyle::Italic) => {
      let mapped = map_ascii(ch, 0x1d434, 0x1d44e, true);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d6e2);
    },
    Some(MathStyle::Bold) => {
      let mapped = map_ascii(ch, 0x1d400, 0x1d41a, false);
      if mapped != ch {
        return mapped;
      }
      let mapped = map_digit(ch, 0x1d7ce);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d6a8);
    },
    Some(MathStyle::BoldItalic) => {
      let mapped = map_ascii(ch, 0x1d468, 0x1d482, false);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d71c);
    },
    Some(MathStyle::Sans) => {
      let mapped = map_ascii(ch, 0x1d5a0, 0x1d5ba, false);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7e2);
    },
    Some(MathStyle::SansItalic) => {
      return map_ascii(ch, 0x1d608, 0x1d622, false);
    },
    Some(MathStyle::SansBold) => {
      let mapped = map_ascii(ch, 0x1d5d4, 0x1d5ee, false);
      if mapped != ch {
        return mapped;
      }
      let mapped = map_digit(ch, 0x1d7ec);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d756);
    },
    Some(MathStyle::SansBoldItalic) => {
      let mapped = map_ascii(ch, 0x1d63c, 0x1d656, false);
      if mapped != ch {
        return mapped;
      }
      return map_greek(ch, 0x1d790);
    },
    Some(MathStyle::Mono) => {
      let mapped = map_ascii(ch, 0x1d670, 0x1d68a, false);
      if mapped != ch {
        return mapped;
      }
      return map_digit(ch, 0x1d7f6);
    },
  }
}

/// Greek 文字 1 文字の Mathematical Greek block 内オフセットを返す
///
/// 大文字 Α..Ρ → 0..16, ϴ → 17, Σ..Ω → 18..24, ∇ → 25。
/// 小文字 α..ρ → 26..42, ς → 43, σ..ω → 44..50。
/// variants は ∂ → 51, ϵ → 52, ϑ → 53, ϰ → 54, ϕ → 55, ϱ → 56, ϖ → 57。
///
/// Unicode Mathematical Greek block（Bold: U+1D6A8〜 / Italic: U+1D6E2〜 等）は
/// すべて同じ 58 要素のレイアウトを持つので、base codepoint にこのオフセットを
/// 加えるだけで該当スタイルのコードポイントを得られる。
fn greek_math_offset(ch: char) -> Option<u32> {
  return match ch {
    // 大文字 Α..Ρ
    '\u{0391}' => Some(0),  // Α
    '\u{0392}' => Some(1),  // Β
    '\u{0393}' => Some(2),  // Γ
    '\u{0394}' => Some(3),  // Δ
    '\u{0395}' => Some(4),  // Ε
    '\u{0396}' => Some(5),  // Ζ
    '\u{0397}' => Some(6),  // Η
    '\u{0398}' => Some(7),  // Θ
    '\u{0399}' => Some(8),  // Ι
    '\u{039A}' => Some(9),  // Κ
    '\u{039B}' => Some(10), // Λ
    '\u{039C}' => Some(11), // Μ
    '\u{039D}' => Some(12), // Ν
    '\u{039E}' => Some(13), // Ξ
    '\u{039F}' => Some(14), // Ο
    '\u{03A0}' => Some(15), // Π
    '\u{03A1}' => Some(16), // Ρ
    // ϴ GREEK CAPITAL THETA SYMBOL
    '\u{03F4}' => Some(17),
    // Σ..Ω
    '\u{03A3}' => Some(18), // Σ
    '\u{03A4}' => Some(19), // Τ
    '\u{03A5}' => Some(20), // Υ
    '\u{03A6}' => Some(21), // Φ
    '\u{03A7}' => Some(22), // Χ
    '\u{03A8}' => Some(23), // Ψ
    '\u{03A9}' => Some(24), // Ω
    // ∇ NABLA
    '\u{2207}' => Some(25),
    // 小文字 α..ρ
    '\u{03B1}' => Some(26), // α
    '\u{03B2}' => Some(27), // β
    '\u{03B3}' => Some(28), // γ
    '\u{03B4}' => Some(29), // δ
    '\u{03B5}' => Some(30), // ε
    '\u{03B6}' => Some(31), // ζ
    '\u{03B7}' => Some(32), // η
    '\u{03B8}' => Some(33), // θ
    '\u{03B9}' => Some(34), // ι
    '\u{03BA}' => Some(35), // κ
    '\u{03BB}' => Some(36), // λ
    '\u{03BC}' => Some(37), // μ
    '\u{03BD}' => Some(38), // ν
    '\u{03BE}' => Some(39), // ξ
    '\u{03BF}' => Some(40), // ο
    '\u{03C0}' => Some(41), // π
    '\u{03C1}' => Some(42), // ρ
    // ς final sigma
    '\u{03C2}' => Some(43),
    // σ..ω
    '\u{03C3}' => Some(44), // σ
    '\u{03C4}' => Some(45), // τ
    '\u{03C5}' => Some(46), // υ
    '\u{03C6}' => Some(47), // φ
    '\u{03C7}' => Some(48), // χ
    '\u{03C8}' => Some(49), // ψ
    '\u{03C9}' => Some(50), // ω
    // variants
    '\u{2202}' => Some(51), // ∂ PARTIAL DIFFERENTIAL
    '\u{03F5}' => Some(52), // ϵ GREEK LUNATE EPSILON SYMBOL
    '\u{03D1}' => Some(53), // ϑ GREEK THETA SYMBOL
    '\u{03F0}' => Some(54), // ϰ GREEK KAPPA SYMBOL
    '\u{03D5}' => Some(55), // ϕ GREEK PHI SYMBOL
    '\u{03F1}' => Some(56), // ϱ GREEK RHO SYMBOL
    '\u{03D6}' => Some(57), // ϖ GREEK PI SYMBOL
    _ => None,
  };
}

// =============================================================================
// ユーティリティ
// =============================================================================

/// インライン要素をプレーンテキストに変換する（一時的なヘルパー）
///
/// ## TODO
///
/// - [ ] 移行完了後にこの関数は不要になる（インライン要素は `lower_inline` で
///   個別にスタイル付きテキストに変換されるため）
/// - [ ] 見出しタイトルのインライン要素に対応するまでの間の暫定実装
fn inline_nodes_to_plain_text(inlines: &[InlineNode]) -> Result<String, LoweringError> {
  let mut text = String::new();
  for inline in inlines {
    match inline {
      InlineNode::Text(s) => text.push_str(s),
      InlineNode::Emphasis(children)
      | InlineNode::Strong(children)
      | InlineNode::Code(children)
      | InlineNode::SansSerif(children) => {
        text.push_str(&inline_nodes_to_plain_text(children)?);
      },
      InlineNode::InlineMath(_) => text.push_str("[Math]"),
      InlineNode::Symbol(ch) => text.push(*ch),
      InlineNode::LineBreak => text.push('\n'),
      InlineNode::Ref { label, number } => {
        let Some(s) = number else {
          return Err(LoweringError::UnresolvedReference {
            label: label.clone(),
          });
        };
        text.push_str(s);
      },
    }
  }
  return Ok(text);
}

#[cfg(test)]
mod tests {
  use super::*;

  /// ## TODO
  ///
  /// - [ ] 各変換関数のユニットテストを追加する
  /// - [ ] 既存の headline.rs のテストケースを移植してリグレッションテストとする
  /// - [ ] 段落変換のテスト（単純テキスト、複数インライン要素の混在）
  /// - [ ] リスト変換のテスト（順序付き/なし、ネスト）
  /// - [ ] インライン要素のスタイル解決テスト（Emphasis → Italic 等）

  #[test]
  fn test_lower_space() {
    // Arrange
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::Space(5.0);

    // Act
    let result = lower_node(&ctx, &node).expect("Space の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Kern { point } => assert!((point - 5.0).abs() < f32::EPSILON),
      other => panic!("Expected Kern, got {other:?}"),
    }
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
      width: 100.0,
      height: 1.0,
    };

    // Act
    let result = lower_node(&ctx, &node).expect("Rule の lowering は失敗しないはず");

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Rule { width, height } => {
        assert!((width - 100.0).abs() < f32::EPSILON);
        assert!((height - 1.0).abs() < f32::EPSILON);
      },
      other => panic!("Expected Rule, got {other:?}"),
    }
  }

  #[test]
  fn test_format_heading_text_section_default_template() {
    // 英語デフォルト: section は "{number} {title}"
    let number = HeadingNumber { parts: vec![2, 3] };

    let formatted = format_heading_text(&number, "Intro", "{number} {title}");

    assert_eq!(formatted, "2.3 Intro");
  }

  #[test]
  fn test_format_heading_text_part_default_template() {
    // 英語デフォルト: part は "Part {number}: {title}"
    let number = HeadingNumber { parts: vec![1] };

    let formatted = format_heading_text(&number, "Foundations", "Part {number}: {title}");

    assert_eq!(formatted, "Part 1: Foundations");
  }

  #[test]
  fn test_format_heading_text_japanese_override() {
    // 日本語化（style.toml 上書き例）が正しく置換されること
    let number = HeadingNumber { parts: vec![3] };

    let formatted = format_heading_text(&number, "序論", "第{number}章 {title}");

    assert_eq!(formatted, "第3章 序論");
  }

  #[test]
  fn test_format_heading_text_legacy_placeholders_are_literal() {
    // 旧プレースホルダ（\partnum / \chapternum / \text）はもはやプレースホルダではなく、
    // テンプレ内にあればそのままリテラルとして残ることを確認する。
    let number = HeadingNumber { parts: vec![1] };

    let formatted = format_heading_text(&number, "Title", "第\\partnum部 \\text");

    assert_eq!(formatted, "第\\partnum部 \\text");
  }

  #[test]
  fn lower_heading_uses_style_template() {
    // style.toml でテンプレを差し替えると見出し出力が追従することを確認する
    let mut style = ReadStyle::default();
    style.section.format = "[{number}] {title}".to_string();
    let ctx = LoweringContext::new(&style);

    let nodes = lower_heading(
      &ctx,
      HeadingLevel::Section,
      &HeadingNumber { parts: vec![4, 7] },
      &[InlineNode::Text("Custom Title".to_string())],
    )
    .expect("解決済みテキストのみの見出しは失敗しないはず");

    let vbox = nodes.iter().find_map(|n| {
      if let LayoutNode::VBox { children, .. } = n {
        Some(children)
      } else {
        None
      }
    });
    let children = vbox.expect("VBox が出力されるはず");
    let text = match &children[0] {
      LayoutNode::Text(text, _) => text.clone(),
      other => panic!("Text ノードが期待されます: {other:?}"),
    };
    assert_eq!(text, "[4.7] Custom Title");
  }

  #[test]
  fn test_inline_text_to_plain() {
    // Arrange
    let inlines = vec![
      InlineNode::Text("Hello ".to_string()),
      InlineNode::Strong(vec![InlineNode::Text("world".to_string())]),
    ];

    // Act
    let result = inline_nodes_to_plain_text(&inlines).expect("解決済みノードのみなので失敗しないはず");

    // Assert
    assert_eq!(result, "Hello world");
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
    }]);

    let err = lower_node(&ctx, &node).expect_err("未解決の Ref は LoweringError を返すべき");

    match err {
      LoweringError::UnresolvedReference { label } => assert_eq!(label, "ch:intro"),
    }
  }

  // ==========================================================
  // 数式（inline math / display math）の lowering テスト
  // ==========================================================

  #[test]
  fn lower_math_text_italicizes_ascii_letters_by_default() {
    // Arrange: デフォルトでは ASCII 英字 "x" のみ Mathematical Italic (U+1D44E) に変換、
    // 記号 "+" と数字 "1" は素通し。1 つの Math セグメントにまとまる
    let nodes = lower_math_text("x+1", 12.0, None);

    // Assert
    assert_eq!(nodes.len(), 1, "Math フォントの単一セグメントにまとまるはず: {nodes:?}");
    match &nodes[0] {
      LayoutNode::Text(t, style) => {
        assert_eq!(t, "\u{1D465}+1"); // U+1D44E + 23 (x - a)
        assert_eq!(style.font_kind, FontKind::Math);
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_text_keeps_japanese_in_math_kind() {
    // Arrange: 数式中に和文が混ざっても lowering 層ではスクリプト分割せず単一の Math セグメントで返す
    // x は italic 化される、和文と数字は素通し
    let nodes = lower_math_text("x速度2", 12.0, None);

    // Assert
    assert_eq!(nodes.len(), 1, "Math フォントの単一セグメントになるはず: {nodes:?}");
    match &nodes[0] {
      LayoutNode::Text(t, style) => {
        assert_eq!(t, "\u{1D465}速度2"); // U+1D44E + 23 (x - a)
        assert_eq!(style.font_kind, FontKind::Math);
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_text_empty_returns_no_nodes() {
    // 空文字列は空のノード列を返す
    let nodes = lower_math_text("", 12.0, None);
    assert!(nodes.is_empty(), "空文字列は空のノード列を返すはず: {nodes:?}");
  }

  #[test]
  fn lower_math_superscript_wraps_in_raise() {
    // Arrange: x^2 の Superscript は Raise（正の dy）でラップされ、フォントサイズが縮小される
    let node = MathNode::Superscript(Box::new(MathNode::Text("2".to_string())));

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Raise { dy, children } => {
        assert!(*dy > 0.0, "上付きは正の dy（上方向）になるべき: dy={dy}");
        // children に縮小サイズの Text が入っているはず
        assert!(!children.is_empty());
        if let LayoutNode::Text(_, style) = &children[0] {
          assert!(style.font_size < 12.0, "上付きはフォントサイズが縮小される: size={}", style.font_size);
        } else {
          panic!("Text を期待: {:?}", children[0]);
        }
      },
      other => panic!("Raise を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_subscript_uses_negative_raise() {
    // Arrange: x_i の Subscript は Raise（負の dy）でラップされる
    let node = MathNode::Subscript(Box::new(MathNode::Text("i".to_string())));

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Raise { dy, .. } => {
        assert!(*dy < 0.0, "下付きは負の dy（下方向）になるべき: dy={dy}");
      },
      other => panic!("Raise を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_symbol_uses_math_font() {
    // Arrange: \alpha → MathNode::Symbol('α') → デフォルトでは α は素通し（Math フォントで描画）
    let node = MathNode::Symbol('α');

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert
    assert_eq!(result.len(), 1);
    match &result[0] {
      LayoutNode::Text(t, style) => {
        assert_eq!(t, "α");
        assert_eq!(style.font_kind, FontKind::Math);
      },
      other => panic!("Math Text を期待: {other:?}"),
    }
  }

  #[test]
  fn lower_math_frac_inlines_as_slash() {
    // Arrange: インライン \frac{a}{b} は "a / b" 形式で代替される（真の縦書きは未対応）
    let node = MathNode::Frac {
      numer: Box::new(MathNode::Text("a".to_string())),
      denom: Box::new(MathNode::Text("b".to_string())),
    };

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert: 何らかの Text に "/" が含まれていること
    let has_slash = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "/"));
    assert!(has_slash, "分数は / 付きで描画されるはず: {result:?}");
  }

  #[test]
  fn lower_math_sqrt_emits_radical_sign() {
    // Arrange: \sqrt{x} は √ 記号 + 被根号 でレンダリング
    let node = MathNode::Sqrt {
      index: None,
      radicand: Box::new(MathNode::Text("x".to_string())),
    };

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert: √ を含む Text ノードが存在し、後ろに変数 x の italic Text が続く
    let has_radical = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "√"));
    assert!(has_radical, "√ 記号が含まれるはず: {result:?}");
  }

  #[test]
  fn lower_math_alignment_mark_is_ignored_inline() {
    // インライン数式に紛れ込んだ AlignmentMark は空のノード列を返す
    let result = lower_math_node(&MathNode::AlignmentMark, 12.0, None);
    assert!(result.is_empty(), "AlignmentMark は無視されるべき: {result:?}");
  }

  // ==========================================================
  // translate_math_char のテスト
  // ==========================================================

  #[test]
  fn translate_default_italicizes_ascii_letters_only() {
    // Arrange & Act & Assert — デフォルト: ASCII 英字のみ italic 化、他は素通し
    assert_eq!(translate_math_char('a', None), '\u{1D44E}'); // mathematical italic a
    assert_eq!(translate_math_char('z', None), '\u{1D467}'); // mathematical italic z
    assert_eq!(translate_math_char('A', None), '\u{1D434}'); // mathematical italic A
    assert_eq!(translate_math_char('Z', None), '\u{1D44D}'); // mathematical italic Z
    assert_eq!(translate_math_char('h', None), '\u{210E}'); // PLANCK CONSTANT (h 例外)
    assert_eq!(translate_math_char('1', None), '1', "デフォルトでは数字は素通し");
    assert_eq!(translate_math_char('+', None), '+', "記号は素通し");
    assert_eq!(translate_math_char('α', None), 'α', "Greek もデフォルトは素通し");
  }

  #[test]
  fn translate_bold_covers_letters_digits_greek() {
    // Arrange & Act & Assert
    use parser::document::MathStyle;
    let style = Some(MathStyle::Bold);
    assert_eq!(translate_math_char('x', style), '\u{1D431}'); // mathematical bold x
    assert_eq!(translate_math_char('A', style), '\u{1D400}'); // mathematical bold A
    assert_eq!(translate_math_char('0', style), '\u{1D7CE}'); // mathematical bold digit 0
    assert_eq!(translate_math_char('9', style), '\u{1D7D7}'); // mathematical bold digit 9
    assert_eq!(translate_math_char('α', style), '\u{1D6C2}'); // mathematical bold alpha
    assert_eq!(translate_math_char('Ω', style), '\u{1D6C0}'); // mathematical bold Omega
    assert_eq!(translate_math_char('+', style), '+', "記号は素通し");
  }

  #[test]
  fn translate_italic_h_uses_planck_constant_exception() {
    // Arrange & Act & Assert — italic 系のみ h の例外
    use parser::document::MathStyle;
    assert_eq!(translate_math_char('h', Some(MathStyle::Italic)), '\u{210E}');
    assert_eq!(
      translate_math_char('h', Some(MathStyle::BoldItalic)),
      '\u{1D489}',
      "BoldItalic h は通常コードポイント"
    );
  }

  #[test]
  fn translate_mono_keeps_greek_passthrough() {
    // Arrange & Act & Assert — Mono は Greek 未対応 → 素通し
    use parser::document::MathStyle;
    assert_eq!(translate_math_char('α', Some(MathStyle::Mono)), 'α');
    assert_eq!(translate_math_char('a', Some(MathStyle::Mono)), '\u{1D68A}'); // mono a
    assert_eq!(translate_math_char('5', Some(MathStyle::Mono)), '\u{1D7FB}'); // mono digit 5
  }

  #[test]
  fn translate_serif_is_passthrough() {
    // Arrange & Act & Assert — Serif (Roman) は全部素通し
    use parser::document::MathStyle;
    let style = Some(MathStyle::Serif);
    assert_eq!(translate_math_char('x', style), 'x');
    assert_eq!(translate_math_char('1', style), '1');
    assert_eq!(translate_math_char('α', style), 'α');
  }

  #[test]
  fn translate_sans_skips_greek() {
    // Arrange & Act & Assert — Sans は Greek 未対応 (Unicode に該当コードポイントなし)
    use parser::document::MathStyle;
    let style = Some(MathStyle::Sans);
    assert_eq!(translate_math_char('a', style), '\u{1D5BA}'); // sans a
    assert_eq!(translate_math_char('1', style), '\u{1D7E3}'); // sans digit 1
    assert_eq!(translate_math_char('α', style), 'α', "Sans Greek は Unicode 未定義のため素通し");
  }

  #[test]
  fn lower_math_node_bold_styled_propagates_to_text_and_symbol() {
    // Arrange — \mathbold{x12 \alpha} 相当: Styled { Bold, [Text("x12"), Symbol('α')] }
    use parser::document::MathStyle;
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Text("x12".to_string()), MathNode::Symbol('α')],
    };

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert — 連結すると "𝐱𝟏𝟐" + "𝛂"
    let texts: String = result
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(t, _) => Some(t.as_str()),
        _ => None,
      })
      .collect();
    assert_eq!(texts, "\u{1D431}\u{1D7CF}\u{1D7D0}\u{1D6C2}");
  }

  #[test]
  fn lower_math_node_styled_propagates_into_frac_body() {
    // Arrange — \mathbold{\frac{a}{b}}: 内側 frac の a と b は bold で変換される
    use parser::document::MathStyle;
    let node = MathNode::Styled {
      style: MathStyle::Bold,
      body: vec![MathNode::Frac {
        numer: Box::new(MathNode::Text("a".to_string())),
        denom: Box::new(MathNode::Text("b".to_string())),
      }],
    };

    // Act
    let result = lower_math_node(&node, 12.0, None);

    // Assert — bold a (U+1D41A) と bold b (U+1D41B) が含まれる
    let has_bold_a = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "\u{1D41A}"));
    let has_bold_b = result.iter().any(|n| matches!(n, LayoutNode::Text(t, _) if t == "\u{1D41B}"));
    assert!(has_bold_a, "bold a が含まれるはず: {result:?}");
    assert!(has_bold_b, "bold b が含まれるはず: {result:?}");
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
  fn lower_display_math_wraps_with_linebreaks() {
    // ディスプレイ数式は前後に LineBreak を挿入して独立した行に配置する
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let node = DocNode::DisplayMath {
      body: vec![MathNode::Text("a".to_string())],
      label: None,
    };

    let nodes = lower_node(&ctx, &node).expect("display math lowering は失敗しないはず");

    assert!(matches!(nodes.first(), Some(LayoutNode::LineBreak)));
    assert!(matches!(nodes.last(), Some(LayoutNode::LineBreak)));
  }

  #[test]
  fn unresolved_ref_in_heading_title_returns_error() {
    // 見出しタイトルに含まれる未解決 Ref も inline_nodes_to_plain_text 経由で
    // 同じエラーとして伝播することを確認する。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);

    let err = lower_heading(
      &ctx,
      HeadingLevel::Section,
      &HeadingNumber { parts: vec![1] },
      &[InlineNode::Ref {
        label: "sec:missing".to_string(),
        number: None,
      }],
    )
    .expect_err("見出しタイトルの未解決 Ref は LoweringError を返すべき");

    match err {
      LoweringError::UnresolvedReference { label } => assert_eq!(label, "sec:missing"),
    }
  }
}
