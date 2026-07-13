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

use config::read_style::Style as ReadStyle;
use document::{DocNode, Document, try_inline_nodes_to_plain_text};
use miette::Diagnostic;
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

/// 複数ソースファイルを 1 回の lowering 呼び出しでまとめて処理する際の、ソースの位置識別子
///
/// lowering はソース名・内容を知らない。`seiran`（呼び出し元）が渡した `sources` スライスの
/// インデックスをそのまま識別子として運び、エラー発生時に `seiran` 側でファイル名・内容へ
/// 逆引きする（[`lower_sources_with_headings`] の `sources` 引数の並びと対応する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceId(usize);

impl SourceId {
  /// 新しい `SourceId` を生成する
  #[must_use]
  pub fn new(index: usize) -> Self { return SourceId(index); }

  /// 元のインデックスを返す
  #[must_use]
  pub fn index(self) -> usize { return self.0; }
}

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
    /// この参照が属するソースの識別子（`seiran` 側で `NamedSource` へ逆引きする）
    ///
    /// `thiserror` は `source` という名のフィールドを誤ってエラー源（`#[source]`）と解釈するため、
    /// エラー源ではないこの識別子は `source_id` と命名して衝突を避ける。
    source_id: SourceId,
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
    /// この重複定義が属するソースの識別子（`seiran` 側で `NamedSource` へ逆引きする）
    ///
    /// `source` はエラー源として `thiserror` に予約されているため `source_id` と命名する。
    source_id: SourceId,
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
    /// この引用が属するソースの識別子（`seiran` 側で `NamedSource` へ逆引きする）
    ///
    /// `source` はエラー源として `thiserror` に予約されているため `source_id` と命名する。
    source_id: SourceId,
  },
}

impl LoweringError {
  /// このエラーが帰属するソースの識別子を返す
  ///
  /// `seiran`（呼び出し元）はこの値を [`SourceId::index`] で `sources` スライスの位置に戻し、
  /// 対応するファイル名・内容を `NamedSource` に紐付けて診断を表示する。範囲外（合成された
  /// 書誌グループ）を指す場合はフォールバック診断に切り替える。
  #[must_use]
  pub fn source_id(&self) -> SourceId {
    return match self {
      LoweringError::UnresolvedReference { source_id, .. }
      | LoweringError::DuplicateLabel { source_id, .. }
      | LoweringError::UnresolvedCitation { source_id, .. } => *source_id,
    };
  }
}

/// Lowering のコンテキスト
///
/// 変換中に必要なスタイル設定（`config::read_style::Style`）への参照を保持します。
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
  /// 現在 lower 中のソースグループの識別子
  ///
  /// [`lower_sources_with_headings`] がグループ `i` を lower する際に [`LoweringContext::with_source`]
  /// で `SourceId::new(i)` に差し替える。この文脈から発行される `LayoutNode::Ref` / `PendingHeading` /
  /// `LoweringError` に帰属ソースとして刻まれ、`seiran` 側でファイル名・内容へ逆引きされる。
  /// 単発変換（[`lower_nodes`] / [`lower_document`]）は常にグループ 0 として扱う。
  pub source: SourceId,
}

impl<'a> LoweringContext<'a> {
  /// 新しい `LoweringContext` を生成する
  ///
  /// # Arguments
  ///
  /// * `style` - `config::read_style::read_style()` の結果への参照
  #[must_use]
  pub fn new(style: &'a ReadStyle) -> Self {
    return LoweringContext {
      style,
      body_font_kind: style.text.font_kind,
      first_line_indent: style.text.first_line_indent,
      // 画像 DPI の既定。production は build_pdf が with_image_defaults で config 由来値へ差し替える
      // （config::read_config::ImageConfig::default と同値）。テストはこの既定をそのまま使う。
      image_max_dpi: 300,
      image_downsample: true,
      // 文書のトップレベルは深さ 0。リストに入るたび with_list_depth で +1 する。
      list_depth: 0,
      // 単発変換はグループ 0 固定。複数ソースは lower_sources_with_headings が with_source で差し替える。
      source: SourceId::new(0),
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
      source: self.source,
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
      source: self.source,
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
      source: self.source,
    };
  }

