//! HIR のブロックノード [`HirNode`] と付随構造。

use crate::{
  length::Length,
  model::{
    CaptionPosition, ColumnAlign, ColumnWidth, HeadingLevel, MathEnvKind, QuoteKind, TheoremClass,
    hir::{HirInline, HirMathRow, NodeId},
  },
  project::ProjectPath,
};

/// ブロックレベルの HIR ノード
///
/// 位置は各 variant ではなく `SourceMap` が `id` をキーに保持する。
#[derive(Debug, PartialEq)]
pub(crate) struct HirNode {
  /// このノードの ID
  pub(crate) id: NodeId,
  /// ノードの種別と内容
  pub(crate) kind: HirNodeKind,
}

impl HirNode {
  /// ID と種別からノードを作る
  pub(crate) fn new(id: NodeId, kind: HirNodeKind) -> Self { return HirNode { id, kind }; }
}

/// ブロックノードの種別
///
/// 著者が書いた内容だけを持つ。`GeneratedBlock::Anchor`（書誌エントリのアンカー）は
/// CSL 整形ステージの生成物なので HIR には無い。見出しの `numbered` も、frontend が
/// 作る見出しは常に採番対象で構造的に一意に決まるため持たない。
#[derive(Debug, PartialEq)]
pub(crate) enum HirNodeKind {
  /// 見出し（`\part` 〜 `\subparagraph`）
  Heading {
    /// 見出しのレベル（Part〜Subparagraph）
    level: HeadingLevel,
    /// 見出しのタイトル（インライン要素として保持）
    title: Vec<HirInline>,
    /// `\section[label=sec:intro]{...}` 形式で付与された参照ラベル名
    label: Option<String>,
  },

  /// 段落（インライン要素の集合）
  Paragraph(Vec<HirInline>),

  /// 箇条書きリスト（`\begin{itemize}` / `\begin{enumerate}`）
  List {
    /// 順序付き（enumerate）かどうか
    ordered: bool,
    /// リストアイテム
    items: Vec<HirListItem>,
    /// 開始番号（`enumerate[start=N]`）。`None` は既定（1 から開始）
    start: Option<u32>,
    /// 項目間の縦アキの上書き。`None` は style.toml の既定値
    item_gap: Option<Length>,
  },

  /// ディスプレイ数式環境（`equation` / `align` / `gather` / `split` / `multiline` / `cases` / `matrix`）
  MathBlock {
    /// 環境種別
    kind: MathEnvKind,
    /// 行（各行は `&` 区切りの列を持つ）
    rows: Vec<HirMathRow>,
    /// 環境全体で 1 つ採番するか（`split` / `multiline` 用）
    numbered: bool,
    /// 環境単位のラベル名（`split` / `multiline` の `[label=...]`）
    label: Option<String>,
  },

  /// 図環境（`\begin{figure}...\end{figure}`）
  Figure {
    /// 画像ファイルへのパス（`\image{...}` の必須引数）
    image_path: ProjectPath,
    /// 画像の幅（未指定なら描画段で本文幅 / 縦横比から算出）
    width: Option<Length>,
    /// 画像の高さ（未指定なら描画段で本文幅 / 縦横比から算出）
    height: Option<Length>,
    /// `\image[dpi=...]` の per-image 上書き
    dpi: Option<u32>,
    /// `\image[downsample=...]` の per-image 上書き
    downsample: Option<bool>,
    /// キャプションのインライン要素（`\caption{...}` の中身）。未指定なら `None`
    caption: Option<Vec<HirInline>>,
    /// キャプションを図本体の上下どちらに配置するか
    caption_position: CaptionPosition,
    /// `\ref{fig:foo}` 解決用のラベル名
    label: Option<String>,
  },

