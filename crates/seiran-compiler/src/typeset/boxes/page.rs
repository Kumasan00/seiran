//! 縦組版の出力 [`Page`] と [`PlacedBlock`]。
//!
//! `typeset::breaking::break_pages` がすべてのレイアウト判断（行送り・改ページ・表の分割）を
//! 終えた確定座標を保持する。`publication::build` は描画命令へ単純変換するだけでよい。
//!
//! 座標系: `x` は本文左端からのオフセット、`y` はページ上端からの距離（下方向に正）。
//! 用紙左端からの絶対位置は、ページ自身が持つ [`Page::content_origin_x`] を描画時に
//! ちょうど 1 回加算して得る（余白の所有は `style.toml` の `[page]`、#389）。

use crate::{
  length::Length,
  project::ProjectPath,
  typeset::boxes::{
    hitem::HBox,
    line::{Line, PositionedBox},
    link::{AnchorMark, LinkTarget},
  },
};

/// 組版済みの 1 ページ
#[derive(Debug, Clone)]
pub(crate) struct Page {
  /// ページ内の配置済みブロック（上から順）
  pub blocks: Vec<PlacedBlock>,
  /// ヘッダー（ページ上端の余白領域に描く走り文）の配置済みブロック
  ///
  /// `break_pages` は空で生成し、ページ数確定後にヘッダー・フッター配置パス
  /// （`layout::build_running_content`）が埋める。本文と同じ [`PlacedBlock`] を流用するため、
  /// 描画 adapter は本文と同一の変換ロジックで扱える。
  pub header: Vec<PlacedBlock>,
  /// フッター（ページ下端の余白領域に描く走り文）の配置済みブロック
  pub footer: Vec<PlacedBlock>,
  /// このページに出現した脚注（本文下部、出現順）
  ///
  /// `break_pages` がページ確定時に埋める（`header`/`footer` と異なり走り文ではなく、
  /// 本文の実効下限から差し引いた分の実領域に確定座標で配置済み）。脚注番号ごとに
  /// [`PlacedFootnote`] を分けて保持する。区切り罫線は `break_pages` がリージョン内最初の
  /// `PlacedFootnote.blocks` の先頭に `PlacedBlock::Rule` として混ぜて持たせる。
  pub footnotes: Vec<PlacedFootnote>,
  /// このページに解決されたリンク到達先アンカー（機構 A）
  ///
  /// 描画 adapter がページ index + 座標から `XyzDestination` を作り、PDF しおりや
  /// 内部リンクの行き先として登録する。
  pub anchors: Vec<PlacedAnchor>,
  /// このページに確定したクリック可能なリンク領域（機構 B）
  ///
  /// 描画 adapter が各ページにリンク注釈として付与する。
  pub links: Vec<PlacedLink>,
  /// このページに出現した索引語（重複除去済み、出現順）
  ///
  /// `break_pages` がページ確定時に `Line::index_marks` から集約する。同一 word/reading の
  /// 重複は 1 出現に畳む。座標を持たない — 索引は場所ではなくページ番号だけを必要とするため
  /// （[`PlacedAnchor`] とは異なる）。
  pub index_entries: Vec<PlacedIndexEntry>,
  /// ページ背景色（RGB）。`None` は塗りつぶさない。`style` 非依存のため
  /// 生の `[u8; 3]` で保持する（`PlacedBlock::Rule.color` と同じ規約）
  pub background_color: Option<[u8; 3]>,
  /// このページの本文水平原点（用紙左端から本文左端まで）
  ///
  /// `typeset` が `style.page.margin_left` から解決して載せる。ページごとに持つのは、
  /// 見開きで左右の余白を変える将来の拡張でも描画側（`publication::build`）の
  /// interface を変えずに済ませるため（#389）。
  pub content_origin_x: Length,
}