  /// ソース識別子だけを差し替えた派生文脈を返す
  ///
  /// [`lower_sources_with_headings`] がグループ `i` を lower する際に `SourceId::new(i)` を渡して使う。
  #[must_use]
  pub fn with_source(&self, source: SourceId) -> LoweringContext<'a> {
    return LoweringContext {
      style: self.style,
      body_font_kind: self.body_font_kind,
      first_line_indent: self.first_line_indent,
      image_max_dpi: self.image_max_dpi,
      image_downsample: self.image_downsample,
      list_depth: self.list_depth,
      source,
    };
  }

  /// 既定フォントサイズ（段落本文用、`style.text.font_size` に等しい）を pt 値で返すヘルパー
  #[must_use]
  pub fn default_font_size(&self) -> Length { return self.style.text.font_size; }
}

/// 見出し 1 件の記録（PDF しおり・目次生成が消費する）
///
/// [`lower_sources_with_headings`] が pass1 の走査と同時に収集する。`number` は
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
  /// 見出しが属するソースグループの識別子。未解決 `\ref` のエラー帰属に使う。
  source: SourceId,
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
/// 見出し記録が不要な単一ソースの呼び出し向けの薄いラッパー。1 要素スライスとして
/// [`lower_sources_with_headings`] に委譲し、その結果から `Vec<LayoutNode>` だけを返す。
///
/// # Errors
///
/// いずれかの `DocNode` の変換中に [`LoweringError`] が発生した場合に返します。
pub fn lower_nodes(ctx: &LoweringContext, nodes: &[DocNode]) -> Result<Vec<LayoutNode>, LoweringError> {
  let (layout, _headings) = lower_sources_with_headings(ctx, &[nodes])?;
  return Ok(layout);
}

/// 複数ソースグループを 1 回の lowering でまとめて変換し、見出し記録（PDF しおり・目次生成用）も返す
///
/// `sources` の各要素は 1 ソースファイル分の `&[DocNode]` で、その並び順が [`SourceId`] のインデックス
/// になる。採番（[`counter::CounterRegistry`]）と見出し収集（`pending_headings`）は**全グループで共有**し、
/// 文書全体を通して連続して発番・連番付けする。`\ref` は前方参照（別グループ含む）を許すため、
/// pass1（本走査）を全グループ完了させたあと pass2（[`resolve::resolve_refs`]）で `LayoutNode::Ref`
/// プレースホルダを解決する。グループ `i` を lower する間だけ [`LoweringContext::with_source`] で
/// `SourceId::new(i)` を刻んだ文脈を使い、そこから発行されるエラー・プレースホルダに帰属ソースが乗る。
///
/// # Errors
///
/// いずれかの `DocNode` の変換中、または pass2 の参照解決で [`LoweringError`] が発生した場合に返します。
pub fn lower_sources_with_headings(
  ctx: &LoweringContext,
  sources: &[&[DocNode]],
) -> Result<(Vec<LayoutNode>, Vec<HeadingRecord>), LoweringError> {
  let mut registry = counter::CounterRegistry::from_style(ctx.style);
  let mut pending_headings = Vec::new();
  let mut result = Vec::new();
  for (i, nodes) in sources.iter().enumerate() {
    let group_ctx = ctx.with_source(SourceId::new(i));
    result.extend(lower_nodes_inner(&group_ctx, nodes, &mut registry, &mut pending_headings)?);
  }

  let headings = pending_headings
    .into_iter()
    .map(|pending| {
      let title_plain = try_resolve_heading_title_plain(&pending.title, &registry, pending.source)?;
      return Ok(HeadingRecord {
        index: pending.index,
        level: pending.level,
        number: pending.number,
        title_plain,
      });
    })
    .collect::<Result<Vec<HeadingRecord>, LoweringError>>()?;

  resolve::resolve_refs(&mut result, &registry)?;

  let input_node_count: usize = sources.iter().map(|nodes| nodes.len()).sum();
  debug!(input_node_count, layout_node_count = result.len(), "lowering が完了しました");
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
  return lower_node_indexed(ctx, node, &mut registry, &mut headings);
}

