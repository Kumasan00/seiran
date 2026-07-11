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
use types::Length;

mod counter;
mod figure;
mod float;
mod heading;
mod inline;
mod layout_node;
mod list;
mod math;
mod paragraph;
mod placeholder;
mod quote;
mod resolve;
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
  /// `\ref{label}` / `proof` の `[of=...]` が参照するラベルが未定義の場合に返されます。
  ///
  /// `lowering` 層の pass2（[`resolve::resolve_refs`]）が `CounterRegistry` に対応するラベルを
  /// 見つけられなかったことを示す、`\ref` 解決の主たるエラー経路です。
  #[error("未解決の参照が lowering に到達しました: ラベル `{label}`")]
  #[diagnostic(code(lowering::unresolved_reference), help("対応する \\label が定義されているか確認してください。"))]
  UnresolvedReference {
    /// 解決できなかったラベル名（`\ref{ch:intro}` の `ch:intro`）
    label: String,
    /// `\ref{...}` のソース位置（`InlineNode::Ref` から引き継ぐ）
    #[label("この参照が未解決です")]
    span: miette::SourceSpan,
  },

  /// `\section[label=...]` / 環境 `[label=...]` で同名ラベルが重複登録された場合に返されます。
  #[error("ラベルが重複しています: {label}")]
  #[diagnostic(code(lowering::duplicate_label), help("label=... の値はドキュメント全体で一意にしてください"))]
  DuplicateLabel {
    /// 重複したラベル名
    label: String,
    /// 2 回目に定義したコマンド / 環境のソース位置
    #[label("このラベルは既に定義されています")]
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
  /// ラスタ画像埋め込み時の最大 DPI（config `[image].max_dpi` 由来）
  ///
  /// `\image[dpi=...]` の per-image 上書きが無いときの既定値。production では
  /// [`LoweringContext::with_image_defaults`] で config の値に差し替える（出力物理の設定は
  /// style ではなく config が持つ）。
  pub image_max_dpi: u32,
  /// ラスタ画像のダウンサンプリング可否（config `[image].downsample` 由来）
  ///
  /// `\image[downsample=...]` の per-image 上書きが無いときの既定値。
  pub image_downsample: bool,
  /// 箇条書き（`itemize` / `enumerate`）のネスト深さ（0 = 最上位）
  ///
  /// [`crate::list::lower_list`] がマーカーの見た目を段ごとに切り替えるために参照する。
  /// リスト項目の内容を lower する際に [`LoweringContext::with_list_depth`] で +1 した文脈を
  /// 渡すことで、item 内容中のネストしたリストが深さ +1 として lower される。
  pub list_depth: usize,
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
      // 画像 DPI の既定。production は build_pdf が with_image_defaults で config 由来値へ差し替える
      // （read_config::ImageConfig::default と同値）。テストはこの既定をそのまま使う。
      image_max_dpi: 300,
      image_downsample: true,
      // 文書のトップレベルは深さ 0。リストに入るたび with_list_depth で +1 する。
      list_depth: 0,
    };
  }

  /// 画像出力の既定値（config `[image]` 由来）を差し替えた文脈を返す
  ///
  /// 画像のダウンサンプリング解像度は「出力物理」の設定で config.toml `[image]` が持つ。
  /// `build_pdf` が `LoweringContext::new(&style).with_image_defaults(...)` の形で注入する。
  #[must_use]
  pub fn with_image_defaults(mut self, image_max_dpi: u32, image_downsample: bool) -> Self {
    self.image_max_dpi = image_max_dpi;
    self.image_downsample = image_downsample;
    return self;
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
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth: self.list_depth,
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
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth: self.list_depth,
    };
  }

  /// 箇条書きのネスト深さだけを差し替えた派生文脈を返す
  ///
  /// `style` 参照・その他フィールドは共有したまま `list_depth` のみ上書きする。
  /// [`crate::list::lower_list`] がリスト項目の内容を lower する際に深さ +1 の文脈を渡し、
  /// item 内容中のネストしたリストへ深さを伝播させるのに使う。
  #[must_use]
  pub fn with_list_depth(&self, list_depth: usize) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind: self.body_font_kind,
      first_line_indent: self.first_line_indent,
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth,
    };
  }

  /// 既定フォントサイズ（段落本文用、`style.text.font_size` に等しい）を pt 値で返すヘルパー
  #[must_use]
  pub fn default_font_size(&self) -> Length { return self.style.text.font_size; }
}

