//! 解決済みブロック要素。`model::DocNode` と 1:1 だが、ラベル宣言箇所が `LabelId` を持つ

use model::{
  AssetId, CaptionPosition, CitationId, ColumnAlign, ColumnWidth, HeadingLevel, LabelId, Length, MathEnvKind, MathNode,
  QuoteKind, Span, TheoremClass,
};

use crate::{counter::CounterValue, inline::ResolvedInline};

/// 解決済みブロックレベル要素
///
/// ラベルが無い要素（`[label=...]` 未指定の図・表・式・定理）にも表示番号は付きうるため、
/// `counter_value` はラベルの有無と独立に持つ（`ResolvedDocument.counter_values` は
/// `LabelId` キーのマップなのでラベル無し要素を表現できない — この埋め込みで解消する）。
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedNode {
  /// 見出し
  Heading {
    /// 見出しのレベル
    level: HeadingLevel,
    /// 採番対象かどうか
    numbered: bool,
    /// 見出しのタイトル
    title: Vec<ResolvedInline>,
    /// 宣言されたラベル（`\section[label=...]`）。登録済みなので重複は起こり得ない
    label: Option<LabelId>,
    /// 見出しコマンドのソース位置
    span: Span,
  },
  /// 段落
  Paragraph(Vec<ResolvedInline>),
  /// 箇条書きリスト（構造は `DocNode::List` と同一。中身だけ再帰的に解決済み）
  List {
    /// 順序付き（enumerate）かどうか
    ordered: bool,
    /// リストアイテム
    items: Vec<ResolvedListItem>,
    /// 開始番号（`enumerate[start=N]`）
    start: Option<u32>,
    /// 項目間の縦アキの上書き
    item_gap: Option<Length>,
  },
  /// ディスプレイ数式環境
  MathBlock {
    /// 環境種別
    kind: MathEnvKind,
    /// 行（各行は `&` 区切りの列を持つ）
    rows: Vec<ResolvedMathRow>,
    /// 環境全体で 1 つ採番するか（`split` / `multiline` 用）
    numbered: bool,
    /// `\ref{eq:foo}` 解決用の環境単位ラベル
    label: Option<LabelId>,
    /// 環境単位の採番値（`split` / `multiline` 用。行単位採番の環境や無採番では `None`）
    counter_value: Option<CounterValue>,
    /// 環境のソース位置
    span: Span,
  },
  /// 図環境
  Figure {
    /// 画像ファイルへのパス
    image_path: AssetId,
    /// 画像の幅
    width: Option<Length>,
    /// 画像の高さ
    height: Option<Length>,
    /// `\image[dpi=...]` の per-image 上書き
    dpi: Option<u32>,
    /// `\image[downsample=...]` の per-image 上書き
    downsample: Option<bool>,
    /// キャプションのインライン要素
    caption: Option<Vec<ResolvedInline>>,
    /// キャプションを図本体の上下どちらに配置するか
    caption_position: CaptionPosition,
    /// `\ref{fig:foo}` 解決用ラベル
    label: Option<LabelId>,
    /// この図の採番値
    counter_value: CounterValue,
    /// 環境のソース位置
    span: Span,
  },
  /// 表環境
  Table {
    /// 列ごとの揃え方向
    columns: Vec<ColumnAlign>,
    /// 列ごとの幅指定
    widths: Vec<ColumnWidth>,
    /// ヘッダ行
    head: Vec<ResolvedTableRow>,
    /// 本体行
    rows: Vec<ResolvedTableRow>,
    /// キャプションのインライン要素
    caption: Option<Vec<ResolvedInline>>,
    /// キャプションを表本体の上下どちらに配置するか
    caption_position: CaptionPosition,
    /// `\ref{tab:foo}` 解決用ラベル
    label: Option<LabelId>,
    /// この表の採番値
    counter_value: CounterValue,
    /// 環境のソース位置
    span: Span,
    /// 改ページによる分割を許可するか
    breakable: bool,
  },
  /// 定理ブロック
  Theorem {
    /// 定理クラス（`theorem` / `lemma` / … / `proof`）
    class: TheoremClass,
    /// サブタイトル（`[title="..."]` の中身）。未指定は `None`
    title: Option<String>,
    /// 本体
    body: Vec<ResolvedNode>,
    /// `proof` の `[of=...]`（解決済み — 参照先は必ず存在する）
    of: Option<LabelId>,
    /// `\ref{thm:foo}` 解決用ラベル
    label: Option<LabelId>,
    /// この定理の採番値（`unnumbered` クラスは `None`）
    counter_value: Option<CounterValue>,
    /// 環境のソース位置
    span: Span,
  },
  /// 引用ブロック
  Quote {
    /// 引用の種別（`quote` / `quotation`）
    kind: QuoteKind,
    /// 本体
    body: Vec<ResolvedNode>,
  },
  /// 罫線
  Rule {
    /// 幅
    width: Length,
    /// 高さ
    height: Length,
  },
  /// 改ページ
  PageBreak,
  /// 固定幅スペース
  Space(Length),
  /// 参考文献エントリのアンカー（citation クレートが合成。既に typed なので無変更）
  Anchor(CitationId),
}

/// 解決済みリスト項目
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedListItem {
  /// 項目本体
  pub content: Vec<ResolvedNode>,
  /// `\item[marker=...]` で指定された個別マーカー文字列
  pub marker: Option<String>,
  /// この項目直後の縦アキの個別上書き
  pub item_gap: Option<Length>,
}

/// 解決済み数式行
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedMathRow {
  /// セル（`&` 区切りの列）
  pub cells: Vec<Vec<MathNode>>,
  /// 行単位で採番するか
  pub numbered: bool,
  /// 行ラベル（登録済み）
  pub label: Option<LabelId>,
  /// 行末マーカー `\label{...}` のソース位置。`None` の場合は環境の `span` をフォールバックとして使う
  pub label_span: Option<Span>,
  /// 行単位の採番値（`numbered = false` の行では `None`）
  pub counter_value: Option<CounterValue>,
}

/// 解決済みテーブル行
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTableRow {
  /// セル
  pub cells: Vec<ResolvedTableCell>,
  /// この行の上に横罫線を引くか（`\row[rule_above]{...}`）
  pub rule_above: bool,
}

/// 解決済みテーブルセル
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedTableCell {
  /// セル内容
  pub content: Vec<ResolvedInline>,
  /// 列方向の結合数
  pub span: u32,
}