/// ページ下部に配置された脚注 1 個（または長い脚注の断片）
///
/// 本体は複数行に分かれ得るため `blocks`（通常は [`PlacedBlock::Line`] の列）として保持する。
/// リージョン内で最初に確定する脚注だけは、区切り罫線（`style.footnote`）が
/// [`PlacedBlock::Rule`] として `blocks` の先頭に混ざる（2 個目以降の脚注には付かない）。
/// 座標系は [`PlacedBlock`] と同じ（本文左端・ページ上端からの距離）。
///
/// 脚注 1 個がページ下部に収まらないときは行単位で分割され、同じ `number` / `index` を持つ
/// [`PlacedFootnote`] が複数ページに現れる（`continued` で区別する）。
#[derive(Debug, Clone)]
pub(crate) struct PlacedFootnote {
  /// 発番済みの表示番号（[`super::line::LineFootnote`] から素通し）
  #[cfg_attr(
    not(test),
    expect(
      dead_code,
      reason = "描画はマーカー側の番号を使うため読まない（crate 内の `#[cfg(test)]` が採番を検証する）"
    )
  )]
  pub number: u32,
  /// 出現順の識別子（0 起点。[`super::line::LineFootnote`] から素通し）
  ///
  /// ページ単位採番の反復（`seiran_compiler::compiler`）が、確定したページ列から
  /// 「どの脚注に何番を振り直すか」を決めるためのキー。
  pub index: u32,
  /// 前ページからの繰越（長い脚注の続き）か
  ///
  /// `true` のとき、この断片は本体の先頭ではないため番号マーカーを持たない（マーカーは
  /// 分割前の先頭行に入っており、分割は行単位なので繰越側には現れない）。マーカーのある行を
  /// 持つ先頭の断片だけが `false` になる。
  ///
  /// 「この脚注がこのページで始まったか」を表すので、ページ単位採番
  /// （`typeset::pagination::footnote_numbering`）は `true` の断片を数えない — 数えると繰越先の
  /// ページで番号を振り直してしまう。
  pub continued: bool,
  /// 脚注本体の配置済みブロック（改行があれば複数の [`PlacedBlock::Line`]。リージョン内最初の
  /// 脚注は先頭に区切り罫線 [`PlacedBlock::Rule`] を持つ）
  pub blocks: Vec<PlacedBlock>,
}

/// 確定座標に解決されたリンク到達先アンカー
///
/// 座標系は [`PlacedBlock`] と同じ（`x` は本文左端からのオフセット、`y` はページ上端から
/// 下方向に正）。描画 adapter が左マージンを加算して `XyzDestination` 点にする。
#[derive(Debug, Clone)]
pub(crate) struct PlacedAnchor {
  /// アンカー種別（見出し / ラベル付きブロック）
  pub mark: AnchorMark,
  /// 本文左端からの水平オフセット（pt、通常 0）
  pub x: Length,
  /// ページ上端からの距離（pt）
  pub y: Length,
}

/// 確定座標に解決されたクリック可能なリンク領域
///
/// 座標系は [`PlacedBlock`] と同じ（`x` / `y` はそれぞれ本文左端・ページ上端からの距離）。
/// 描画 adapter が左マージンを加算して矩形のリンク注釈にする。
#[derive(Debug, Clone)]
pub(crate) struct PlacedLink {
  /// リンクの行き先（内部アンカー / 外部 URI）
  pub target: LinkTarget,
  /// 矩形左端の本文左端からの水平オフセット（pt）
  pub x: Length,
  /// 矩形上端のページ上端からの距離（pt）
  pub y: Length,
  /// 矩形の幅（pt）
  pub width: Length,
  /// 矩形の高さ（pt）
  pub height: Length,
}

/// ページに出現した索引語 1 件
///
/// `HItem::IndexMark` → `Line::index_marks` を経て `break_pages` がページ単位に集約する。
#[derive(Debug, Clone)]
pub(crate) struct PlacedIndexEntry {
  /// 索引語
  pub word: String,
  /// 読みソートキー（`[reading=...]`）
  pub reading: Option<String>,
}