/// 見出し 1 件の記録（PDF しおり・目次生成が消費する）
///
/// [`lower_nodes_with_headings`] が pass1 の走査と同時に収集する。`number` は
/// `CounterRegistry` が発番した書式化済み文字列（無採番の見出しは空文字列）、`title_plain` は
/// タイトル中の `\ref` も解決済みのプレーンテキスト。
#[derive(Debug, Clone, PartialEq)]
pub struct HeadingRecord {
  /// 見出しの文書順インデックス（0 始まり）
  pub index: usize,
  /// 見出しレベル
  pub level: types::HeadingLevel,
  /// 書式化済みの見出し番号（無採番の見出しは空文字列）
  pub number: String,
  /// 見出しタイトルのプレーンテキスト（`\ref` 解決済み）
  pub title_plain: String,
}

/// pass1 走査中に集める見出しの生データ（`title` はまだ `\ref` 未解決）
struct PendingHeading {
  index: usize,
  level: types::HeadingLevel,
  number: String,
  title: Vec<document::InlineNode>,
}

/// Document IR をレイアウトノードに変換する（ドキュメント全体）
///
/// # Errors
///
/// 内部で呼び出す [`lower_nodes`] が返すエラーをそのまま伝播します。
pub fn lower_document(ctx: &LoweringContext, document: &Document) -> Result<Vec<LayoutNode>, LoweringError> {
  return lower_nodes(ctx, &document.body);
}

/// `DocNode` のリストをレイアウトノードに変換する
///
/// `build_pdf.rs` から呼ばれる lowering のエントリーポイント。見出し記録が不要な呼び出し向けの
/// 薄いラッパーで、[`lower_nodes_with_headings`] の結果から `Vec<LayoutNode>` だけを返す。
///
/// # Errors
///
/// いずれかの `DocNode` の変換中に [`LoweringError`] が発生した場合に返します。
pub fn lower_nodes(ctx: &LoweringContext, nodes: &[DocNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let (layout, _headings) = lower_nodes_with_headings(ctx, nodes)?;
  return Ok(layout);
}

/// `DocNode` のリストをレイアウトノードに変換し、見出し記録（PDF しおり・目次生成用）も返す
///
/// 採番（[`counter::CounterRegistry`]）は 1 回の呼び出しにつき 1 つのレジストリを共有し、
/// 全ノードを通して連続して発番される。`\ref` は前方参照を許すため、pass1（本走査）完了後に
/// pass2（[`resolve::resolve_refs`]）で `LayoutNode::Ref` プレースホルダを解決する。
///
/// # Errors
///
/// いずれかの `DocNode` の変換中、または pass2 の参照解決で [`LoweringError`] が発生した場合に返します。
pub fn lower_nodes_with_headings(
  ctx: &LoweringContext,
  nodes: &[DocNode],
) -> Result<(Vec<LayoutNode>, Vec<HeadingRecord>), LoweringError> {
  let mut registry = counter::CounterRegistry::from_style(ctx.style);
  let mut pending_headings = Vec::new();
  let mut result = lower_nodes_inner(ctx, nodes, &mut registry, &mut pending_headings)?;

  let headings = pending_headings
    .into_iter()
    .map(|pending| HeadingRecord {
      index: pending.index,
      level: pending.level,
      number: pending.number,
      title_plain: resolve_heading_title_plain(&pending.title, &registry),
    })
    .collect();

  resolve::resolve_refs(&mut result, &registry)?;

  debug!(input_node_count = nodes.len(), layout_node_count = result.len(), "lowering が完了しました");
  return Ok((result, headings));
}

/// 単一の `DocNode` をレイアウトノードに変換する（見出しインデックス 0 固定の単体エントリ）
///
/// 見出しの暗黙キーが文書順に依存しないテスト・単発変換用。本文全体の変換は
/// [`lower_nodes`] が [`lower_node_indexed`] を介して連番キーを与える。pass2（`\ref` 解決）は
/// 実行しないため、`InlineNode::Ref` を含むノードは `LayoutNode::Ref` プレースホルダのまま返る。
#[cfg(test)]
fn lower_node(ctx: &LoweringContext, node: &DocNode) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut registry = counter::CounterRegistry::from_style(ctx.style);
  let mut headings = Vec::new();
  return lower_node_indexed(ctx, node, 0, &mut registry, &mut headings);
}