  /// 表環境（`\begin{table}...\end{table}`）
  Table {
    /// 列ごとの揃え方向（列数に正規化済み）
    columns: Vec<ColumnAlign>,
    /// 列ごとの幅指定（列数に正規化済み）
    widths: Vec<ColumnWidth>,
    /// ヘッダ行（`\head{...}` 内の `\row`）
    head: Vec<HirTableRow>,
    /// 本体行（`\row{...}`）
    rows: Vec<HirTableRow>,
    /// キャプションのインライン要素（`\caption{...}` の中身）。未指定なら `None`
    caption: Option<Vec<HirInline>>,
    /// キャプションを表本体の上下どちらに配置するか
    caption_position: CaptionPosition,
    /// `\ref{tab:foo}` 解決用のラベル名
    label: Option<String>,
    /// 改ページによる分割を許可するか（`[breakable=false]` で禁止、既定 `true`）
    breakable: bool,
  },

  /// 定理ブロック（`\begin{theorem}...\end{theorem}` 等の 10 種）
  Theorem {
    /// 定理クラス（`theorem` / `lemma` / … / `proof`）
    class: TheoremClass,
    /// サブタイトル（`[title="..."]` の中身）。未指定は `None`
    title: Option<String>,
    /// 本体（再帰評価されたブロックノード列）
    body: Vec<HirNode>,
    /// `proof` の `[of=label]` 参照（証明対象の定理）。`proof` 以外や未指定は `None`
    of: Option<HirProofTarget>,
    /// `\ref{thm:foo}` 解決用のラベル名。未指定は `None`
    label: Option<String>,
  },

  /// 引用ブロック（`\begin{quote}` / `\begin{quotation}`）
  Quote {
    /// 引用の種別（`quote` / `quotation`）
    kind: QuoteKind,
    /// 本体（再帰評価されたブロックノード列）
    body: Vec<HirNode>,
  },

  /// 罫線（描画線）
  // `\rule` は未実装で frontend が構築しない。IR と lowering の受け口だけ先にある。
  #[allow(dead_code)]
  Rule {
    /// 幅
    width: Length,
    /// 高さ
    height: Length,
  },

  /// 改ページ
  PageBreak,

  /// 固定幅スペース（`\space{N}` コマンド）
  Space(Length),
}

/// リストの個別アイテム（`\item` に対応）
#[derive(Debug, PartialEq)]
pub(crate) struct HirListItem {
  /// このアイテムの ID
  pub(crate) id: NodeId,
  /// アイテムの内容（段落、ネストされたリスト等）
  pub(crate) content: Vec<HirNode>,
  /// `\item[marker=...]` で指定された個別マーカー文字列
  ///
  /// `None` は自動生成マーカーを使うこと、`Some("")` はマーカーを表示しないことを表す。
  pub(crate) marker: Option<String>,
  /// この項目直後の縦アキの個別上書き。`None` は環境・style 既定へフォールバック
  pub(crate) item_gap: Option<Length>,
}

/// 表の 1 行（`\row{...}` に対応）
#[derive(Debug, PartialEq)]
pub(crate) struct HirTableRow {
  /// この行の ID
  pub(crate) id: NodeId,
  /// 行内のセル（ソース上の `&` 区切り、または `\cell[...]{...}`）
  pub(crate) cells: Vec<HirTableCell>,
  /// この行の上に横罫線を引くか（`\row[rule_above]{...}`）
  pub(crate) rule_above: bool,
}

/// 表の 1 セル
#[derive(Debug, PartialEq)]
pub(crate) struct HirTableCell {
  /// このセルの ID
  pub(crate) id: NodeId,
  /// セルの内容（インライン要素）
  pub(crate) content: Vec<HirInline>,
  /// 列方向の結合数（colspan、1 以上）。ソース位置ではない
  pub(crate) span: u32,
}

/// `proof` 環境の `[of=label]` 参照（証明対象の定理）
#[derive(Debug, PartialEq)]
pub(crate) struct HirProofTarget {
  /// `[of=...]` 任意引数自身の ID（未解決時の診断位置に使う）
  pub(crate) id: NodeId,
  /// 参照先のラベル名（`[of=thm:foo]` の `thm:foo`）
  pub(crate) label: String,
}