/// ページ内に配置されたブロック
#[derive(Debug, Clone)]
pub(crate) enum PlacedBlock {
  /// テキスト行
  Line {
    /// 行の内容
    line: Line,
    /// ベースラインのページ上端からの距離（pt）
    baseline_y: Length,
  },
  /// 表の断片（このページに描く行の集まり。改ページ後のヘッダ再描画行も含む）
  Table {
    /// このページに描く行（上から順、セル内容・罫線とも位置確定済み）
    rows: Vec<PlacedTableRow>,
  },
  /// 画像
  Image {
    /// 画像ファイルへのパス
    path: ProjectPath,
    /// 本文左端からの水平オフセット（pt）
    x: Length,
    /// ページ上端からの距離（pt、画像上端）
    y: Length,
    /// 描画幅（pt）
    width: Length,
    /// 描画高さ（pt）
    height: Length,
    /// ラスタ画像のダウンサンプリング上限 DPI。`None` ならリサイズなし
    target_dpi: Option<u32>,
  },
  /// 罫線（塗りつぶし矩形）
  Rule {
    /// 本文左端からの水平オフセット（pt）
    x: Length,
    /// ページ上端からの距離（pt、矩形上端）
    y: Length,
    /// 幅（pt）
    width: Length,
    /// 高さ（pt）
    height: Length,
    /// 塗り色（RGB）。`None` は黒。`style` 非依存のため生の `[u8; 3]` で保持する
    color: Option<[u8; 3]>,
  },
  /// ディスプレイ数式ブロック（本体 Atom + 行番号、いずれも確定座標）
  MathBlock {
    /// 数式本体（閉じた Atom）
    body: HBox,
    /// 本体の本文左端からの水平オフセット（pt、揃えで算出済み）
    x: Length,
    /// 本体ベースラインのページ上端からの距離（pt）
    baseline_y: Length,
    /// 行番号（位置確定済み）
    numbers: Vec<PlacedMathNumber>,
  },
}

/// 配置確定済みの数式行番号
///
/// 座標系は [`PlacedBlock`] と同じ（`x` は本文左端から、`baseline_y` はページ上端から下方向）。
#[derive(Debug, Clone)]
pub(crate) struct PlacedMathNumber {
  /// 番号ボックス（シェーピング済み）
  pub content: HBox,
  /// 本文左端からの水平オフセット（pt）
  pub x: Length,
  /// ベースラインのページ上端からの距離（pt）
  pub baseline_y: Length,
}

/// 位置確定済みの表の 1 行
///
/// セルの x（段オフセット + 表の揃え + 列の揃え + セル余白 + 行内カーソル）は `Length`（sp 整数）
/// のまま足し込んであり、pt の `f32` へ変換するのは描画命令を作る 1 回だけである。
#[derive(Debug, Clone)]
pub(crate) struct PlacedTableRow {
  /// 行帯上端のページ上端からの距離（pt）
  pub top_y: Length,
  /// 行帯の高さ（pt）
  pub height: Length,
  /// セル内容が共有するベースラインのページ上端からの距離（pt）
  pub baseline_y: Length,
  /// 本文左端からの絶対 x 座標へ配置済みのセル内容
  pub boxes: Vec<PositionedBox>,
  /// 行の上罫線。`None` は罫線なし
  pub rule: Option<PlacedTableRule>,
}

/// 位置と見た目が確定した表の横罫線
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlacedTableRule {
  /// 本文左端からの水平オフセット（pt）
  pub x: Length,
  /// ページ上端からの距離（pt）
  pub y: Length,
  /// 罫線の幅（pt）
  pub width: Length,
  /// 罫線の高さ（pt）
  pub height: Length,
  /// 罫線色（RGB）。`None` は黒
  pub color: Option<[u8; 3]>,
}