/// `nodes` を順に [`lower_node_indexed`] へ渡す内部ウォーク（pass1 本体）
///
/// `registry` / `headings` は同一呼び出し内の再帰（`theorem` / `quote` / `list` の本体）を通して
/// 共有する。見出しの文書順インデックスは `lower_node_indexed` 側で `headings.len()`（push 前の
/// 長さ）から都度導出するため、再帰の深さに関わらず文書全体で単調増加し一意になる。
fn lower_nodes_inner(
  ctx: &LoweringContext,
  nodes: &[DocNode],
  registry: &mut counter::CounterRegistry,
  headings: &mut Vec<PendingHeading>,
) -> Result<Vec<LayoutNode>, LoweringError> {
  let mut result = Vec::new();
  for node in nodes {
    result.extend(lower_node_indexed(ctx, node, registry, headings)?);
  }
  return Ok(result);
}

/// 単一の `DocNode` をレイアウトノードに変換する
fn lower_node_indexed(
  ctx: &LoweringContext,
  node: &DocNode,
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
          ctx.source,
        )?
      } else {
        String::new()
      };
      // 見出しの文書順インデックス。`headings`（呼び出しツリー全体で共有された Vec）の
      // push 前の長さを使うことで、再帰（quote / theorem / list item 本体）を挟んでも
      // 単調増加する一意な値になる（`heading_anchor_key` の暗黙 destination キー採番に使う）。
      let heading_index = headings.len();
      headings.push(PendingHeading {
        index: heading_index,
        level: *level,
        number: number.clone(),
        title: title.clone(),
        source: ctx.source,
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
      let number = registry.increment_theorem_with_label(*class, label.as_deref(), *span, ctx.source)?;
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
      // 環境単位ラベル（`split` / `multiline` の `[label=...]`）も同様にブロック先頭へ解決する。
      let mut anchor_labels: Vec<&str> = Vec::new();
      if let Some(env_label) = label.as_deref() {
        anchor_labels.push(env_label);
      }
      // 行ラベルは逆順で積む（「後から prepend」を繰り返す旧実装と同じ最終順序を 1 パスで
      // 再現するため。`with_label_anchors` の doc comment も参照）
      anchor_labels.extend(rows.iter().rev().filter_map(|row| row.label.as_deref()));
      nodes = with_label_anchors(&anchor_labels, nodes);
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
      let number =
        registry.increment_with_label(config::read_style::CounterName::Figure, label.as_deref(), *span, ctx.source)?;
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
      let number =
        registry.increment_with_label(config::read_style::CounterName::Table, label.as_deref(), *span, ctx.source)?;
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

/// 複数のラベルを先頭からこの順でアンカーとして 1 回の構築でまとめて付与する
///
/// `labels` が空なら `nodes` をそのまま返す。`labels[0]` が最終的な先頭アンカーになる
/// （[`with_label_anchor`] を繰り返し呼ぶより 1 回の Vec 構築で済む O(k+n) 版）。
fn with_label_anchors(labels: &[&str], nodes: Vec<LayoutNode>) -> Vec<LayoutNode> {
  if labels.is_empty() {
    return nodes;
  }
  let mut result = Vec::with_capacity(nodes.len() + labels.len());
  result.extend(labels.iter().map(|label| LayoutNode::Anchor(types::AnchorMark::Label((*label).to_string()))));
  result.extend(nodes);
  return result;
}

/// 見出しタイトルのプレーンテキストを、`\ref` を `CounterRegistry` で解決しながら組み立てる
///
/// [`document::try_inline_nodes_to_plain_text`] に resolver を注入する薄いラッパー。
/// `\ref` が未定義ラベルを参照している場合は黙って空文字列にせず
/// [`LoweringError::UnresolvedReference`] を返す。`source` は呼び出し元（このタイトルが属する
/// 見出しの [`PendingHeading::source`]）をそのままエラーへ引き継ぐための帰属ソース。
/// pass1 完了後（`registry` の labels テーブルが確定した後）にのみ呼ぶこと。
///
/// # Errors
///
/// タイトル中の `\ref` が未定義ラベルを参照している場合に [`LoweringError::UnresolvedReference`]
/// を返します。
fn try_resolve_heading_title_plain(
  title: &[document::InlineNode],
  registry: &counter::CounterRegistry,
  source: SourceId,
) -> Result<String, LoweringError> {
  let mut resolve_ref = |label: &str, span: miette::SourceSpan| -> Result<String, LoweringError> {
    return registry.resolve_label(label).map(str::to_string).ok_or_else(|| LoweringError::UnresolvedReference {
      label: label.to_string(),
      span,
      source_id: source,
    });
  };
  return try_inline_nodes_to_plain_text(title, &mut resolve_ref);
}

#[cfg(test)]
mod tests {
  use document::{HeadingLevel, InlineNode, ListItem, MathEnvKind, MathNode, MathRow, QuoteKind};
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
  fn nested_heading_gets_sequential_anchor_index() {
    // Arrange — トップレベル見出し → quote 本体内の見出し → トップレベル見出し、の順に並べる。
    // 修正前は lower_nodes_inner の heading_index がローカル変数で、quote 本体（再帰呼び出し）に
    // 入るたび 0 にリセットされていたため、本体内見出しの index が先頭見出しの index 0 と衝突していた。
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("Top1".to_string())]),
      DocNode::Quote {
        kind: QuoteKind::Quote,
        body: vec![DocNode::heading(
          HeadingLevel::Section,
          vec![InlineNode::Text("Nested".to_string())],
        )],
      },
      DocNode::heading(HeadingLevel::Section, vec![InlineNode::Text("Top2".to_string())]),
    ];

    // Act
    let (_layout, headings) = lower_sources_with_headings(&ctx, &[nodes.as_slice()]).expect("失敗しないはず");

    // Assert — quote 本体内の見出しを挟んでも index が 0, 1, 2 と重複なく連番になる
    assert_eq!(headings.len(), 3, "見出しは 3 件記録されるはず: {headings:?}");
    let mut indices: Vec<usize> = headings.iter().map(|h| h.index).collect();
    indices.sort_unstable();
    assert_eq!(indices, vec![0, 1, 2], "見出し index は重複なく連番のはず: {headings:?}");
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
    let mut style = config::read_style::Style::default();
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

  /// ラベル付き Chapter 見出しを作るテストヘルパ
  fn labeled_chapter(title: &str, label: &str) -> DocNode {
    return DocNode::Heading {
      level: HeadingLevel::Chapter,
      numbered: true,
      title: vec![InlineNode::Text(title.to_string())],
      label: Some(label.to_string()),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    };
  }

  #[test]
  fn numbering_continues_across_sources() {
    // Arrange — 2 ソースグループにそれぞれ Chapter 見出しを 1 つずつ置く
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let g0 = vec![DocNode::heading(
      HeadingLevel::Chapter,
      vec![InlineNode::Text("A".to_string())],
    )];
    let g1 = vec![DocNode::heading(
      HeadingLevel::Chapter,
      vec![InlineNode::Text("B".to_string())],
    )];

    // Act
    let (_layout, headings) =
      lower_sources_with_headings(&ctx, &[g0.as_slice(), g1.as_slice()]).expect("失敗しないはず");

    // Assert — 採番レジストリは全グループで共有され、2 番目のグループが 1 番目の続きから発番される
    assert_eq!(headings.len(), 2, "{headings:?}");
    assert_eq!(headings[0].number, "1", "1 グループ目の chapter は 1");
    assert_eq!(headings[1].number, "2", "2 グループ目の chapter は連番の 2: {headings:?}");
  }

  #[test]
  fn ref_resolves_across_sources() {
    // Arrange — グループ 0 でラベルを定義し、グループ 1 からそのラベルを `\ref` する
    // （グループごとに registry が分かれていれば解決できず、共有していれば解決できる）
    fn contains_internal_link(nodes: &[LayoutNode], target: &str) -> bool {
      return nodes.iter().any(|n| match n {
        LayoutNode::Link {
          target: types::LinkTarget::Internal(t),
          ..
        } => t == target,
        LayoutNode::VBox { children, .. } | LayoutNode::HBox { children, .. } => {
          contains_internal_link(children, target)
        },
        _ => false,
      });
    }

    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let g0 = vec![labeled_chapter("Intro", "ch:intro")];
    let g1 = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "ch:intro".to_string(),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }])];

    // Act
    let (layout, _headings) =
      lower_sources_with_headings(&ctx, &[g0.as_slice(), g1.as_slice()]).expect("跨りラベルは解決されるはず");

    // Assert — pass2 が `\ref` を内部リンク（Internal("ch:intro")）に解決している
    assert!(contains_internal_link(&layout, "ch:intro"), "跨りの \\ref が解決されるはず: {layout:?}");
  }

  #[test]
  fn duplicate_label_across_sources_reports_second_source_id() {
    // Arrange — グループ 0 と グループ 1 の両方で同名ラベル `dup` を定義する
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let g0 = vec![labeled_chapter("First", "dup")];
    let g1 = vec![labeled_chapter("Second", "dup")];

    // Act — 2 回目の登録（グループ 1）で衝突する
    let err = lower_sources_with_headings(&ctx, &[g0.as_slice(), g1.as_slice()])
      .expect_err("同名ラベルの重複はエラーになるはず");

    // Assert — DuplicateLabel の帰属ソースは衝突した 2 番目の定義（グループ 1）を指す
    let LoweringError::DuplicateLabel { ref label, .. } = err else {
      panic!("DuplicateLabel が期待されます: {err:?}");
    };
    assert_eq!(label, "dup");
    assert_eq!(err.source_id().index(), 1, "衝突した 2 番目の定義のグループ index を指すはず");
  }

  #[test]
  fn unresolved_ref_reports_owning_source_id() {
    // Arrange — グループ 1 に未定義ラベルへの `\ref` を置く（どのグループにも定義は無い）
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let g0 = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "plain".to_string(),
    )])];
    let g1 = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: miette::SourceSpan::from((0_usize, 0_usize)),
    }])];

    // Act
    let err = lower_sources_with_headings(&ctx, &[g0.as_slice(), g1.as_slice()])
      .expect_err("未定義ラベルへの \\ref はエラーになるはず");

    // Assert — UnresolvedReference の帰属ソースは `\ref` を含むグループ 1 を指す
    let LoweringError::UnresolvedReference { ref label, .. } = err else {
      panic!("UnresolvedReference が期待されます: {err:?}");
    };
    assert_eq!(label, "missing");
    assert_eq!(err.source_id().index(), 1, "\\ref を含むグループ index を指すはず");
  }

  #[test]
  fn heading_title_with_unresolved_ref_errors() {
    // Arrange — 既定の見出しフォーマット（"{number} {title}"）は {title} を含むので、
    // 見出しタイトル内の未定義ラベルへの \ref が headings 変換で検出されるはず
    let style = ReadStyle::default();
    let ctx = LoweringContext::new(&style);
    let nodes = vec![DocNode::heading(
      HeadingLevel::Chapter,
      vec![
        InlineNode::Text("見出し ".to_string()),
        InlineNode::Ref {
          label: "typo".to_string(),
          span: miette::SourceSpan::from((0_usize, 0_usize)),
        },
      ],
    )];

    // Act
    let err = lower_sources_with_headings(&ctx, &[nodes.as_slice()])
      .expect_err("見出しタイトル内の未定義ラベルへの \\ref はエラーになるはず");

    // Assert — UnresolvedReference として検出され、ラベルは期待どおり
    let LoweringError::UnresolvedReference { ref label, .. } = err else {
      panic!("UnresolvedReference が期待されます: {err:?}");
    };
    assert_eq!(label, "typo");
  }
}