/// `nodes` を順に [`lower_node_indexed`] へ渡す内部ウォーク（pass1 本体）
///
/// `registry` / `headings` は同一呼び出し内の再帰（`theorem` / `quote` / `list` の本体）を通して
/// 共有する。見出しの文書順インデックス（`heading_index`）だけは再帰呼び出しごとにリセットする
/// （見出しは環境本体に現れないため実質的に影響しない。既存の暗黙アンカーキー採番規則を保つための
/// 挙動）。
fn lower_nodes_inner(
  ctx: &LoweringContext,
  nodes: &[DocNode],
  registry: &mut counter::CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut result = Vec::new();
  // 見出しの文書順インデックス。`heading_anchor_key` で暗黙 destination キーを採番するのに使う
  // （目次エントリの内部リンク到達先と一致させるため、`HeadingRecord::index` と同じ規則）。
  let mut heading_index = 0;
  for node in nodes {
    result.extend(lower_node_indexed(ctx, node, heading_index, registry, headings)?);
    if node.is_heading() {
      heading_index += 1;
    }
  }
  return Ok(result);
}

/// 単一の `DocNode` をレイアウトノードに変換する
///
/// `heading_index` は見出しの文書順インデックス（[`document::heading_anchor_key`] に渡す）。
fn lower_node_indexed(
  ctx: &LoweringContext,
  node: &DocNode,
  heading_index: usize,
  registry: &mut counter::CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  match node {
    DocNode::Heading {
      level,
      numbered,
      title,
      label,
      span,
    } => {
      let number = if *numbered {
        registry.increment_with_label(
          counter::CounterRegistry::counter_name_for_heading(*level),
          label.as_deref(),
          *span,
        )?
      } else {
        String::new()
      };
      headings.push(PendingHeading {
        index: heading_index,
        level: *level,
        number: number.clone(),
        title: title.clone(),
      });
      return heading::lower_heading(ctx, *level, &number, title, label.clone(), heading_index);
    },
    DocNode::Paragraph(inlines) => {
      return paragraph::lower_paragraph(ctx, inlines);
    },
    DocNode::List { ordered, items } => {
      return list::lower_list(ctx, *ordered, items, registry, headings);
    },
    DocNode::Theorem {
      class,
      title,
      body,
      of,
      label,
      span,
    } => {
      // 見出し（独立行）＋ クラス別 font_kind 本文 ＋ 上下マージン、proof は末尾に QED。
      // `of`（証明対象、proof のみ）は前方参照になり得るため、`{of}` プレースホルダとして
      // 見出しテンプレートに埋め込み、pass2（`resolve::resolve_refs`）で解決する。
      let of = of.as_ref().map(|target| (target.label.as_str(), target.span));
      let number = registry.increment_theorem_with_label(*class, label.as_deref(), *span)?;
      return theorem::lower_theorem(
        ctx,
        *class,
        number.as_deref(),
        title.as_deref(),
        body,
        of,
        label.as_deref(),
        registry,
        headings,
      );
    },
    DocNode::Quote { kind, body } => {
      return quote::lower_quote(ctx, *kind, body, registry, headings);
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
      numbered,
      label,
      span,
    } => {
      let block = &ctx.style.math.block;
      let math_block = math::lower_math_block(ctx, *kind, rows, *numbered, label.as_deref(), *span, registry)?;
      let mut nodes = vec![
        LayoutNode::Vkern {
          length: block.top_margin,
        },
        math_block,
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
      label,
      span,
    } => {
      let caption_arg = caption.as_deref().map(|inlines| (*caption_position, inlines));
      let overrides = figure::ImageOverrides {
        dpi: *dpi,
        downsample: *downsample,
      };
      let number = registry.increment_with_label(read_style::CounterName::Figure, label.as_deref(), *span)?;
      let nodes = figure::lower_figure(ctx, image_path, *width, *height, overrides, caption_arg, &number)?;
      return Ok(with_label_anchor(label.as_deref(), nodes));
    },
    DocNode::Table {
      columns,
      widths,
      head,
      rows,
      caption,
      caption_position,
      breakable,
      label,
      span,
    } => {
      let caption_arg = caption.as_deref().map(|inlines| (*caption_position, inlines));
      let number = registry.increment_with_label(read_style::CounterName::Table, label.as_deref(), *span)?;
      let nodes = table::lower_table(ctx, columns, widths, head, rows, caption_arg, &number, *breakable)?;
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

/// 見出しタイトルのプレーンテキストを、`\ref` を `CounterRegistry` で解決しながら組み立てる
///
/// [`document::InlineNode::to_plain_text`] と同じ規則だが、`Ref` は `registry.resolve_label` で
/// 解決した文字列を使う（`InlineNode` 自体はもう解決済み番号を保持しないため）。
/// pass1 完了後（`registry` の labels テーブルが確定した後）にのみ呼ぶこと。
fn resolve_heading_title_plain(title: &[document::InlineNode], registry: &counter::CounterRegistry) -> String {
  return title.iter().map(|inline| resolve_inline_plain(inline, registry)).collect();
}

/// 単一の `InlineNode` を、`Ref` は `registry` 解決込みでプレーンテキストへ変換する
fn resolve_inline_plain(inline: &document::InlineNode, registry: &counter::CounterRegistry) -> String {
  use document::InlineNode;
  return match inline {
    InlineNode::Text(s) => s.clone(),
    InlineNode::Styled { children, .. } | InlineNode::Colored { children, .. } => {
      children.iter().map(|c| resolve_inline_plain(c, registry)).collect()
    },
    InlineNode::InlineMath(_) => "[Math]".to_string(),
    InlineNode::Symbol(ch) => ch.to_string(),
    InlineNode::LineBreak => "\n".to_string(),
    InlineNode::NoIndent => String::new(),
    InlineNode::Ref { label, .. } => registry.resolve_label(label).unwrap_or_default().to_string(),
    InlineNode::Link { children, .. } | InlineNode::InternalLink { children, .. } => {
      children.iter().map(|c| resolve_inline_plain(c, registry)).collect()
    },
    InlineNode::Cite { keys, label, .. } => label
      .as_deref()
      .map_or_else(|| keys.join(", "), |inlines| inlines.iter().map(|c| resolve_inline_plain(c, registry)).collect()),
  };
}

#[cfg(test)]
mod tests {
  use document::{HeadingLevel, InlineNode, ListItem, MathEnvKind, MathNode, MathRow};
  use types::Length;

  use super::*;

  /// 1 行 1 セルの `equation` 相当 `DocNode::MathBlock` を作るテストヘルパ
  fn equation_block(numbered: bool, label: Option<&str>) -> DocNode {
    return DocNode::MathBlock {
      kind: MathEnvKind::Equation,
      rows: vec![MathRow {
        cells: vec![vec![MathNode::Text("a".to_string())]],
        numbered,
        label: label.map(str::to_string),
        label_span: None,
      }],
      numbered: false,
      // equation はラベルを行側（`MathRow::label`）に持つため、環境単位ラベルは None
      label: None,
      span: miette::SourceSpan::from((0_usize, 0_usize)),
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
    // 未定義ラベルへの \ref は pass2（lower_nodes 内の resolve::resolve_refs）で
    // LoweringError::UnresolvedReference になることを確認する。
    // lower_node（単体テスト用エントリ）は pass2 を実行しないため、ここでは
    // pass2 も走る公開 API の lower_nodes を使う。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "ch:intro".to_string(),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }])];

    let err = lower_nodes(&ctx, &nodes).expect_err("未解決の Ref は LoweringError を返すべき");

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
    let node = equation_block(false, None);

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
    // numbered: true のとき MathBlock の行に番号ボックスが乗る（実際の発番は lowering が行う）
    let mut style = ReadStyle::default();
    style.counters.equation.number_format = "{n}".to_string();
    let ctx = LoweringContext::new(&style);
    let node = equation_block(true, None);

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
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("H".to_string())]),
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
      DocNode::heading(HeadingLevel::Chapter, vec![InlineNode::Text("Intro".to_string())]),
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
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("Heading".to_string())]),
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
    let node = equation_block(true, Some("eq:foo"));

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
    let node = equation_block(true, None);

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
    assert_eq!(ctx.default_font_size(), Length::pt(18.0));
    let LayoutNode::Text(_, text_style) = &out[0] else {
      panic!("先頭は Text であるべき: {out:?}");
    };
    assert_eq!(text_style.font_size, Length::pt(18.0));
  }
}
