//! 縦組版 — ブロック列をページへ配置する

mod footnote_packing;
mod page_draft;
mod paragraph_plan;
mod region_cursor;

use std::fmt;

use footnote_packing::{
  FootnoteCharges, FootnoteDemand, LineFootnoteFit, fit_line_footnotes, footnote_area_full, pack_footnotes,
  split_pending,
};
use itertools::izip;
use page_draft::PageDraft;
use paragraph_plan::plan_paragraph_lines;
use region_cursor::RegionCursor;
use tracing::debug;

use crate::{
  document::{NodeId, TextAlignment},
  length::Length,
  project::ProjectPath,
  typeset::{
    boxes::{
      Align, Block, HBox, HItem, Line, MathRowNumber, MeasuredFootnote, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK,
      Page, PlacedBlock, PlacedMathNumber, TableBox, TableRowFragment,
    },
    breaking::break_lines::LineBreaker,
    geometry::{PageGeometry, column_width},
    observe,
  },
};

/// はみ出しとみなす超過量の下限（0.1pt。TeX の `\hfuzz` / `\vfuzz` の既定値と同じ）。
///
/// 両端揃えの伸縮を箱ごとに sp へ丸めた誤差で、段幅ちょうどの行が数 sp 超えることがあるため。
const OVERFLOW_TOLERANCE: Length = Length::from_sp(6554);

/// 内容が配置先の段・ページに収まらないまま配置された事実。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::typeset) struct Overflow {
  /// はみ出しが起きたページの 0 起点 index（この [`break_pages`] 呼び出しが返すページ列の中での位置。
  /// 前付け・後付けを連結した物理ページ番号ではない）
  pub page_index: usize,
  /// はみ出し方
  pub kind: OverflowKind,
}

/// [`Overflow`] のはみ出し方
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::typeset) enum OverflowKind {
  /// 1 行に付いた脚注群が、空のリージョンでもページ全高に収まらなかった（出現順）
  FootnoteLine {
    /// はみ出した脚注群の表示番号と、それぞれが帰属する `\footnote` コマンド
    footnotes: Vec<(u32, NodeId)>,
  },
  /// 繰越脚注の先頭 1 行がページ全高を超えた
  FootnoteSingleLine {
    /// はみ出した脚注の表示番号
    number: u32,
    /// はみ出した脚注が帰属する `\footnote` コマンド
    origin: NodeId,
  },
  /// 行（本文・合成行・脚注本体）が段の幅か実効の本文下限を超えた
  Line {
    /// 行を含む要素（脚注本体の行は `\footnote` コマンド）。単一のソース要素に対応しない生成物は `None`
    origin: Option<NodeId>,
    /// 行の内容の要約（識別用）
    excerpt: String,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
  },
  /// 画像が段の幅か実効の本文下限を超えた
  Image {
    /// `\image` コマンド
    origin: NodeId,
    /// 画像ファイルへのパス
    path: ProjectPath,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
  },
  /// ディスプレイ数式の本体が段の幅か実効の本文下限を超えた（行番号は測らない）
  Math {
    /// 数式環境
    origin: NodeId,
    /// 数式本体の要約（識別用）
    excerpt: String,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
  },
  /// 表の幅が段の幅を超えた（表 1 個につき 1 件。最初の行が着地したページに記録する）
  Table {
    /// 表環境
    origin: NodeId,
    /// 超過量
    excess: Length,
  },
  /// 表の行が段の幅か実効の本文下限を超えた
  TableRow {
    /// `\row`
    origin: NodeId,
    /// はみ出した方向
    axis: OverflowAxis,
    /// 超過量
    excess: Length,
  },
}

/// はみ出しの方向
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverflowAxis {
  /// 段の右端を超えた
  Horizontal,
  /// 実効の本文下限を超えた
  Vertical,
}

impl fmt::Display for OverflowAxis {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    return f.write_str(match self {
      OverflowAxis::Horizontal => "横",
      OverflowAxis::Vertical => "縦",
    });
  }
}

/// `extent` が `limit` を [`OVERFLOW_TOLERANCE`] より大きく超えていれば、その超過量を返す
fn excess(extent: Length, limit: Length) -> Option<Length> {
  let over = extent - limit;
  return (over > OVERFLOW_TOLERANCE).then_some(over);
}

/// 縦組版の内部状態（確定ページ列・カーソル・脚注の予約と繰越）
struct PageComposer {
  /// 確定済みページ
  pages: Vec<Page>,
  /// 現在ページの配置台帳
  draft: PageDraft,
  /// リージョン内のカーソル（位置・底辺基準フラグ・脚注予約）
  cursor: RegionCursor,
  /// 段組み数（1 = 単段）
  num_columns: usize,
  /// 1 段あたりの幅
  column_width: Length,
  /// 段間（gutter）
  column_gap: Length,
  /// 現在の段インデックス（0 = 左段）
  col: usize,
  /// 現在リージョン（段）に集約された脚注（出現順、行分割済み）
  region_footnotes: Vec<PendingFootnote>,
  /// 次リージョンへ繰り越す脚注の残り（出現順）。
  carry: Vec<PendingFootnote>,
  /// 収まらないまま配置した内容の記録（検出順＝ページ順）
  overflows: Vec<Overflow>,
}

/// 現在リージョンに集約された脚注 1 個（行分割済み、未確定座標）
struct PendingFootnote {
  /// 帰属する `\footnote` コマンド
  origin: NodeId,
  /// 発番済みの表示番号
  number: u32,
  /// 出現順の識別子（0 起点）
  index: u32,
  /// 前リージョンからの繰越（続き）か
  continued: bool,
  /// 行分割済みの本体（このリージョンに置く分だけに切り詰め済み）
  lines: Vec<Line>,
  /// 行送り
  leading: Length,
}

impl PageComposer {
  /// 先頭ページの初期状態で `PageComposer` を生成する
  fn new(geom: &PageGeometry, column_width: Length) -> Self {
    return PageComposer {
      pages: Vec::new(),
      draft: PageDraft::new(),
      cursor: RegionCursor {
        y: geom.margin_top,
        at_edge: false,
        footnote_reserved: Length::ZERO,
      },
      num_columns: geom.num_columns.max(1),
      column_width,
      column_gap: geom.column_gap,
      col: 0,
      region_footnotes: Vec::new(),
      carry: Vec::new(),
      overflows: Vec::new(),
    };
  }

  /// 現在の段の左端 x オフセット（本文左端基準）。段 `k` は `k * (段幅 + 段間)` だけ右へ寄る
  fn column_offset(&self) -> Length {
    #[expect(
      clippy::cast_precision_loss,
      reason = "段インデックスは実用上 0〜1 で、f32 で精度を失う桁数にならない"
    )]
    let col = self.col as f32;
    return (self.column_width + self.column_gap) * col;
  }

  /// ページ下限を超えたときの遷移。次の段があれば改段、なければ改ページし、繰越脚注を新リージョンへ詰める。
  fn advance_region(&mut self, geom: &PageGeometry) {
    // 下端揃えの対象は満杯で閉じるリージョンだけ（強制改ページ・最終ページは `flush = false` で閉じる）
    self.end_region(geom, true);
    self.next_region(geom);
    self.seed_carry(geom);
  }

  /// 次のリージョン（段 / ページ）へ移るだけの遷移（リージョン確定・繰越の詰め込みは行わない）
  fn next_region(&mut self, geom: &PageGeometry) {
    if self.col + 1 < self.num_columns {
      self.col += 1;
      self.cursor.y = geom.margin_top;
      self.cursor.at_edge = false;
    } else {
      self.start_new_page(geom);
    }
  }

  /// 強制改ページ（[`PENALTY_FORCE_BREAK`]）。新ページを開始し、繰越脚注を詰める。
  fn force_new_page(&mut self, geom: &PageGeometry) {
    self.start_new_page(geom);
    self.seed_carry(geom);
  }

  /// 繰越脚注（`carry`）を新しいリージョンの脚注エリアの先頭へ 1 リージョンぶん詰める。
  fn seed_carry(&mut self, geom: &PageGeometry) {
    if self.carry.is_empty() {
      return;
    }
    let charges = FootnoteCharges::of(geom);
    let demands: Vec<FootnoteDemand> = self
      .carry
      .iter()
      .map(|pending| return FootnoteDemand::new(&pending.lines, pending.leading))
      .collect();
    // はみ出しの記録に使う繰越先頭の表示番号と起源（`pack_footnotes` が `overflowed` を立てるのは
    // 先頭の脚注に限られる）
    let (leading_number, leading_origin) = (self.carry[0].number, self.carry[0].origin);
    // 繰越は「そのページの自前の脚注より前」に置くので、常にエリア先頭（`base_reserved` = 0）から詰める
    let packing = pack_footnotes(&demands, Length::ZERO, geom.page_limit - geom.margin_top, charges, false)
      .expect("繰越の詰め込みは先頭に最低 1 行を強制するので None にならない");
    if packing.overflowed {
      self.record(OverflowKind::FootnoteSingleLine {
        number: leading_number,
        origin: leading_origin,
      });
    }
    self.cursor.footnote_reserved = packing.height;
    let mut rest = Vec::new();
    for (pending, &placed) in std::mem::take(&mut self.carry).into_iter().zip(&packing.splits) {
      let (head, tail) = split_pending(pending, placed);
      if let Some(head) = head {
        self.push_region_footnote(head);
      }
      if let Some(tail) = tail {
        rest.push(tail);
      }
    }
    self.carry = rest;
  }

  /// 現在ページを確定し、新しいページを開始する
  fn start_new_page(&mut self, geom: &PageGeometry) {
    // 強制改ページは `advance_region` を経ないので、残っている脚注をここで確定させる
    // （`advance_region` 経由では確定済みで、この二重呼び出しは無害）
    self.end_region(geom, false);
    // 本文 block も確定脚注も無ければ白紙ページを作らない。未解決アンカーは `draft` が持ち越す
    if !self.draft.has_content() {
      return;
    }
    self.pages.push(self.draft.take_page(geom));
    self.cursor.y = geom.margin_top;
    self.cursor.at_edge = false;
    self.col = 0;
  }

  /// 全ブロックの配置後に最終ページを確定し、ページ列とはみ出し記録を返す
  fn finish(mut self, geom: &PageGeometry) -> (Vec<Page>, Vec<Overflow>) {
    // 末尾に残った未解決アンカーは現在カーソル位置（現在の段の左端）で解決する
    let x = self.column_offset();
    self.draft.land_anchors(x, self.cursor.y);
    self.end_region(geom, false);
    // 文書末尾の行で分割された脚注の繰越を、繰越だけのリージョンへ尽きるまで出す
    // （[`PageComposer::seed_carry`] は 1 リージョンぶんしか詰めない）
    while !self.carry.is_empty() {
      self.next_region(geom);
      self.seed_carry(geom);
      self.end_region(geom, false);
    }
    // 本文も確定脚注も無い末尾ページは push しない（`start_new_page` と同じ述語）。ただし 1 ページも
    // 確定していなければ push する（空文書でも最低 1 ページを返す `break_pages` の事後条件のため）。
    if self.pages.is_empty() || self.draft.has_content() {
      self.pages.push(self.draft.take_page(geom));
    }
    return (self.pages, self.overflows);
  }

  /// はみ出しを現在ページの記録として積む。
  ///
  /// `page_index` は確定済みページ数なので、改リージョン後（着地ページが確定してから）呼ぶこと。
  fn record(&mut self, kind: OverflowKind) {
    self.overflows.push(Overflow {
      page_index: self.pages.len(),
      kind,
    });
  }

  /// 段左端基準の右端 `right` が段の幅を、下端 `bottom` が下限 `limit` を超えていれば、横 → 縦の順に
  /// `kind(方向, 超過量)` を記録する
  fn record_extent_overflow(
    &mut self,
    right: Length,
    bottom: Length,
    limit: Length,
    kind: impl Fn(OverflowAxis, Length) -> OverflowKind,
  ) {
    let overflows = [
      (OverflowAxis::Horizontal, excess(right, self.column_width)),
      (OverflowAxis::Vertical, excess(bottom, limit)),
    ];
    for (axis, over) in overflows {
      if let Some(excess) = over {
        self.record(kind(axis, excess));
      }
    }
  }

  /// `origin` に帰属する段左端基準の行 `line`（下端 `bottom`）のはみ出しを、下限 `limit` に対して記録する
  fn record_line_overflow(&mut self, line: &Line, bottom: Length, limit: Length, origin: Option<NodeId>) {
    self.record_extent_overflow(line.width(), bottom, limit, |axis, excess| {
      return OverflowKind::Line {
        origin,
        excerpt: observe::summarize_line(line),
        axis,
        excess,
      };
    });
  }

  /// 現在リージョンに置く脚注を積む。本体の各行が段の幅を超えていれば記録する
  ///
  /// 脚注の行は着地したリージョンでちょうど 1 回ここを通る（繰越の残りは次のリージョンで改めて積まれる）。
  /// 縦のはみ出しは脚注の詰め込み（[`OverflowKind::FootnoteLine`] / [`OverflowKind::FootnoteSingleLine`]）が記録する。
  fn push_region_footnote(&mut self, head: PendingFootnote) {
    for line in &head.lines {
      if let Some(excess) = excess(line.width(), self.column_width) {
        self.record(OverflowKind::Line {
          origin: Some(head.origin),
          excerpt: observe::summarize_line(line),
          axis: OverflowAxis::Horizontal,
          excess,
        });
      }
    }
    self.region_footnotes.push(head);
  }

  /// 現在リージョン（段）を確定する。下端揃えの配分と脚注の確定座標化は `draft` が行う
  ///
  /// 脚注は `flush` を問わずリージョンが閉じるたびに確定する（強制改ページ・最終ページでも落とさない）。
  /// `region_footnotes` は [`pack_footnotes`] の決めた行数へ切り詰め済み。
  fn end_region(&mut self, geom: &PageGeometry, flush: bool) {
    let footnotes = std::mem::take(&mut self.region_footnotes);
    let had_footnotes = !footnotes.is_empty();
    let column_x = self.column_offset();
    let region_limit = self.cursor.region_limit(geom);
    self.draft.close_region(geom, column_x, region_limit, flush, footnotes);
    if had_footnotes {
      self.cursor.footnote_reserved = Length::ZERO;
    }
  }
}

/// ブロック列をページへ配置し、確定ページ列とはみ出し記録を返す。
///
/// はみ出しは着地が確定した行（脚注本体を含む）・画像・ディスプレイ数式について、段の幅と実効の本文下限を基準に
/// 軸ごとに記録する。表は幅（表 1 個につき 1 件）と、行ごとの横（折り返すセルの行の超過）・縦（空の領域でも
/// 入らない行帯）を測る。
/// 表の行の脚注は本文の行と同じく `FootnoteLine` を記録する。
#[must_use]
pub(crate) fn break_pages(
  blocks: Vec<Block>,
  text_width: Length,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
) -> (Vec<Page>, Vec<Overflow>) {
  let col_width = column_width(text_width, geom.num_columns, geom.column_gap);
  let mut composer = PageComposer::new(geom, col_width);
  let block_count = blocks.len();
  let mut blocks = blocks;

  // FORBID penalty で連結された keep グループの先頭で一度だけ、末尾ブロックの先頭が見出しと同じ
  // リージョンに乗るかを判定し、収まらなければグループごと次リージョンへ送る
  let mut i = 0;
  let mut gated_end: Option<usize> = None;
  while i < blocks.len() {
    if gated_end.is_none_or(|e| return i > e)
      && is_content_block(&blocks[i])
      && let Some(end) = keep_group_end(&blocks, i)
    {
      if keep_group_orphaned(composer.cursor, !composer.carry.is_empty(), geom, breaker, col_width, &blocks[i..=end])
        && !composer.cursor.at_region_top(geom)
      {
        composer.advance_region(geom);
      }
      gated_end = Some(end);
    }
    let block = std::mem::replace(
      &mut blocks[i],
      Block::Glue {
        natural: Length::ZERO,
        stretch: Length::ZERO,
      },
    );
    match block {
      Block::Paragraph {
        origin,
        items,
        leading,
        indent,
        right_indent,
        alignment,
      } => {
        place_paragraph(
          &mut composer,
          geom,
          breaker,
          &items,
          leading,
          col_width,
          indent,
          right_indent,
          alignment,
          origin,
        );
      },
      Block::ComposedLine {
        origin,
        line,
        leading,
      } => {
        place_single_line(&mut composer, geom, line, leading, origin);
      },
      // アキは cursor.at_edge を変えない
      Block::Glue { natural, stretch } => {
        composer.draft.pass_stretch(stretch);
        composer.cursor.y += natural;
      },
      // 分割禁止は keep グループの連結マーカーで、ゲート（keep_group_*）が処理済み
      Block::Penalty { value } => match value {
        PENALTY_FORCE_BREAK => composer.force_new_page(geom),
        PENALTY_FORBID_BREAK => {},
        _ => unreachable!(
          "有限値の penalty はどの構築元も作らない（boxing の PageBreak / KeepWithNext と pagination::index は \
           強制改ページか分割禁止だけを積む）"
        ),
      },
      Block::Image {
        origin,
        path,
        width,
        height,
        target_dpi,
        align,
      } => {
        if composer.cursor.overflows(height, geom) {
          composer.advance_region(geom);
        }
        let col_off = composer.column_offset();
        let y = composer.cursor.y;
        let limit = composer.cursor.region_limit(geom);
        composer.record_extent_overflow(align.offset(col_width, width) + width, y + height, limit, |axis, excess| {
          return OverflowKind::Image {
            origin,
            path: path.clone(),
            axis,
            excess,
          };
        });
        composer.draft.place_block(
          PlacedBlock::Image {
            path,
            x: col_off + align.offset(col_width, width),
            y,
            width,
            height,
            target_dpi,
          },
          col_off,
          y,
        );
        composer.cursor = composer.cursor.below(height);
      },
      Block::Table { table, align } => {
        place_table(&mut composer, geom, breaker, table, col_width, align);
        composer.cursor.at_edge = true;
      },
      Block::Math {
        origin,
        body,
        numbers,
        numbers_on_right,
        align,
      } => {
        place_math_block(&mut composer, geom, body, numbers, numbers_on_right, align, col_width, origin);
      },
      // アンカーはゼロサイズで、次の実ブロックの確定座標で解決する
      Block::Anchor(id) => {
        composer.draft.defer_anchor(id);
      },
    }
    i += 1;
  }

  let (pages, overflows) = composer.finish(geom);
  debug!(block_count, page_count = pages.len(), "ページを分割");
  return (pages, overflows);
}

/// widow/orphan 制御でまとめて送る最小行数。
const MIN_LINES_AT_BREAK: usize = 2;

/// 内容ブロック（実際の高さを占め、リージョン配置の対象になるブロック）か。
fn is_content_block(block: &Block) -> bool {
  return match block {
    Block::Paragraph { .. }
    | Block::Table { .. }
    | Block::Image { .. }
    | Block::ComposedLine { .. }
    | Block::Math { .. } => true,
    Block::Glue { .. } | Block::Penalty { .. } | Block::Anchor(_) => false,
  };
}

/// `blocks[start]` が keep-with-next グループの先頭なら、グループ末尾（最後の内容ブロック）の
/// index を返す（純粋・幾何非依存）。
fn keep_group_end(blocks: &[Block], start: usize) -> Option<usize> {
  if !is_content_block(&blocks[start]) {
    return None;
  }
  let mut end = start;
  loop {
    let mut saw_forbid = false;
    let mut next_content = None;
    let mut j = end + 1;
    while j < blocks.len() {
      match &blocks[j] {
        Block::Penalty { value } if *value == PENALTY_FORBID_BREAK => saw_forbid = true,
        Block::Penalty { value } if *value == PENALTY_FORCE_BREAK => break,
        b if is_content_block(b) => {
          next_content = Some(j);
          break;
        },
        _ => {},
      }
      j += 1;
    }
    match next_content {
      Some(k) if saw_forbid => end = k,
      _ => break,
    }
  }
  return if end > start { Some(end) } else { None };
}

/// 段落以外の内容ブロック 1 個の配置判定
struct AtomicPlacement {
  /// ブロックを次リージョンへ送るか
  starts_region: bool,
  /// 送らずに置いた場合の配置後カーソル
  cursor_after: RegionCursor,
}

/// 段落以外の内容ブロック（画像・数式・合成行・表）をカーソル `cursor` から置くときの判定（純粋関数）。
///
/// 表の脚注本体は `breaker` で段幅 `column_width` に行分割して判定に含める（`place_table` と同じ判定関数を通す）。
fn plan_atomic(
  block: &Block,
  cursor: RegionCursor,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  column_width: Length,
) -> AtomicPlacement {
  match block {
    Block::Image { height, .. } => {
      return AtomicPlacement {
        starts_region: cursor.overflows(*height, geom),
        cursor_after: cursor.below(*height),
      };
    },
    Block::Math { body, .. } => {
      let height = body.height + body.depth;
      return AtomicPlacement {
        starts_region: cursor.defers_unbreakable(height, geom),
        cursor_after: cursor.below(height),
      };
    },
    Block::ComposedLine { line, leading, .. } => {
      return AtomicPlacement {
        starts_region: cursor.line_overflows(line, geom),
        cursor_after: cursor.after_line(cursor.line_baseline(line), *leading),
      };
    },
    Block::Table { table, .. } => {
      let total = table.height();
      // 分割可能な表は先頭行（ヘッダがあればヘッダの 1 行目）を送るかだけで決まる（`place_table` の行ループと
      // 同じ判定）。先頭行はヘッダ行かヘッダの無い表の本体行なので、再掲ヘッダは積まれない
      let starts_region = if table.breakable {
        let first = table.head.first().or_else(|| return table.rows.first());
        first.is_some_and(|row| {
          let demands = footnote_demands(&[pending_footnotes(breaker, &row.footnotes, column_width)]);
          return defers_table_row(cursor, row.height(), &demands[0], Length::ZERO, geom);
        })
      } else {
        let demands: Vec<FootnoteDemand> = footnote_demands(&table_footnote_bodies(breaker, &table.head, column_width))
          .into_iter()
          .chain(footnote_demands(&table_footnote_bodies(breaker, &table.rows, column_width)))
          .flatten()
          .collect();
        defers_unbreakable_table(cursor, total, &demands, geom)
      };
      return AtomicPlacement {
        starts_region,
        cursor_after: cursor.below(total),
      };
    },
    Block::Paragraph { .. } => unreachable!("段落は keep_group_orphaned が plan_paragraph_lines で扱う"),
    Block::Glue { .. } | Block::Penalty { .. } | Block::Anchor(_) => {
      unreachable!("内容ブロック以外は呼び出し側の is_content_block ガードが弾く")
    },
  }
}

/// keep グループを現在のカーソルから配置したとき、末尾の内容ブロックの先頭が見出しと別リージョンに
/// 落ちる（= 見出しが孤立する）かを返す純粋関数。リージョン改は行わず、孤立するなら `true`。
fn keep_group_orphaned(
  mut cursor: RegionCursor,
  mut carry_pending: bool,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  column_width: Length,
  group: &[Block],
) -> bool {
  let Some(last_content) = group.iter().rposition(is_content_block) else {
    return false;
  };
  let charges = FootnoteCharges::of(geom);
  for (gi, block) in group.iter().enumerate() {
    match block {
      Block::Glue { natural, .. } => {
        cursor.y += *natural;
      },
      Block::Paragraph {
        items,
        leading,
        indent,
        right_indent,
        alignment,
        ..
      } => {
        let lines = break_paragraph(breaker, items, column_width, *indent, *right_indent, *alignment);
        let demands = footnote_demands(&footnote_bodies(breaker, &lines, column_width));
        let (plan, truncated) = plan_paragraph_lines(
          &lines,
          cursor,
          *leading,
          geom.margin_top,
          geom.page_limit,
          &demands,
          charges,
          true,
          carry_pending,
        );
        if gi == last_content {
          // 本文は先頭行が見出しと同じリージョンに乗るかだけを見る。先頭リージョンに最小行数を残せない
          // ときの送り（orphan）と短い段落を丸ごと送る widow 補正は、計画の `starts_region` に反映済み。
          // 計画が空（繰越で先頭行すら入らない）なら実配置は改リージョンする
          return !lines.is_empty() && plan.first().is_none_or(|placement| return placement.starts_region);
        }
        // 見出しは全行が同じリージョンに乗るか。打ち切られて残った行は実配置では次リージョンへ送られる
        if plan.len() < lines.len() || plan.iter().any(|placement| return placement.starts_region) {
          return true;
        }
        cursor = match plan.last() {
          Some(last) => RegionCursor {
            footnote_reserved: last.reserved_after,
            ..cursor
          }
          .after_line(last.baseline, *leading),
          None => cursor.after_line(cursor.y, *leading),
        };
        // 最終行で脚注が分割された（打ち切られたが行は残っていない）なら、以降は繰越を抱えて計画する
        carry_pending |= truncated;
      },
      _ if is_content_block(block) => {
        let placement = plan_atomic(block, cursor, geom, breaker, column_width);
        if gi == last_content || placement.starts_region {
          return placement.starts_region;
        }
        cursor = placement.cursor_after;
      },
      _ => {},
    }
  }
  return false;
}

/// 段落を段幅・インデント・揃えに従って行に割り、左インデントまで加えた行列を返す。
fn break_paragraph(
  breaker: &dyn LineBreaker,
  items: &[HItem],
  column_width: Length,
  indent: Length,
  right_indent: Length,
  alignment: TextAlignment,
) -> Vec<Line> {
  let available = (column_width - indent - right_indent).max(Length::ZERO);
  let mut lines = breaker.break_lines(items, available, alignment);
  // 揃えのずらしは行分割が利用可能幅の中で済ませている。段オフセットは段をまたぐと行ごとに変わるため、
  // ここでは足さず着地段が決まってから足す
  for line in &mut lines {
    line.shift_x(indent);
  }
  return lines;
}

/// 脚注を段幅で行分割し、未確定の脚注にする（`footnotes` と同順・同長）。本文の行と表の行が共用する
fn pending_footnotes(
  breaker: &dyn LineBreaker,
  footnotes: &[MeasuredFootnote],
  column_width: Length,
) -> Vec<PendingFootnote> {
  return footnotes
    .iter()
    .map(|footnote| {
      return PendingFootnote {
        origin: footnote.origin,
        number: footnote.number,
        index: footnote.index,
        continued: false,
        lines: breaker.break_lines(&footnote.items, column_width, TextAlignment::Left),
        leading: footnote.leading,
      };
    })
    .collect();
}

/// 各行に付いた脚注（`line.footnotes`）を段幅で行分割し、未確定の脚注にする（`lines` と同順・同長）
fn footnote_bodies(breaker: &dyn LineBreaker, lines: &[Line], column_width: Length) -> Vec<Vec<PendingFootnote>> {
  return lines.iter().map(|line| return pending_footnotes(breaker, &line.footnotes, column_width)).collect();
}

/// 表の各行断片に付いた脚注を段幅で行分割し、未確定の脚注にする（`rows` と同順・同長）
fn table_footnote_bodies(
  breaker: &dyn LineBreaker,
  rows: &[TableRowFragment],
  column_width: Length,
) -> Vec<Vec<PendingFootnote>> {
  return rows.iter().map(|row| return pending_footnotes(breaker, &row.footnotes, column_width)).collect();
}

/// 行ごとの未確定脚注から、分割可能な需要（行ごとの積み上げ高さ）を作る（`bodies` と同順・同長）
fn footnote_demands(bodies: &[Vec<PendingFootnote>]) -> Vec<Vec<FootnoteDemand>> {
  return bodies
    .iter()
    .map(|line_bodies| {
      return line_bodies
        .iter()
        .map(|pending| return FootnoteDemand::new(&pending.lines, pending.leading))
        .collect();
    })
    .collect();
}

/// 段落を行に割ってベースライン送りで配置する
#[expect(
  clippy::too_many_arguments,
  reason = "ページ構成器・幾何・スタイル・脚注状態を 1 回の配置で同時に要するため、束ねる中間型を作っても呼び出し側の情報量は減らない"
)]
fn place_paragraph(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  items: &[HItem],
  leading: Length,
  column_width: Length,
  indent: Length,
  right_indent: Length,
  alignment: TextAlignment,
  origin: Option<NodeId>,
) {
  let mut lines = break_paragraph(breaker, items, column_width, indent, right_indent, alignment);
  // 脚注の行分割と需要は widow/orphan の再フローや chunk の再計画で再計算しない
  // （脚注の構成は改リージョン点の選び方で変わらないため）
  let charges = FootnoteCharges::of(geom);
  let mut bodies = footnote_bodies(breaker, &lines, column_width);
  let mut demands = footnote_demands(&bodies);
  // 段落を前から chunk 単位で確定する。繰越が生じない段落ではループは 1 周で終わる
  let mut last_baseline = composer.cursor.y;
  let mut is_paragraph_start = true;
  while !lines.is_empty() {
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      composer.cursor,
      leading,
      geom.margin_top,
      geom.page_limit,
      &demands,
      charges,
      is_paragraph_start,
      !composer.carry.is_empty(),
    );
    let chunk_len = plan.len();
    if chunk_len == 0 {
      // 繰越がこのリージョンを埋め尽くして先頭行すら入らない（＝本文 0 行のリージョン）。
      // 改リージョンして繰越を 1 リージョンぶん減らしてから計画し直す。`pack_footnotes` が
      // 繰越を必ず 1 行以上進めるので、この再試行は有限回で終わる
      composer.advance_region(geom);
      continue;
    }
    let chunk_lines: Vec<Line> = lines.drain(..chunk_len).collect();
    let chunk_bodies: Vec<Vec<PendingFootnote>> = bodies.drain(..chunk_len).collect();
    demands.drain(..chunk_len);
    for (mut line, footnotes, placement) in izip!(chunk_lines, chunk_bodies, plan) {
      if placement.starts_region {
        composer.advance_region(geom);
      }
      if placement.overflowed {
        composer.record(OverflowKind::FootnoteLine {
          footnotes: footnotes.iter().map(|footnote| return (footnote.number, footnote.origin)).collect(),
        });
      }
      let baseline = placement.baseline;
      last_baseline = baseline;
      // 縦の下限はこの行自身の脚注予約を足す前のもの（行自身の脚注のはみ出しは `FootnoteLine` が記録する）
      let limit = composer.cursor.region_limit(geom);
      composer.record_line_overflow(&line, baseline + line.depth, limit, origin);
      let col_off = composer.column_offset();
      line.shift_x(col_off);
      for (footnote, &placed) in footnotes.into_iter().zip(&placement.own_splits) {
        let (head, tail) = split_pending(footnote, placed);
        if let Some(head) = head {
          composer.push_region_footnote(head);
        }
        if let Some(tail) = tail {
          composer.carry.push(tail);
        }
      }
      composer.cursor.footnote_reserved = placement.reserved_after;
      // 行の着地が確定した。未解決アンカー（段落先頭でだけ非空）はこの行の上端で解決される
      composer.draft.place_line(line, baseline, col_off);
    }
    is_paragraph_start = false;
    // 続きが無い（段落末尾の行で分割した）場合の繰越は、次のブロックの改リージョンか
    // [`PageComposer::finish`] が拾う
    if truncated && !lines.is_empty() {
      composer.advance_region(geom);
    }
  }
  composer.cursor = composer.cursor.after_line(last_baseline, leading);
}

/// 合成済みの単一行（[`Block::ComposedLine`]）を 1 行として配置する
fn place_single_line(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  mut line: Line,
  leading: Length,
  origin: Option<NodeId>,
) {
  let mut baseline = composer.cursor.line_baseline(&line);
  if composer.cursor.line_overflows(&line, geom) {
    composer.advance_region(geom);
    baseline = geom.margin_top;
  }
  let limit = composer.cursor.region_limit(geom);
  composer.record_line_overflow(&line, baseline + line.depth, limit, origin);
  let col_off = composer.column_offset();
  line.shift_x(col_off);
  composer.draft.place_line(line, baseline, col_off);
  composer.cursor = composer.cursor.after_line(baseline, leading);
}

/// ディスプレイ数式ブロックを配置する
#[expect(
  clippy::too_many_arguments,
  reason = "数式本体・行番号・揃えの各値は Block::Math から分解した 1 ブロック分で、束ねる中間型を作っても呼び出し側の情報量は減らない"
)]
fn place_math_block(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  body: HBox,
  numbers: Vec<MathRowNumber>,
  numbers_on_right: bool,
  align: Align,
  column_width: Length,
  origin: NodeId,
) {
  let total_height = body.height + body.depth;
  if composer.cursor.defers_unbreakable(total_height, geom) {
    composer.advance_region(geom);
  }
  let col_off = composer.column_offset();
  let top = composer.cursor.y;
  let limit = composer.cursor.region_limit(geom);
  let body_offset = align.offset(column_width, body.width);
  composer.record_extent_overflow(body_offset + body.width, top + total_height, limit, |axis, excess| {
    return OverflowKind::Math {
      origin,
      excerpt: observe::summarize_box(&body),
      axis,
      excess,
    };
  });

  let x = col_off + body_offset;
  let baseline_y = top + body.height;
  let placed_numbers: Vec<PlacedMathNumber> = numbers
    .into_iter()
    .map(|n| {
      let number_x = col_off
        + if numbers_on_right {
          (column_width - n.content.width).max(Length::ZERO)
        } else {
          Length::ZERO
        };
      return PlacedMathNumber {
        content: n.content,
        x: number_x,
        baseline_y: baseline_y - n.dy,
      };
    })
    .collect();

  // 未解決アンカーは数式の上端（`top`）で解決する（block の baseline ではない）
  composer.draft.place_block(
    PlacedBlock::MathBlock {
      body,
      x,
      baseline_y,
      numbers: placed_numbers,
    },
    col_off,
    top,
  );
  composer.cursor = composer.cursor.below(total_height);
}

/// 表の行 1 本（行帯の高さ `row_height`・脚注の需要 `demands`）をカーソルから置くとき、行と脚注を合わせて
/// リージョンへどう収めるか。
///
/// 実配置（[`place_table_row`]）と keep-with-next の計画（[`plan_atomic`]）が共用する唯一の判定。行を本文の 1 行と同じ
/// 単位として扱い、段落の行と同じ [`fit_line_footnotes`] を通す。脚注の無い行では `Full` ⇔
/// `!cursor.overflows(row_height, geom)`。
fn fit_table_row(
  cursor: RegionCursor,
  row_height: Length,
  demands: &[FootnoteDemand],
  geom: &PageGeometry,
) -> LineFootnoteFit {
  return fit_line_footnotes(
    demands,
    cursor.footnote_reserved,
    cursor.y + row_height,
    geom.page_limit,
    FootnoteCharges::of(geom),
  );
}

/// 分割可能な表の行 1 本（行帯の高さ `row_height`・脚注の需要 `demands`）を次リージョンへ送るべきか。
///
/// 現在リージョンでは行と脚注が収まらず、予約の無い新しいリージョンの再掲ヘッダ（高さ `head_height`）の下なら
/// 収まるときだけ送る（新しいリージョンでも収まらない行は送っても改善せず、送ると前のリージョンにヘッダだけが残る
/// ので、その場に置く）。判定は [`fit_table_row`] だけで行い、実配置（[`place_table_row`]）と keep-with-next の計画
/// （[`plan_atomic`]）が共用する。
fn defers_table_row(
  cursor: RegionCursor,
  row_height: Length,
  demands: &[FootnoteDemand],
  head_height: Length,
  geom: &PageGeometry,
) -> bool {
  let fresh = RegionCursor {
    y: geom.margin_top + head_height,
    footnote_reserved: Length::ZERO,
    ..cursor
  };
  return matches!(fit_table_row(cursor, row_height, demands, geom), LineFootnoteFit::Rejected)
    && !matches!(fit_table_row(fresh, row_height, demands, geom), LineFootnoteFit::Rejected);
}

/// 分割禁止の表（高さ `table_height`、全行の脚注の需要を出現順に並べた `demands`）を次リージョンへ送るべきか。
///
/// 現在リージョンでは今の予約に全行の脚注を積んだ脚注エリアと合わせて収まらず、予約の無い新しいリージョンなら
/// 全行の脚注と合わせて収まるときだけ送る（足さないと、脚注なしでは収まる表が行ループの送りで途中から割れる。
/// 新しいリージョンでも収まらない表は送っても改善しないので、その場に置く）。実配置（[`place_table`]）と
/// keep-with-next の計画（[`plan_atomic`]）が共用する。
fn defers_unbreakable_table(
  cursor: RegionCursor,
  table_height: Length,
  demands: &[FootnoteDemand],
  geom: &PageGeometry,
) -> bool {
  let charges = FootnoteCharges::of(geom);
  let here = footnote_area_full(demands, cursor.footnote_reserved, charges);
  let fresh = footnote_area_full(demands, Length::ZERO, charges);
  return cursor.y + table_height > geom.page_limit - here && geom.margin_top + table_height <= geom.page_limit - fresh;
}

/// 表を行断片の単位で配置する（改段・改ページ時は先頭に再掲ヘッダを積む）
///
/// 表固有の判断は分割してよいか（`breakable`）と再掲ヘッダを積むことの 2 つだけで、断片の中身（列幅・セル余白・
/// 罫線・再掲ヘッダの付随情報）は表エンジンが確定済み。ここで足すのは段オフセットと揃えオフセットと、行の脚注の
/// 予約・配置（[`place_table_row`]）。セルの脚注本体は `breaker` で段幅 `column_width` に行分割する。
/// 未解決アンカーは空でない最初の断片の着地（[`PageDraft::place_table_fragment`]）で解決する。
/// 先頭行の fit 判定より前に解決すると、先頭行が次リージョンへ送られたときアンカーが前リージョンに残る。
fn place_table(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  table: TableBox,
  column_width: Length,
  align: Align,
) {
  // 揃えオフセットは段内のもの。段オフセットは着地段が決まる flush 時に断片ごとに足す
  let align_offset = align.offset(column_width, table.width);
  let head_bodies = table_footnote_bodies(breaker, &table.head, column_width);
  let row_bodies = table_footnote_bodies(breaker, &table.rows, column_width);
  if !table.breakable {
    let demands: Vec<FootnoteDemand> =
      footnote_demands(&head_bodies).into_iter().chain(footnote_demands(&row_bodies)).flatten().collect();
    if defers_unbreakable_table(composer.cursor, table.height(), &demands, geom) {
      composer.advance_region(geom);
    }
  }

  let TableBox {
    origin,
    width,
    head,
    repeated_head,
    rows,
    ..
  } = table;
  // 幅のはみ出しは表 1 個につき 1 件で、最初の断片が着地して `page_index` が着地ページになってから記録する
  // （断片が 1 つも無い表は呼び出し時点のページ）
  let mut width_excess = excess(width, column_width);
  let mut record_width_excess = |composer: &mut PageComposer| {
    if let Some(excess) = width_excess.take() {
      composer.record(OverflowKind::Table { origin, excess });
    }
  };
  let mut pending: Vec<(TableRowFragment, Length)> = Vec::new();
  for (row, bodies) in head.into_iter().zip(head_bodies) {
    place_table_row(composer, geom, row, bodies, &[], &mut pending, align_offset);
    record_width_excess(composer);
  }
  for (row, bodies) in rows.into_iter().zip(row_bodies) {
    place_table_row(composer, geom, row, bodies, &repeated_head, &mut pending, align_offset);
    record_width_excess(composer);
  }
  record_width_excess(composer);
  flush_table_rows(composer, &mut pending, align_offset);
}

/// 溜めた行断片を現在の段へ着地させる（改リージョンの前に呼ぶので段オフセットは着地段のもの）
fn flush_table_rows(composer: &mut PageComposer, pending: &mut Vec<(TableRowFragment, Length)>, align_offset: Length) {
  let column_x = composer.column_offset();
  composer.draft.place_table_fragment(std::mem::take(pending), column_x, align_offset);
}

/// 表の行 1 本を、その脚注（行分割済みの `bodies`）と合わせて現在リージョンへ置く
///
/// 送るとき（[`defers_table_row`]）は、溜めた断片を着地させて改リージョンし、`repeated_head`（ヘッダ行では空）の
/// 下に置くとして判定し直す。新リージョンの脚注エリアが繰越で埋まっていて入らないときは、ヘッダを積まずにさらに送る
/// （ヘッダだけのリージョンを作って繰越の脚注と重ねない。繰越は送るたびに 1 行以上減り、予約の無いリージョンなら
/// 入ることを送る判定が保証するので有限回で止まる）。予約の無い新リージョンでも入らない行は送らず、本文の行と同じく
/// はみ出しを記録してその場に置く。
/// 脚注の予約・分割・繰越は段落の行の確定（`place_paragraph`）と同じ順で行う。
fn place_table_row(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  row: TableRowFragment,
  bodies: Vec<PendingFootnote>,
  repeated_head: &[TableRowFragment],
  pending: &mut Vec<(TableRowFragment, Length)>,
  align_offset: Length,
) {
  let charges = FootnoteCharges::of(geom);
  let demands: Vec<FootnoteDemand> =
    bodies.iter().map(|pending| return FootnoteDemand::new(&pending.lines, pending.leading)).collect();
  let height = row.height();
  let head_height: Length = repeated_head.iter().map(TableRowFragment::height).sum();
  let mut fit = fit_table_row(composer.cursor, height, &demands, geom);
  if defers_table_row(composer.cursor, height, &demands, head_height, geom) {
    loop {
      flush_table_rows(composer, pending, align_offset);
      composer.advance_region(geom);
      let carried = composer.cursor.footnote_reserved > Length::ZERO;
      let below_head = RegionCursor {
        y: composer.cursor.y + head_height,
        ..composer.cursor
      };
      fit = fit_table_row(below_head, height, &demands, geom);
      if !(carried && matches!(fit, LineFootnoteFit::Rejected)) {
        break;
      }
    }
    for head_row in repeated_head {
      pending.push((head_row.clone(), composer.cursor.y));
      composer.cursor.y += head_row.height();
    }
  }
  let (area, splits) = match fit {
    LineFootnoteFit::Full(area) => (area, demands.iter().map(FootnoteDemand::line_count).collect()),
    LineFootnoteFit::Split(area, splits) => (area, splits),
    // 予約の無いリージョンでも収まらない病的ケース。送っても改善しないので、はみ出したまま置く
    LineFootnoteFit::Rejected => {
      if !bodies.is_empty() {
        composer.record(OverflowKind::FootnoteLine {
          footnotes: bodies.iter().map(|footnote| return (footnote.number, footnote.origin)).collect(),
        });
      }
      (
        footnote_area_full(&demands, composer.cursor.footnote_reserved, charges),
        demands.iter().map(FootnoteDemand::line_count).collect::<Vec<_>>(),
      )
    },
  };
  // 縦は行自身の脚注予約を足す前の下限で測る（本文の行と同じ。脚注の溢れは `FootnoteLine` が記録する）
  let top = composer.cursor.y;
  let limit = composer.cursor.region_limit(geom);
  let (origin, overrun) = (row.origin, row.overrun);
  pending.push((row, top));
  composer.cursor.y += height;
  let row_overflows = [
    (OverflowAxis::Horizontal, excess(overrun, Length::ZERO)),
    (OverflowAxis::Vertical, excess(top + height, limit)),
  ];
  for (axis, over) in row_overflows {
    if let Some(excess) = over {
      composer.record(OverflowKind::TableRow {
        origin,
        axis,
        excess,
      });
    }
  }
  for (footnote, placed) in bodies.into_iter().zip(splits) {
    let (head, tail) = split_pending(footnote, placed);
    if let Some(head) = head {
      composer.push_region_footnote(head);
    }
    if let Some(tail) = tail {
      composer.carry.push(tail);
    }
  }
  composer.cursor.footnote_reserved = area;
}

#[cfg(test)]
mod tests {
  use super::{
    FootnoteCharges, FootnoteDemand, Overflow, OverflowAxis, OverflowKind, PageGeometry, RegionCursor, break_pages,
    is_content_block, keep_group_end, pack_footnotes, page_draft::placed_block_bottom,
  };
  use crate::{
    document::{NodeId, TextAlignment},
    length::Length,
    project::{FontType, ProjectPath},
    publication::GlyphRun,
    semantics::{HeadingKey, LabelId},
    source::SourceId,
    typeset::{
      boxes::{
        Align, AnchorId, Block, FootnoteId, HBox, HBoxContent, HItem, IndexTerm, Line, LineLink, LinkTarget,
        MeasuredFootnote, PENALTY_FORBID_BREAK, Page, PlacedBlock, PlacedFootnote, PlacedHBox, PlacedLink,
        PlacedTableRow, PlacedTableRule, TableBox, TableRowFragment,
      },
      breaking::break_lines::GreedyBreaker,
    },
  };

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// pt 値の `Vec` を `Length` の `Vec` に変換する短縮子
  fn pts(values: &[f32]) -> Vec<Length> { return values.iter().map(|v| return Length::pt(*v)).collect(); }

  /// `Length` が pt 値 `expected` に（sp 丸め精度内で）一致するか
  fn close(actual: Length, expected: f32) -> bool { return (actual.to_pt() - expected).abs() < 1e-3; }

  /// keep-with-next マーカー（分割禁止 penalty）を作るテストヘルパ
  fn forbid_break() -> Block {
    return Block::Penalty {
      value: PENALTY_FORBID_BREAK,
    };
  }

  /// ページごとの本文行数を採取するヘルパ
  fn line_counts(pages: &[Page]) -> Vec<usize> {
    return pages.iter().map(|p| return page_baselines(p).len()).collect();
  }

  /// テスト用ジオメトリ（`margin_top=10`, `page_limit=50`、単段）
  fn test_geometry() -> PageGeometry {
    return PageGeometry {
      content_origin_x: Length::ZERO,
      margin_top: Length::pt(10.0),
      page_limit: Length::pt(50.0),
      num_columns: 1,
      column_gap: Length::pt(0.0),
      flush_bottom: false,
      footnote_top_margin: Length::ZERO,
      footnote_rule_length: Length::ZERO,
      footnote_rule_thickness: Length::ZERO,
      footnote_rule_color: None,
      footnote_rule_gap: Length::pt(4.0),
      background_color: None,
    };
  }

  /// テスト用 2 段ジオメトリ（`num_columns=2`, `column_gap=10`）。
  /// `text_width=100` と組むと段幅 45・段オフセット(右段) 55 になる。
  fn two_column_geometry() -> PageGeometry {
    return PageGeometry {
      num_columns: 2,
      column_gap: Length::pt(10.0),
      ..test_geometry()
    };
  }

  /// テストの段落が帰属する HIR ノードの ID
  fn paragraph_origin() -> NodeId { return NodeId::for_test(SourceId::new(0), 1); }

  /// テストの画像（[`image_block`]）が帰属する `\image` コマンドの ID
  fn image_origin() -> NodeId { return NodeId::for_test(SourceId::new(0), 2); }

  /// テストのディスプレイ数式（[`math_block`]）が帰属する数式環境の ID
  fn math_origin() -> NodeId { return NodeId::for_test(SourceId::new(0), 3); }

  /// [`footnote_item`] が作る `number` 番の脚注が帰属する `\footnote` コマンドの ID
  fn footnote_origin(number: u32) -> NodeId { return NodeId::for_test(SourceId::new(0), 100 + number); }

  /// テスト用ボックス（幅 10、高さ 8、深さ 2）
  fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: Length::pt(10.0),
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
    });
  }

  /// `n` 行（ForcedBreak 区切り）の合成段落を作る
  fn paragraph_of_lines(n: usize) -> Block {
    let mut items = Vec::new();
    for i in 0..n {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
    }
    return Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    };
  }

  /// 幅 0 の脚注マーカー（`HItem::Footnote`）を作るテストヘルパ
  fn footnote_item(number: u32, body: Vec<HItem>, leading: Length) -> HItem {
    return HItem::Footnote(MeasuredFootnote {
      origin: footnote_origin(number),
      number,
      index: number - 1,
      items: body,
      leading,
    });
  }

  /// ページの脚注のうち、[`footnote_item`] に渡した番号の脚注（`index = number - 1`）を返す
  fn footnote_numbered(page: &Page, number: u32) -> Option<&PlacedFootnote> {
    return page.footnotes.iter().find(|f| return f.index + 1 == number);
  }

  /// 1 行だけの段落（widow/orphan 補正の対象外）を作る。`items` の末尾に脚注マーカーを追加できる
  fn single_line_paragraph(mut items: Vec<HItem>) -> Block {
    items.insert(0, test_box());
    return Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    };
  }

  /// ページの脚注のうち、指定番号の本体行ベースライン列を返す
  fn footnote_baselines(page: &Page, number: u32) -> Vec<Length> {
    return footnote_numbered(page, number)
      .expect("指定番号の脚注があるはず")
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
  }

  /// 幅 0 の索引マーカー（`HItem::IndexMark`）を作るテストヘルパ
  fn index_mark_item(word: &str, reading: Option<&str>) -> HItem {
    return HItem::IndexMark(IndexTerm {
      word: word.to_string(),
      reading: reading.map(str::to_string),
    });
  }

  #[test]
  fn index_entries_collected_per_page_in_order() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("A", None)]),
      single_line_paragraph(vec![index_mark_item("B", None)]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].index_entries.len(), 2);
    assert_eq!(pages[0].index_entries[0].word, "A");
    assert_eq!(pages[0].index_entries[1].word, "B");
  }

  #[test]
  fn index_entries_different_reading_are_separate_entries() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("語", None)]),
      single_line_paragraph(vec![index_mark_item("語", Some("よみ"))]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].index_entries.len(), 2);
  }

  /// `lines` 行の脚注本体を作り、`at` 行目（0 起点）の行頭に索引マーカーを置く
  fn footnote_of_lines_with_index_at(number: u32, lines: usize, at: usize, word: &str) -> HItem {
    let mut items = Vec::new();
    for i in 0..lines {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      if i == at {
        items.push(index_mark_item(word, None));
      }
      items.push(test_box());
    }
    return footnote_item(number, items, pt(12.0));
  }

  #[test]
  fn index_entries_in_footnote_body_land_on_the_footnote_page() {
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![
      footnote_of_lines_with_index_at(1, 1, 0, "脚注語"),
    ])];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].index_entries.len(), 1, "{:?}", pages[0].index_entries);
    assert_eq!(pages[0].index_entries[0].word, "脚注語");
  }

  #[test]
  fn index_entries_in_carried_footnote_lines_land_on_the_carry_page() {
    // 4 行の脚注は 3 行目までが 1 ページ目、4 行目が繰越
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines_with_index_at(1, 4, 3, "繰越語")]),
      single_line_paragraph(vec![]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2);
    assert!(
      pages[0].index_entries.is_empty(),
      "マーカーの行は繰越されたので 1 ページ目には無い: {:?}",
      pages[0].index_entries
    );
    assert_eq!(pages[1].index_entries.len(), 1, "{:?}", pages[1].index_entries);
    assert_eq!(pages[1].index_entries[0].word, "繰越語");
  }

  /// `lines` 行の脚注本体を作り、`at` 行目（0 起点）の箱を外部リンクで挟む
  fn footnote_of_lines_with_link_at(number: u32, lines: usize, at: usize, uri: &str) -> HItem {
    let mut items = Vec::new();
    for i in 0..lines {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      if i == at {
        items.push(HItem::LinkStart(LinkTarget::External(uri.to_string())));
        items.push(test_box());
        items.push(HItem::LinkEnd);
      } else {
        items.push(test_box());
      }
    }
    return footnote_item(number, items, pt(12.0));
  }

  /// ページのリンクのうち、指定 URI の外部リンクだけを返す
  fn external_links<'a>(page: &'a Page, uri: &str) -> Vec<&'a PlacedLink> {
    return page
      .links
      .iter()
      .filter(|link| return matches!(&link.target, LinkTarget::External(u) if u == uri))
      .collect();
  }

  #[test]
  fn links_in_footnote_body_land_on_the_footnote_page() {
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_of_lines_with_link_at(
      1,
      1,
      0,
      "https://example.com",
    )])];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    // 本文中の脚注マーカーも内部リンクを作るので、外部リンクだけを取り出して見る
    assert_eq!(pages.len(), 1);
    let links = external_links(&pages[0], "https://example.com");
    assert_eq!(links.len(), 1, "{:?}", pages[0].links);
    let baseline = footnote_baselines(&pages[0], 1)[0];
    assert!(close(links[0].x, 0.0));
    assert!(close(links[0].y, baseline.to_pt() - 8.0), "行 box の上端に一致するはず: {:?}", links[0]);
    assert!(close(links[0].width, 10.0));
    assert!(close(links[0].height, 10.0));
  }

  #[test]
  fn links_in_carried_footnote_lines_land_on_the_carry_page() {
    // 4 行の脚注は 3 行目までが 1 ページ目、4 行目が繰越
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines_with_link_at(
        1,
        4,
        3,
        "https://example.com",
      )]),
      single_line_paragraph(vec![]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2);
    assert!(
      external_links(&pages[0], "https://example.com").is_empty(),
      "リンクの行は繰越されたので 1 ページ目には無い: {:?}",
      pages[0].links
    );
    let links = external_links(&pages[1], "https://example.com");
    assert_eq!(links.len(), 1, "{:?}", pages[1].links);
    let baseline = *footnote_baselines(&pages[1], 1).last().expect("繰越断片に行があるはず");
    assert!(close(links[0].y, baseline.to_pt() - 8.0), "繰越先の行 box の上端に一致するはず: {:?}", links[0]);
  }

  #[test]
  fn index_entries_in_table_body_cells_land_on_the_row_page() {
    // 5 行の表は 2 ページに分かれる
    let geom = test_geometry();
    let table = table_box(
      vec![table_row("HEAD")],
      (0..5).map(|i| return with_index(table_row(&format!("R{i}")), &format!("W{i}"))).collect(),
    );

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    assert_eq!(pages.len(), 2, "{pages:?}");
    let words: Vec<Vec<&str>> = pages
      .iter()
      .map(|page| return page.index_entries.iter().map(|e| return e.word.as_str()).collect())
      .collect();
    assert!(!words[0].is_empty() && !words[1].is_empty(), "両ページに行があるはず: {words:?}");
    let all: Vec<&str> = words.concat();
    assert_eq!(all, vec!["W0", "W1", "W2", "W3", "W4"], "行の順序どおりに 1 回ずつ: {words:?}");
  }

  #[test]
  fn index_entries_split_across_pages_are_not_merged() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("語", None)]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![index_mark_item("語", None)]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{:?}", line_counts(&pages));
    assert_eq!(pages[0].index_entries.len(), 1);
    assert_eq!(pages[1].index_entries.len(), 1);
  }

  #[test]
  fn index_mark_does_not_affect_page_layout() {
    let geom = test_geometry();
    let with_marks = vec![
      single_line_paragraph(vec![index_mark_item("A", None)]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![index_mark_item("B", Some("びー"))]),
      single_line_paragraph(vec![]),
    ];
    let without_marks = vec![
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
    ];

    let (with_pages, _) = break_pages(with_marks, Length::pt(100.0), &geom, &GreedyBreaker);
    let (without_pages, _) = break_pages(without_marks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&with_pages), line_counts(&without_pages));
    for (with_page, without_page) in with_pages.iter().zip(&without_pages) {
      assert_eq!(page_baselines(with_page), page_baselines(without_page));
    }
  }

  #[test]
  fn footnote_places_at_page_bottom_without_overlap() {
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_item(
      1,
      vec![test_box()],
      pt(12.0),
    )])];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    let body_baseline = page_baselines(&pages[0])[0];
    assert!(close(body_baseline, 10.0), "body baseline={}", body_baseline.to_pt());
    let footnote_baseline = footnote_baselines(&pages[0], 1)[0];
    assert!(footnote_baseline.to_pt() > body_baseline.to_pt() + 2.0, "脚注が本文より下にあるはず");
    assert!(footnote_baseline.to_pt() <= geom.page_limit.to_pt(), "脚注が page_limit を超えないはず");
  }

  #[test]
  fn line_with_overflowing_footnote_moves_to_next_page_with_it() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![footnote_item(
        2,
        vec![test_box(), HItem::ForcedBreak, test_box()],
        pt(12.0),
      )]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0, 34.0]));
    assert!(pages[0].footnotes.is_empty(), "1 ページ目に脚注が漏れてはいけない");
    let body_baseline = page_baselines(&pages[1])[0];
    assert!(close(body_baseline, 10.0), "2 ページ目先頭 baseline={}", body_baseline.to_pt());
    let footnote_lines = footnote_baselines(&pages[1], 2);
    assert_eq!(footnote_lines.len(), 2, "脚注本体は 2 行");
    assert!(footnote_lines[0].to_pt() > body_baseline.to_pt() + 2.0, "脚注が本文より下にあるはず");
    assert!(footnote_lines[1].to_pt() > footnote_lines[0].to_pt(), "脚注本体は出現順に積まれる");
    assert!(footnote_lines[1].to_pt() <= geom.page_limit.to_pt() + 1e-3, "脚注が page_limit を超えないはず");
  }

  #[test]
  fn multiple_footnotes_on_same_page_stack_in_order() {
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![
      footnote_item(1, vec![test_box()], pt(12.0)),
      footnote_item(2, vec![test_box()], pt(12.0)),
    ])];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].footnotes.len(), 2);
    assert_eq!(pages[0].footnotes[0].index, 0);
    assert_eq!(pages[0].footnotes[1].index, 1);
    let first = footnote_baselines(&pages[0], 1)[0];
    let second = footnote_baselines(&pages[0], 2)[0];
    assert!(second.to_pt() > first.to_pt(), "脚注 2 は脚注 1 より下");
  }

  /// `lines` 行の本体を持つ脚注マーカーを作る（各行は [`test_box`] 1 個 = 高さ 8・深さ 2）
  fn footnote_of_lines(number: u32, lines: usize) -> HItem {
    let mut items = Vec::new();
    for i in 0..lines {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
    }
    return footnote_item(number, items, pt(12.0));
  }

  /// ページ 1 枚の要約: 本文行数と、脚注ごとの ([`footnote_item`] に渡した番号, 繰越か, 本体行数)
  type PageFootnoteLayout = (usize, Vec<(u32, bool, usize)>);

  /// ページごとの本文行数・脚注構成の要約（分割の連鎖を読みやすく比較する）
  fn footnote_layout(pages: &[Page]) -> Vec<PageFootnoteLayout> {
    return pages
      .iter()
      .map(|page| {
        let footnotes = page
          .footnotes
          .iter()
          .map(|f| {
            let lines = f.blocks.iter().filter(|b| return matches!(b, PlacedBlock::Line { .. })).count();
            return (f.index + 1, f.continued, lines);
          })
          .collect();
        return (page_baselines(page).len(), footnotes);
      })
      .collect();
  }

  #[test]
  fn long_footnote_splits_and_carries_remainder_to_next_page() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(footnote_layout(&pages), vec![(1, vec![(1, false, 3)]), (1, vec![(1, true, 1)])], "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0]));
    assert_eq!(footnote_baselines(&pages[0], 1), pts(&[24.0, 36.0, 48.0]));
    let carried = footnote_baselines(&pages[1], 1)[0];
    assert!(carried.to_pt() > 12.0, "繰越が本文と重ならないはず: {}", carried.to_pt());
    assert!(carried.to_pt() + 2.0 <= geom.page_limit.to_pt() + 1e-3, "繰越が page_limit を超えないはず");
    let anchors_on: fn(&Page) -> usize = |page| {
      return page
        .anchors
        .iter()
        .filter(|a| return matches!(&a.id, AnchorId::Footnote(id) if *id == FootnoteId::new(0)))
        .count();
    };
    assert_eq!(anchors_on(&pages[0]), 1, "{:?}", pages[0].anchors);
    assert_eq!(anchors_on(&pages[1]), 0, "繰越側にアンカーは無いはず: {:?}", pages[1].anchors);
  }

  #[test]
  fn carried_footnote_stacks_before_own_footnote_of_the_page() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![footnote_of_lines(2, 1)]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(
      footnote_layout(&pages),
      vec![
        (1, vec![(1, false, 3)]),
        (1, vec![(1, true, 1), (2, false, 1)])
      ],
      "{pages:?}"
    );
    let carried = footnote_baselines(&pages[1], 1)[0];
    let own = footnote_baselines(&pages[1], 2)[0];
    assert!(own.to_pt() > carried.to_pt(), "自前の脚注 2 は繰越より下に積まれるはず");
  }

  #[test]
  fn long_footnote_carries_across_multiple_pages() {
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 10)]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(
      footnote_layout(&pages),
      vec![
        (1, vec![(1, false, 3)]),
        (1, vec![(1, true, 3)]),
        (1, vec![(1, true, 3)]),
        (1, vec![(1, true, 1)]),
      ],
      "{pages:?}"
    );
    let total: usize =
      footnote_layout(&pages).iter().flat_map(|(_, f)| return f.clone()).map(|(_, _, n)| return n).sum();
    assert_eq!(total, 10, "脚注の行が欠落しないはず");
  }

  #[test]
  fn footnote_split_on_last_line_is_drained_at_document_end() {
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_of_lines(1, 4)])];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(footnote_layout(&pages), vec![(1, vec![(1, false, 3)]), (0, vec![(1, true, 1)])], "{pages:?}");
  }

  /// 版面が 14pt しかないページ（脚注 1 行 = 10pt + 固定費 4pt すら入らない）
  fn cramped_geometry() -> PageGeometry {
    return PageGeometry {
      page_limit: pt(24.0),
      ..test_geometry()
    };
  }

  /// 高さ `height` pt・深さ 2pt の箱（1 行がページ全高を超える脚注を作るため）
  fn tall_box(height: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(10.0),
      height: pt(height),
      depth: pt(2.0),
    });
  }

  #[test]
  fn footnotes_on_the_same_overflowing_line_are_reported_together() {
    let geom = cramped_geometry();
    let blocks = vec![single_line_paragraph(vec![
      footnote_of_lines(1, 1),
      footnote_of_lines(2, 1),
    ])];

    let (pages, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "はみ出しても配置は続くので 1 ページに収まる: {pages:?}");
    assert_eq!(
      overflows,
      vec![Overflow {
        page_index: 0,
        kind: OverflowKind::FootnoteLine {
          footnotes: vec![(1, footnote_origin(1)), (2, footnote_origin(2))]
        },
      }],
      "1 行のはみ出しは 1 件で、その行の脚注番号を出現順に並べるはず"
    );
  }

  #[test]
  fn each_overflowing_line_is_reported_once_in_page_order() {
    // 各行に脚注が付き、widow/orphan 補正で計画が立て直される長さの段落
    let geom = cramped_geometry();
    let mut items = Vec::new();
    for number in 1..=3u32 {
      if number > 1 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
      items.push(footnote_of_lines(number, 1));
    }
    let blocks = vec![Block::Paragraph {
      origin: None,
      items,
      leading: pt(12.0),
      indent: Length::ZERO,
      right_indent: Length::ZERO,
      alignment: TextAlignment::Left,
    }];

    let (_, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let recorded: Vec<(usize, Vec<u32>)> = overflows
      .iter()
      .map(|overflow| {
        let OverflowKind::FootnoteLine { footnotes } = &overflow.kind else {
          panic!("行のはみ出しとして記録されるはず: {overflow:?}");
        };
        return (overflow.page_index, footnotes.iter().map(|(number, _)| return *number).collect());
      })
      .collect();
    assert_eq!(
      recorded,
      vec![(0, vec![1]), (1, vec![2]), (2, vec![3])],
      "はみ出しは行ごとに 1 件ずつ、ページ昇順で並ぶはず"
    );
  }

  #[test]
  fn carried_footnote_line_overflow_is_reported_once_per_page_it_lands_on() {
    // 繰越の 2 行がどちらもページ全高を超える（1 ページに 1 行ずつ置かれる）
    let geom = test_geometry();
    let footnote = footnote_item(
      1,
      vec![
        test_box(),
        HItem::ForcedBreak,
        tall_box(40.0),
        HItem::ForcedBreak,
        tall_box(40.0),
      ],
      pt(12.0),
    );
    let blocks = vec![single_line_paragraph(vec![footnote])];

    let (pages, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 3, "繰越は次ページへ送られる: {pages:?}");
    assert_eq!(
      overflows,
      vec![
        Overflow {
          page_index: 1,
          kind: OverflowKind::FootnoteSingleLine {
            number: 1,
            origin: footnote_origin(1),
          },
        },
        Overflow {
          page_index: 2,
          kind: OverflowKind::FootnoteSingleLine {
            number: 1,
            origin: footnote_origin(1),
          },
        },
      ],
      "繰越が続く限りページごとに記録するはず"
    );
  }

  /// `lines` 行の段落を作り、`at` 行目（0 起点）の末尾に脚注マーカーを置く
  fn paragraph_with_footnote_at(lines: usize, at: usize, footnote: HItem) -> Block {
    let mut footnote = Some(footnote);
    let mut items = Vec::new();
    for i in 0..lines {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
      if i == at
        && let Some(marker) = footnote.take()
      {
        items.push(marker);
      }
    }
    return Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    };
  }

  /// ページごとの「本文の下端」と「脚注エリアの上端」を返す（重なり検証用。無ければ `None`）
  fn body_bottom_and_footnote_top(page: &Page) -> (Option<Length>, Option<Length>) {
    let body_bottom = page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, baseline_y } => return Some(*baseline_y + line.depth),
        PlacedBlock::Table { rows } => return rows.last().map(|row| return row.top_y + row.height),
        _ => return None,
      })
      .reduce(Length::max);
    let footnote_top = page
      .footnotes
      .iter()
      .flat_map(|f| return &f.blocks)
      .filter_map(|b| match b {
        PlacedBlock::Line { line, baseline_y } => return Some(*baseline_y - line.height),
        PlacedBlock::Rule { y, .. } => return Some(*y),
        _ => return None,
      })
      .reduce(Length::min);
    return (body_bottom, footnote_top);
  }

  /// 全ページで本文と脚注が重なっていないことを表明する
  fn assert_no_body_footnote_overlap(pages: &[Page]) {
    for (index, page) in pages.iter().enumerate() {
      let (Some(body_bottom), Some(footnote_top)) = body_bottom_and_footnote_top(page) else {
        continue;
      };
      assert!(
        footnote_top.to_pt() >= body_bottom.to_pt() - 1e-3,
        "page {index}: 本文の下端 {} と脚注の上端 {} が重なっている",
        body_bottom.to_pt(),
        footnote_top.to_pt()
      );
    }
  }

  #[test]
  fn footnote_splits_mid_paragraph_and_remaining_lines_reflow_after_the_carry() {
    let geom = test_geometry();
    let blocks = vec![paragraph_with_footnote_at(6, 1, footnote_of_lines(1, 4))];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(
      footnote_layout(&pages),
      vec![
        (2, vec![(1, false, 2)]),
        (2, vec![(1, true, 2)]),
        (2, vec![])
      ],
      "{pages:?}"
    );
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0]));
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0]), "繰越ページでも本文は実効下限まで流れる");
    assert_no_body_footnote_overlap(&pages);
  }

  #[test]
  fn footnote_split_coexists_with_flush_bottom() {
    let geom = flush_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::Glue {
        natural: Length::ZERO,
        stretch: pt(10.0),
      },
      paragraph_with_footnote_at(4, 0, footnote_of_lines(1, 4)),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let split_pages: Vec<usize> = pages
      .iter()
      .enumerate()
      .filter(|(_, p)| return p.footnotes.iter().any(|f| return f.continued))
      .map(|(i, _)| return i)
      .collect();
    assert!(!split_pages.is_empty(), "脚注が分割されるはず（空振り検知）: {:?}", footnote_layout(&pages));
    assert_no_body_footnote_overlap(&pages);
    for (index, page) in pages.iter().enumerate() {
      for block in page.footnotes.iter().flat_map(|f| return &f.blocks) {
        let bottom = placed_block_bottom(block);
        assert!(
          bottom.to_pt() <= geom.page_limit.to_pt() + 1e-3,
          "page {index}: 脚注が page_limit を超えている: {}",
          bottom.to_pt()
        );
      }
    }
  }

  /// 脚注 1 個ぶんの需要を作るテストヘルパ（各行は [`test_line`] = 高さ 8・深さ 2、行送り 12）
  fn demand_of_lines(count: usize) -> FootnoteDemand {
    return FootnoteDemand::new(&vec![test_line(); count], pt(12.0));
  }

  /// `rule_gap` だけを課金する脚注パラメータ（`test_geometry` と同じ）
  fn gap_only_charges() -> FootnoteCharges {
    return FootnoteCharges {
      rule_gap: pt(4.0),
      ..no_charges()
    };
  }

  #[test]
  fn pack_footnotes_fills_budget_with_as_many_lines_as_fit() {
    let demands = vec![demand_of_lines(4)];

    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), true).expect("先頭 1 行は入る");

    assert_eq!(packing.splits, vec![3]);
    assert!(close(packing.height, 38.0), "{}", packing.height.to_pt());
  }

  #[test]
  fn pack_footnotes_rejects_when_first_line_does_not_fit() {
    let demands = vec![demand_of_lines(2)];

    let packing = pack_footnotes(&demands, Length::ZERO, pt(13.0), gap_only_charges(), true);

    assert!(packing.is_none());
  }

  #[test]
  fn pack_footnotes_marks_overflow_when_the_carried_first_line_does_not_fit() {
    // 繰越（`require_first_line = false`）で、先頭脚注の 1 行ぶんも無い予算
    let demands = vec![demand_of_lines(2)];

    let packing =
      pack_footnotes(&demands, Length::ZERO, pt(5.0), gap_only_charges(), false).expect("繰越は必ず 1 行進める");

    assert!(packing.overflowed, "はみ出したまま置いた事実を返すはず");
    assert_eq!(packing.splits, vec![1], "停止条件として 1 行だけ進める");
  }

  #[test]
  fn pack_footnotes_does_not_mark_overflow_when_lines_fit() {
    let demands = vec![demand_of_lines(2)];

    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), false).expect("全行が入る");

    assert!(!packing.overflowed, "収まったときは記録しないはず");
  }

  #[test]
  fn pack_footnotes_reserves_a_first_line_for_later_footnotes_on_the_same_line() {
    let demands = vec![demand_of_lines(4), demand_of_lines(1)];

    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), true)
      .expect("予約すれば両方に先頭行が置ける");

    assert_eq!(packing.splits, vec![1, 1]);
    assert!(close(packing.height, 28.0), "{}", packing.height.to_pt());
  }

  /// 区切り罫線・`top_margin` を有効にしたテスト用ジオメトリ（他は `test_geometry` と同じ）
  fn geometry_with_footnote_rule() -> PageGeometry {
    return PageGeometry {
      footnote_top_margin: Length::pt(3.0),
      footnote_rule_length: Length::pt(20.0),
      footnote_rule_thickness: Length::pt(1.0),
      footnote_rule_color: Some([255, 0, 0]),
      footnote_rule_gap: Length::pt(2.0),
      ..test_geometry()
    };
  }

  /// ページ内の脚注エリアに描かれた区切り罫線（`PlacedBlock::Rule`）の総数
  fn footnote_rule_count(page: &Page) -> usize {
    return page
      .footnotes
      .iter()
      .flat_map(|f| return &f.blocks)
      .filter(|b| matches!(b, PlacedBlock::Rule { .. }))
      .count();
  }

  #[test]
  fn footnote_rule_drawn_once_before_first_footnote_in_region() {
    let geom = geometry_with_footnote_rule();
    let blocks = vec![single_line_paragraph(vec![
      footnote_item(1, vec![test_box()], pt(12.0)),
      footnote_item(2, vec![test_box()], pt(12.0)),
    ])];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert_eq!(footnote_rule_count(&pages[0]), 1, "罫線は 1 リージョンに 1 本だけのはず");
    let first_block = pages[0].footnotes[0].blocks.first().expect("脚注 1 は空でないはず");
    assert!(
      matches!(
        first_block,
        PlacedBlock::Rule { width, height, color, .. }
        if close(*width, 20.0) && close(*height, 1.0) && *color == Some([255, 0, 0])
      ),
      "{first_block:?}"
    );
  }

  #[test]
  fn break_pages_carries_background_color_from_geometry() {
    let geom = PageGeometry {
      background_color: Some([10, 20, 30]),
      ..test_geometry()
    };
    let blocks = vec![paragraph_of_lines(1)];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages[0].background_color, Some([10, 20, 30]));
  }

  #[test]
  fn footnote_rule_reservation_does_not_overlap_when_footnote_starts_new_region() {
    let geom = geometry_with_footnote_rule();
    let blocks = vec![
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![footnote_item(
        2,
        vec![test_box(), HItem::ForcedBreak, test_box()],
        pt(12.0),
      )]),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].footnotes.is_empty(), "1 ページ目に脚注が漏れてはいけない");
    assert_eq!(footnote_rule_count(&pages[1]), 1, "新リージョンにも罫線は 1 本だけのはず");
    let footnote_lines = footnote_baselines(&pages[1], 2);
    assert_eq!(footnote_lines.len(), 2, "脚注本体は 2 行");
    let last_line_bottom = footnote_lines[1] + pt(2.0); // test_box() の depth=2
    assert!(
      last_line_bottom.to_pt() <= geom.page_limit.to_pt() + 1e-3,
      "脚注の最終行が page_limit を超えないはず（見積りと配置の非対称バグの回帰）: {}",
      last_line_bottom.to_pt()
    );
  }

  /// 各ページの行ベースライン列を採取するヘルパ
  fn page_baselines(page: &Page) -> Vec<Length> {
    return page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
  }

  /// 高さ 8・深さ 2 の単純な行（純粋関数テスト用）
  fn test_line() -> Line {
    return Line {
      boxes: Vec::new(),
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }

  /// 課金ゼロの脚注パラメータ
  fn no_charges() -> FootnoteCharges {
    return FootnoteCharges {
      top_margin: Length::ZERO,
      rule_thickness: Length::ZERO,
      rule_gap: Length::ZERO,
    };
  }

  #[test]
  fn short_paragraph_moves_whole_rather_than_split() {
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(2), paragraph_of_lines(3)];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0]), "先行段落の 2 行だけ");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0, 34.0]), "3 行段落は分割せず丸ごと次ページ");
  }

  #[test]
  fn oversized_paragraph_builds_without_hang() {
    let geom = test_geometry();

    let (pages, _) = break_pages(vec![paragraph_of_lines(20)], Length::pt(100.0), &geom, &GreedyBreaker);

    let total: usize = pages.iter().map(|p| return page_baselines(p).len()).sum();
    assert_eq!(total, 20, "行が欠落しない: {pages:?}");
    assert!(pages.len() >= 5, "4 行/ページなので 5 ページ以上に分かれる: {}", pages.len());
  }

  #[test]
  fn vspace_shifts_following_baseline() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(5.0)),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let baselines: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
    assert_eq!(baselines, pts(&[10.0, 27.0]));
  }

  #[test]
  fn paragraph_after_image_clears_ascent() {
    let geom = test_geometry();
    let blocks = vec![
      Block::Image {
        origin: image_origin(),
        path: ProjectPath::new("x.png"),
        width: pt(20.0),
        height: pt(15.0),
        target_dpi: None,
        align: Align::Left,
      },
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let baseline = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(baseline, 33.0), "baseline={}", baseline.to_pt());
  }

  #[test]
  fn oversized_image_moves_to_next_page() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Image {
        origin: image_origin(),
        path: ProjectPath::new("x.png"),
        width: pt(20.0),
        height: pt(30.0),
        target_dpi: None,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Image { y, .. } = pages[1].blocks.first().expect("画像があるはず") else {
      panic!("Image を期待");
    };
    assert!(close(*y, 10.0), "画像はページ先頭 (margin_top) に置かれる");
  }

  /// テキスト入り（フォント 10）の 1 セル行を作るヘルパ
  /// テキスト `text`（フォント 10・幅 20・高さ 10）の箱を作るヘルパ
  fn table_text_box(text: &str) -> HBox {
    return HBox {
      content: HBoxContent::Glyphs(GlyphRun {
        font_size: pt(10.0),
        text: text.to_string(),
        glyphs: Vec::new(),
        font_type: FontType::Serif,
        color: None,
      }),
      width: Length::pt(20.0),
      height: Length::pt(10.0),
      depth: Length::pt(0.0),
    };
  }

  /// テストの表（[`table_box`]）が帰属する表環境の ID
  fn table_origin() -> NodeId { return NodeId::for_test(SourceId::new(0), 4); }

  /// テストの表の行（[`table_fragment`]）が帰属する `\row` の ID
  fn table_row_origin() -> NodeId { return NodeId::for_test(SourceId::new(0), 5); }

  /// 行帯の高さ `height`・ベースライン `baseline` で、箱 `hbox` を表左端 + 余白 2 に置いた 1 列表の行断片を作るヘルパ
  fn table_fragment(hbox: HBox, height: f32, baseline: f32) -> TableRowFragment {
    return TableRowFragment {
      row: PlacedTableRow {
        top_y: Length::ZERO,
        height: pt(height),
        baseline_y: pt(baseline),
        boxes: vec![PlacedHBox {
          hbox,
          dx: pt(2.0),
          dy: Length::ZERO,
        }],
        rule: None,
      },
      origin: table_row_origin(),
      overrun: Length::ZERO,
      links: Vec::new(),
      index_marks: Vec::new(),
      footnotes: Vec::new(),
    };
  }

  /// テキスト入り（フォント 10）の 1 セル行の断片を作るヘルパ（行帯 10・ベースライン 10 は既定フォントサイズ 10・
  /// 行高係数 1（`test_geometry` の本文と同じ）で表エンジンが組む行帯と同じ）
  fn table_row(text: &str) -> TableRowFragment { return table_fragment(table_text_box(text), 10.0, 10.0); }

  /// 1 列・表幅 24（内容 20 + 余白 2 × 2）の分割可能な表を作るヘルパ。再掲ヘッダは表エンジンと同じく
  /// `head` の複製から索引語を除いたもの
  fn table_box(head: Vec<TableRowFragment>, rows: Vec<TableRowFragment>) -> TableBox {
    let repeated_head = head
      .iter()
      .map(|fragment| {
        return TableRowFragment {
          index_marks: Vec::new(),
          footnotes: Vec::new(),
          ..fragment.clone()
        };
      })
      .collect();
    return TableBox {
      origin: table_origin(),
      width: pt(24.0),
      head,
      repeated_head,
      rows,
      breakable: true,
    };
  }

  /// 表の行断片に、`text` の箱（x 2..22）と行帯の全高を覆うリンク矩形を足すヘルパ
  fn with_link(mut row: TableRowFragment, target: LinkTarget) -> TableRowFragment {
    row.links.push(PlacedLink {
      target,
      x: pt(2.0),
      y: Length::ZERO,
      width: pt(20.0),
      height: row.row.height,
    });
    return row;
  }

  /// 表の行断片に索引語 `word`（読みなし）を足すヘルパ
  fn with_index(mut row: TableRowFragment, word: &str) -> TableRowFragment {
    row.index_marks.push(IndexTerm {
      word: word.to_string(),
      reading: None,
    });
    return row;
  }

  #[test]
  fn table_rows_stack_by_their_content_band() {
    // 2 行目のセルは高さ 12・深さ 6 の箱を持ち、行送り 10（フォント 10 × 係数 1）を超えて 18 を占める
    let geom = test_geometry();
    let tall = table_fragment(
      HBox {
        content: HBoxContent::Atom(Vec::new()),
        width: Length::pt(10.0),
        height: Length::pt(12.0),
        depth: Length::pt(6.0),
      },
      18.0,
      12.0,
    );
    let table = table_box(Vec::new(), vec![table_row("A"), tall, table_row("B")]);

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    let Some(PlacedBlock::Table { rows }) = pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. }))
    else {
      panic!("1 ページ目に表があるはず: {pages:?}");
    };
    assert!(close(rows[0].top_y, 10.0), "{:?}", rows[0]);
    assert!(close(rows[1].top_y, 20.0), "{:?}", rows[1]);
    assert!(close(rows[2].top_y, 38.0), "2 行目は中身の高さ + 深さ 18 を占める: {:?}", rows[2]);
    assert!(close(rows[1].height, 18.0), "{:?}", rows[1]);
    assert!(close(rows[1].baseline_y, 32.0), "余りが無いのでベースラインは上端 20 + 高さ 12: {:?}", rows[1]);
  }

  /// ページ内の最初の表ブロックの先頭行セルのテキストを取り出すヘルパ
  fn first_table_row_text(page: &Page) -> Option<String> {
    for block in &page.blocks {
      if let PlacedBlock::Table { rows, .. } = block
        && let Some(first) = rows.first()
        && let Some(placed) = first.boxes.first()
        && let HBoxContent::Glyphs(run) = &placed.hbox.content
      {
        return Some(run.text.clone());
      }
    }
    return None;
  }

  #[test]
  fn consecutive_page_breaks_without_content_collapse() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::force_break(),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "中間に白紙ページは生じない: {pages:?}");
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[1].blocks.len(), 1);
  }

  #[test]
  fn trailing_page_break_does_not_create_blank_page() {
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(1), Block::force_break()];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].blocks.len(), 1);
  }

  #[test]
  fn page_break_only_input_returns_single_empty_page() {
    let geom = test_geometry();
    let blocks = vec![Block::force_break()];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert!(pages[0].blocks.is_empty());
  }

  #[test]
  fn breakable_table_splits_across_pages_and_redraws_header() {
    let geom = test_geometry();
    let table = table_box(vec![table_row("HEAD")], (0..5).map(|i| return table_row(&format!("R{i}"))).collect());

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]).as_deref(), Some("HEAD"), "1 ページ目もヘッダ始まり");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("HEAD"), "2 ページ目はヘッダ再描画");
  }

  /// 1 列 1 行、セル内容がまるごとリンク（`LinkStart`, box(幅20), `LinkEnd`）の表を作る
  fn single_cell_link_table(target: LinkTarget) -> TableBox {
    return table_box(Vec::new(), vec![with_link(table_row("リンク"), target)]);
  }

  #[test]
  fn table_cell_link_becomes_placed_link_on_page() {
    let geom = test_geometry();
    let target = LinkTarget::External("https://example.com".to_string());
    let table = single_cell_link_table(target.clone());

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert_eq!(link.target, target);
    assert!(close(link.x, 2.0), "{link:?}");
    assert!(close(link.y, 10.0), "{link:?}");
    assert!(close(link.width, 20.0), "{link:?}");
  }

  #[test]
  fn place_table_shifts_rule_and_boxes_to_the_landing_point() {
    // 罫線と箱は表左端・行帯上端を原点に確定済みで、配置は着地点（段オフセット 0・上端 10）を足すだけ
    let geom = test_geometry();
    let mut row = table_row("R0");
    row.row.rule = Some(PlacedTableRule {
      x: Length::ZERO,
      y: Length::ZERO,
      width: pt(24.0),
      height: pt(1.5),
      color: Some([9, 9, 9]),
    });
    let table = TableBox {
      breakable: false,
      ..table_box(Vec::new(), vec![row])
    };

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!("直前の matches! で確認済み");
    };
    assert!(close(rows[0].boxes[0].dx, 2.0), "箱の x は断片の値のまま: {:?}", rows[0]);
    let rule = rows[0].rule.expect("罫線を持つ断片なので確定罫線を持つはず");
    assert!(close(rule.x, 0.0) && close(rule.y, 10.0), "{rule:?}");
    assert!(close(rule.width, 24.0) && close(rule.height, 1.5), "{rule:?}");
    assert_eq!(rule.color, Some([9, 9, 9]));
  }

  #[test]
  fn table_link_shifts_with_its_row_under_flush_bottom() {
    let geom = flush_geometry();
    let target = LinkTarget::Internal(AnchorId::Label(LabelId::new("fig:1")));
    let table = single_cell_link_table(target.clone());
    let blocks = vec![
      fixed_block(8.0),                           // idx0, bottom=18（シフト対象外）
      Block::stretchable_space(pt(4.0), pt(4.0)), // stretch アキ
      Block::Table {
        table,
        align: Align::Left,
      }, // idx1, 行高10。glue 後の y=22、bottom=32
      fixed_block(40.0),                          // 溢れて改ページ（不足 50-32=18 を配分）
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(fixed_block_ys(&pages[0]), vec![Length::pt(10.0)], "先頭のブロックはシフトされない");
    let PlacedBlock::Table { rows, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!("直前の matches! で確認済み");
    };
    let row_top_y = rows[0].top_y;
    assert!(close(row_top_y, 40.0), "行帯下端が 50 に達するよう +18 シフトされるはず: {row_top_y:?}");
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert_eq!(link.target, target);
    assert!(close(link.y, row_top_y.to_pt()), "リンクは行と同量シフトされ揃ったままのはず: {link:?}");
  }

  #[test]
  fn pending_anchor_resolves_to_next_paragraph_top() {
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorId::Heading(HeadingKey::new(0))),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!(close(pages[0].anchors[0].y, 2.0));
    assert!(close(pages[0].anchors[0].x, 0.0));
    assert!(matches!(pages[0].anchors[0].id, AnchorId::Heading(_)));
  }

  #[test]
  fn consecutive_anchors_resolve_to_the_same_point() {
    // ラベル付き見出しと同じ並び（見出しキー → ラベル → 内容）
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorId::Heading(HeadingKey::new(0))),
      Block::Anchor(AnchorId::Label(LabelId::new("sec:x"))),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let anchors = &pages[0].anchors;
    assert_eq!(anchors.len(), 2, "{anchors:?}");
    assert_eq!(anchors[0].id, AnchorId::Heading(HeadingKey::new(0)));
    assert_eq!(anchors[1].id, AnchorId::Label(LabelId::new("sec:x")));
    assert_eq!((anchors[0].x, anchors[0].y), (anchors[1].x, anchors[1].y), "同じ座標で解決される");
  }

  #[test]
  fn pending_anchor_resolves_on_page_after_break() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(4),
      Block::Anchor(AnchorId::Label(LabelId::new("tab:x"))),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].anchors.is_empty(), "ページ 1 にアンカーは無い: {:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!(close(pages[1].anchors[0].y, 2.0));
  }

  #[test]
  fn paragraph_link_becomes_placed_link() {
    use crate::typeset::boxes::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(matches!(&link.target, LinkTarget::External(uri) if uri == "https://example.com"));
    assert!(close(link.x, 0.0));
    assert!(close(link.y, 2.0));
    assert!(close(link.width, 20.0));
    assert!(close(link.height, 10.0));
  }

  #[test]
  fn paragraph_right_indent_reduces_available_width() {
    let geom = test_geometry();
    let mut items = Vec::new();
    for i in 0..6 {
      if i > 0 {
        items.push(HItem::Glue {
          natural: Length::pt(5.0),
          stretch: Length::pt(0.0),
          shrink: Length::pt(0.0),
          breakable: true,
        });
      }
      items.push(test_box());
    }
    let blocks = vec![Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(10.0),
      right_indent: Length::pt(10.0),
      alignment: TextAlignment::Left,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(60.0), &geom, &GreedyBreaker);

    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .collect();
    assert!(lines.len() >= 2, "利用可能幅 40 で折り返すはず: {} 行", lines.len());
    for line in &lines {
      let first = line.boxes.first().expect("各行にボックスがあるはず");
      assert!(first.dx.to_pt() >= 10.0 - f32::EPSILON, "先頭ボックス x={} は indent(10) 以上", first.dx.to_pt());
      for placed in &line.boxes {
        assert!(
          (placed.dx + placed.hbox.width).to_pt() <= 50.0 + f32::EPSILON,
          "x+width={} <= text_width - right_indent = 50",
          (placed.dx + placed.hbox.width).to_pt()
        );
      }
    }
  }

  #[test]
  fn paragraph_indent_shifts_links() {
    use crate::typeset::boxes::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(15.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(close(link.x, 15.0), "link.x={}", link.x.to_pt());
    assert!(close(link.width, 20.0), "link.width={}", link.width.to_pt());
  }

  #[test]
  fn right_aligned_paragraph_shifts_line_to_right_edge() {
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      origin: None,
      items: vec![test_box()],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Right,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].dx, 90.0), "box.x={}", line.boxes[0].dx.to_pt());
  }

  /// テスト用の伸縮能力付き breakable glue（幅 5・伸長 2.5・収縮 5/3 = 単語間スペース相当）
  fn stretch_glue() -> HItem {
    return HItem::Glue {
      natural: Length::pt(5.0),
      stretch: Length::pt(2.5),
      shrink: Length::pt(5.0) / 3.0,
      breakable: true,
    };
  }

  /// 伸縮能力付き glue で折り返す 2 行の段落（1 行目自然幅 25）を作る
  fn stretchable_paragraph(alignment: TextAlignment) -> Block {
    return Block::Paragraph {
      origin: None,
      items: vec![
        test_box(),
        stretch_glue(),
        test_box(),
        stretch_glue(),
        test_box(),
      ],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment,
    };
  }

  #[test]
  fn justified_paragraph_stretches_glue() {
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(TextAlignment::Justify)];

    let (pages, _) = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker);

    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[1].dx + line.boxes[1].hbox.width, 27.0), "{:?}", line.boxes);
  }

  #[test]
  fn centered_paragraph_does_not_stretch_glue() {
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(TextAlignment::Center)];

    let (pages, _) = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker);

    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].dx, 1.0), "{:?}", line.boxes);
    assert!(close(line.boxes[1].dx - line.boxes[0].dx, 15.0), "glue は自然幅のまま: {:?}", line.boxes);
  }

  #[test]
  fn centered_overflowing_line_is_not_shifted_negative() {
    let geom = test_geometry();
    let wide = HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: Length::pt(50.0),
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
    });
    let blocks = vec![Block::Paragraph {
      origin: None,
      items: vec![wide],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Center,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(30.0), &geom, &GreedyBreaker);

    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].dx, 0.0), "box.x={}", line.boxes[0].dx.to_pt());
  }

  #[test]
  fn centered_wrapped_lines_are_each_independently_centered() {
    let geom = test_geometry();
    let items = vec![
      test_box(),
      HItem::Glue {
        natural: Length::pt(5.0),
        stretch: Length::pt(0.0),
        shrink: Length::pt(0.0),
        breakable: true,
      },
      test_box(),
      HItem::Glue {
        natural: Length::pt(5.0),
        stretch: Length::pt(0.0),
        shrink: Length::pt(0.0),
        breakable: true,
      },
      test_box(),
    ];
    let blocks = vec![Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Center,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(35.0), &geom, &GreedyBreaker);

    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .collect();
    assert_eq!(lines.len(), 2, "text_width=35 で 2 行に折り返すはず: {} 行", lines.len());
    assert!(close(lines[0].boxes[0].dx, 5.0), "1 行目先頭 x={}", lines[0].boxes[0].dx.to_pt());
    assert!(close(lines[1].boxes[0].dx, 12.5), "2 行目先頭 x={}", lines[1].boxes[0].dx.to_pt());
  }

  /// ページ内の最初の `PlacedBlock::Image` を取り出すヘルパ
  fn first_image(page: &Page) -> &PlacedBlock {
    return page.blocks.iter().find(|b| matches!(b, PlacedBlock::Image { .. })).expect("画像があるはず");
  }

  #[test]
  fn centered_image_shifts_x_to_horizontal_center() {
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      origin: image_origin(),
      path: ProjectPath::new("x.png"),
      width: pt(20.0),
      height: pt(15.0),
      target_dpi: None,
      align: Align::Center,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!(close(*x, 40.0), "image.x={}", x.to_pt());
  }

  #[test]
  fn right_aligned_image_shifts_x_to_right_edge() {
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      origin: image_origin(),
      path: ProjectPath::new("x.png"),
      width: pt(20.0),
      height: pt(15.0),
      target_dpi: None,
      align: Align::Right,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!(close(*x, 80.0), "image.x={}", x.to_pt());
  }

  #[test]
  fn centered_table_shifts_all_rows_x() {
    let geom = test_geometry();
    let table = table_box(vec![table_row("HEAD")], vec![table_row("R0")]);
    let blocks = vec![Block::Table {
      table,
      align: Align::Center,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!(close(rows[0].boxes[0].dx, 40.0), "cell.x={}", rows[0].boxes[0].dx.to_pt());
  }

  #[test]
  fn full_width_table_is_not_shifted_by_center() {
    let geom = test_geometry();
    // 表幅が段幅（100）いっぱいの表
    let table = TableBox {
      width: pt(100.0),
      ..table_box(vec![table_row("HEAD")], vec![table_row("R0")])
    };
    let blocks = vec![Block::Table {
      table,
      align: Align::Center,
    }];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!(close(rows[0].boxes[0].dx, 2.0), "cell.x={}", rows[0].boxes[0].dx.to_pt());
  }

  #[test]
  fn no_line_baseline_exceeds_page_limit() {
    let geom = test_geometry();

    let (pages, _) = break_pages(vec![paragraph_of_lines(12)], Length::pt(100.0), &geom, &GreedyBreaker);

    assert!(pages.len() >= 2, "複数ページに分かれる: {}", pages.len());
    for page in &pages {
      for block in &page.blocks {
        if let PlacedBlock::Line { line, baseline_y } = block {
          assert!(
            (*baseline_y + line.depth).to_pt() <= geom.page_limit.to_pt() + f32::EPSILON,
            "baseline={} depth={} が page_limit={} を超えた",
            baseline_y.to_pt(),
            line.depth.to_pt(),
            geom.page_limit.to_pt()
          );
        }
      }
    }
  }

  /// 合成済み単一行（[`Block::ComposedLine`]）のテスト用ヘルパ。幅・高さ・深さと任意のリンクを持つ
  fn composed_line(width: f32, height: f32, depth: f32, link: Option<LinkTarget>) -> Block {
    let width = pt(width);
    let height = pt(height);
    let depth = pt(depth);
    let links = link.map_or_else(Vec::new, |target| {
      return vec![LineLink {
        target,
        x0: Length::ZERO,
        x1: width,
      }];
    });
    return Block::ComposedLine {
      origin: None,
      line: Line {
        boxes: vec![PlacedHBox {
          hbox: HBox {
            content: HBoxContent::Atom(Vec::new()),
            width,
            height,
            depth,
          },
          dx: Length::ZERO,
          dy: Length::ZERO,
        }],
        height,
        depth,
        links,
        footnotes: Vec::new(),
        index_marks: Vec::new(),
      },
      leading: Length::pt(12.0),
    };
  }

  #[test]
  fn composed_line_places_at_baseline_with_leading() {
    let geom = test_geometry();
    let blocks = vec![
      composed_line(20.0, 8.0, 2.0, None),
      composed_line(20.0, 8.0, 2.0, None),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    let baselines: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
    assert_eq!(baselines, pts(&[10.0, 22.0]));
  }

  #[test]
  fn composed_line_resolves_anchor_and_collects_link() {
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorId::Heading(HeadingKey::new(0))),
      composed_line(20.0, 8.0, 2.0, Some(LinkTarget::Internal(AnchorId::Heading(HeadingKey::new(5))))),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!(close(pages[0].anchors[0].y, 2.0));
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    assert!(
      matches!(&pages[0].links[0].target, LinkTarget::Internal(k) if *k == AnchorId::Heading(HeadingKey::new(5)))
    );
    assert!(close(pages[0].links[0].y, 2.0));
    assert!(close(pages[0].links[0].height, 10.0));
  }

  #[test]
  fn composed_lines_break_across_pages() {
    let geom = test_geometry();
    let blocks: Vec<Block> = std::iter::repeat_n(composed_line(20.0, 8.0, 2.0, None), 5).collect();

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Line { baseline_y, .. } = pages[1].blocks.first().expect("2 ページ目に行があるはず")
    else {
      panic!("Line を期待");
    };
    assert!(close(*baseline_y, 10.0));
  }

  #[test]
  fn two_column_flow_fills_left_then_right_then_next_page() {
    let geom = two_column_geometry();

    let (pages, _) = break_pages(vec![paragraph_of_lines(9)], Length::pt(100.0), &geom, &GreedyBreaker);

    let page_lines = |page: &Page| -> Vec<(f32, f32)> {
      return page
        .blocks
        .iter()
        .filter_map(|b| match b {
          PlacedBlock::Line { line, baseline_y } => return Some(((*baseline_y).to_pt(), line.boxes[0].dx.to_pt())),
          _ => return None,
        })
        .collect();
    };
    assert_eq!(pages.len(), 2, "{pages:?}");
    let p0 = page_lines(&pages[0]);
    assert_eq!(p0.len(), 7, "1 ページ目は左段 4 行 + 右段 3 行（widow 制御で右段の 4 行目が繰り下がる）: {p0:?}");
    assert_eq!(p0[0..4], [(10.0, 0.0), (22.0, 0.0), (34.0, 0.0), (46.0, 0.0)], "左段は x=0 で 10,22,34,46");
    assert_eq!(
      p0[4..7],
      [(10.0, 55.0), (22.0, 55.0), (34.0, 55.0)],
      "右段は x=55 で baseline がリセットされ、末尾 2 行を送るため 3 行"
    );
    let p1 = page_lines(&pages[1]);
    assert_eq!(p1, [(10.0, 0.0), (22.0, 0.0)], "末尾 2 行（8・9 行目）が次ページ左段に揃う");
  }

  #[test]
  fn two_column_paragraph_link_rect_uses_column_offset() {
    use crate::typeset::boxes::LinkTarget;
    let geom = two_column_geometry();
    let link_para = Block::Paragraph {
      origin: None,
      items: vec![
        HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
        test_box(),
        test_box(),
        HItem::LinkEnd,
      ],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    };
    let blocks = vec![paragraph_of_lines(5), link_para];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "全行が 1 ページの 2 段に収まる: {pages:?}");
    let link = pages[0]
      .links
      .iter()
      .find(|l| matches!(&l.target, LinkTarget::External(uri) if uri == "https://example.com"))
      .expect("External リンクがあるはず");
    assert!(close(link.x, 55.0), "リンク x={} は段オフセット 55", link.x.to_pt());
    assert!(close(link.width, 20.0), "リンク幅={}", link.width.to_pt());
  }

  #[test]
  fn breakable_table_spans_columns_uses_column_offset() {
    let geom = two_column_geometry();
    let table = table_box(Vec::new(), (0..6).map(|i| return table_row(&format!("R{i}"))).collect());

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    assert_eq!(pages.len(), 1, "{pages:?}");
    let xs: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Table { rows } => return rows.first()?.boxes.first().map(|placed| return placed.dx),
        _ => return None,
      })
      .collect();
    assert_eq!(xs.len(), 2, "左段断片 + 右段断片の 2 つ: {xs:?}");
    assert!(close(xs[0], 2.0), "1 つ目（左段）のセル x={}", xs[0].to_pt());
    assert!(close(xs[1], 57.0), "2 つ目（右段）のセル x={}", xs[1].to_pt());
  }

  /// 2 段組みで区切り罫線も出すテスト用ジオメトリ（段幅 45・右段オフセット 55）
  fn two_column_geometry_with_footnote_rule() -> PageGeometry {
    return PageGeometry {
      num_columns: 2,
      column_gap: Length::pt(10.0),
      ..geometry_with_footnote_rule()
    };
  }

  /// 左段と右段の両方に脚注が付く 8 行段落を作る（`two_column_geometry_with_footnote_rule` 前提）
  ///
  /// 脚注の予約でリージョンが縮むため、この版面では 0 行目が左段・2 行目が右段に着地する。
  fn paragraph_with_footnotes_in_both_columns(left: HItem, right: HItem) -> Block {
    let mut left = Some(left);
    let mut right = Some(right);
    let mut items = Vec::new();
    for i in 0..8 {
      if i > 0 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
      if i == 0 {
        items.push(left.take().expect("左段の脚注はここだけ"));
      }
      if i == 2 {
        items.push(right.take().expect("右段の脚注はここだけ"));
      }
    }
    return Block::Paragraph {
      origin: None,
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      alignment: TextAlignment::Left,
    };
  }

  /// ページの脚注のうち、指定番号の区切り罫線の x（罫線が無ければ `None`）
  fn footnote_rule_x(page: &Page, number: u32) -> Option<Length> {
    return footnote_numbered(page, number)?.blocks.iter().find_map(|b| match b {
      PlacedBlock::Rule { x, .. } => return Some(*x),
      _ => return None,
    });
  }

  /// ページの脚注のうち、指定番号の本体行の先頭ボックスの x 列
  fn footnote_line_xs(page: &Page, number: u32) -> Vec<Length> {
    return footnote_numbered(page, number)
      .expect("指定番号の脚注があるはず")
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => return line.boxes.first().map(|placed| return placed.dx),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn two_column_footnote_link_rect_uses_column_offset() {
    let geom = two_column_geometry_with_footnote_rule();
    let blocks = vec![paragraph_with_footnotes_in_both_columns(
      footnote_of_lines_with_link_at(1, 1, 0, "https://left.example"),
      footnote_of_lines_with_link_at(2, 1, 0, "https://right.example"),
    )];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    // 空振り検知。罫線の x は段オフセットを持つので、
    // 「左段と右段の両方に脚注が着地した」ことの前提条件として使える
    let left_rule = footnote_rule_x(&pages[0], 1).expect("左段の脚注 1 に区切り罫線があるはず");
    let right_rule = footnote_rule_x(&pages[0], 2).expect("右段の脚注 2 に区切り罫線があるはず");
    assert!(close(left_rule, 0.0), "脚注 1 が左段に着地していない（罫線 x={}）", left_rule.to_pt());
    assert!(close(right_rule, 55.0), "脚注 2 が右段に着地していない（罫線 x={}）", right_rule.to_pt());

    for (number, uri, expected) in [
      (1u32, "https://left.example", 0.0),
      (2u32, "https://right.example", 55.0),
    ] {
      let links = external_links(&pages[0], uri);
      assert_eq!(links.len(), 1, "{uri} のクリック矩形は 1 つのはず: {links:?}");
      assert!(
        close(links[0].x, expected),
        "{uri} の矩形 x={} は段オフセット {expected} のはず",
        links[0].x.to_pt()
      );
      let xs = footnote_line_xs(&pages[0], number);
      assert_eq!(xs.first().copied(), Some(links[0].x), "矩形 x が脚注本体のテキスト位置と一致するはず");
    }
  }

  #[test]
  fn is_content_block_classifies_variants() {
    assert!(is_content_block(&paragraph_of_lines(1)));
    assert!(is_content_block(&fixed_block(1.0)));
    assert!(!is_content_block(&Block::fixed_space(pt(5.0))));
    assert!(!is_content_block(&forbid_break()));
    assert!(!is_content_block(&Block::force_break()));
  }

  /// 位置 `y`・脚注予約 `reserved` のカーソル
  fn cursor(y: f32, at_edge: bool, reserved: f32) -> RegionCursor {
    return RegionCursor {
      y: pt(y),
      at_edge,
      footnote_reserved: pt(reserved),
    };
  }

  /// 高さ 8・深さ 2 の空行（カーソル判定のテスト用）
  fn bare_line() -> Line {
    return Line {
      boxes: Vec::new(),
      height: pt(8.0),
      depth: pt(2.0),
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }

  #[test]
  fn region_limit_subtracts_footnote_reservation() {
    assert_eq!(cursor(10.0, false, 14.0).region_limit(&test_geometry()), pt(36.0));
  }

  #[test]
  fn overflows_uses_the_reserved_limit() {
    let geom = test_geometry();

    assert!(!cursor(26.0, false, 14.0).overflows(pt(10.0), &geom), "下端 36 は下限 36 に収まる");
    assert!(cursor(26.0, false, 14.0).overflows(pt(11.0), &geom), "下端 37 は下限 36 を超える");
  }

  #[test]
  fn defers_unbreakable_only_when_an_empty_region_would_hold_it() {
    let geom = test_geometry();

    assert!(cursor(30.0, false, 0.0).defers_unbreakable(pt(30.0), &geom), "空リージョンなら 40 ≤ 50 で収まる");
    assert!(
      !cursor(30.0, false, 0.0).defers_unbreakable(pt(45.0), &geom),
      "空リージョンでも収まらないなら送らない"
    );
    assert!(!cursor(10.0, false, 0.0).defers_unbreakable(pt(30.0), &geom), "収まるなら送らない");
  }

  #[test]
  fn line_baseline_drops_by_ascent_after_an_edge_block() {
    assert_eq!(cursor(20.0, false, 0.0).line_baseline(&bare_line()), pt(20.0));
    assert_eq!(cursor(20.0, true, 0.0).line_baseline(&bare_line()), pt(28.0));
  }

  #[test]
  fn line_overflows_compares_descender_with_the_reserved_limit() {
    let geom = test_geometry();

    assert!(!cursor(34.0, false, 14.0).line_overflows(&bare_line(), &geom), "下端 36 は下限 36 に収まる");
    assert!(cursor(34.0, false, 26.0).line_overflows(&bare_line(), &geom), "下端 36 は下限 24 を超える");
  }

  #[test]
  fn below_and_after_line_keep_the_reservation() {
    let start = cursor(20.0, false, 14.0);

    assert_eq!(start.below(pt(5.0)), cursor(25.0, true, 14.0));
    assert_eq!(start.after_line(pt(22.0), pt(12.0)), cursor(34.0, false, 14.0));
  }

  #[test]
  fn keep_group_end_links_heading_to_following_block() {
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(2),
    ];

    assert_eq!(keep_group_end(&blocks, 0), Some(3));
  }

  #[test]
  fn keep_group_end_none_without_forbid() {
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      paragraph_of_lines(2),
    ];

    assert_eq!(keep_group_end(&blocks, 0), None);
  }

  #[test]
  fn keep_group_end_chains_consecutive_headings() {
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(2),
    ];

    assert_eq!(keep_group_end(&blocks, 0), Some(6));
  }

  #[test]
  fn keep_group_end_severed_by_forced_break() {
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      Block::force_break(),
      paragraph_of_lines(2),
    ];

    assert_eq!(keep_group_end(&blocks, 0), None);
  }

  #[test]
  fn heading_kept_with_body_moves_to_next_page() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3), // filler
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![3, 3], "{pages:?}");
  }

  #[test]
  fn heading_without_keep_marker_is_orphaned() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1),
      Block::fixed_space(pt(0.0)),
      paragraph_of_lines(2),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![4, 2], "{pages:?}");
  }

  #[test]
  fn consecutive_headings_move_together() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1), // 見出し 1
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(1), // 見出し 2
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![3, 4], "{pages:?}");
  }

  #[test]
  fn heading_with_fitting_body_stays_in_place() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1), // filler → y=22
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![4], "{pages:?}");
  }

  #[test]
  fn doc_final_heading_without_body_does_not_hang() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1),
      Block::fixed_space(pt(0.0)),
      forbid_break(),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![4], "{pages:?}");
  }

  #[test]
  fn unavoidable_keep_at_region_top_does_not_add_blank_page() {
    let geom = PageGeometry {
      page_limit: Length::pt(30.0),
      ..test_geometry()
    };
    let blocks = vec![
      paragraph_of_lines(1), // 見出し（ページ先頭）
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![1, 2], "{pages:?}");
  }

  #[test]
  fn heading_kept_with_body_when_footnote_reservation_shrinks_region() {
    // 1 行目の脚注 2 行でエリア 26 → 実効下限 24。見出し（ベースライン 22）は収まるが、
    // 本文 1 行目（ベースライン 34）は収まらない
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 2)]),
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![1, 3], "{pages:?}");
    assert_eq!(pages[0].footnotes.len(), 1, "脚注は参照行と同じ 1 ページ目に残る");
  }

  #[test]
  fn heading_kept_with_body_whose_first_line_carries_a_footnote() {
    // 本文 1 行目（ベースライン 46）は自前の脚注（エリア 14）込みでは下限 36 を超える
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(2), // filler
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      single_line_paragraph(vec![footnote_of_lines(1, 1)]), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![2, 2], "{pages:?}");
    assert_eq!(pages[1].footnotes.len(), 1, "脚注は本文と一緒に 2 ページ目へ移る");
  }

  #[test]
  fn heading_kept_with_image_when_footnote_reservation_shrinks_region() {
    // 見出しの後の画像（y 34 から高さ 10 → 下端 44）は実効下限 24 を超える
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 2)]),
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      fixed_block(10.0),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![1, 1], "{pages:?}");
    assert_eq!(fixed_block_ys(&pages[1]), pts(&[22.0]), "画像は見出し（ベースライン 10）の 1 行送り下");
  }

  #[test]
  fn heading_kept_with_short_body_that_widow_control_sends_whole() {
    // 脚注なし。本文 3 行のうち 2 行は収まるが、widow 補正が 4 行未満の段落を丸ごと送る
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1), // filler
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(3), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![1, 4], "{pages:?}");
  }

  #[test]
  fn heading_with_body_fitting_under_footnote_reservation_stays_in_place() {
    // 脚注 1 行でエリア 14 → 実効下限 36。本文（ベースライン 34・下端 36）はちょうど収まる
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 1)]),
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(1), // 本文
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(line_counts(&pages), vec![3], "{pages:?}");
  }

  /// 下端揃えを有効にしたテスト用ジオメトリ（他は `test_geometry` と同じ）
  fn flush_geometry() -> PageGeometry {
    return PageGeometry {
      flush_bottom: true,
      ..test_geometry()
    };
  }

  /// 高さ `height` の内容ブロック（幅 10・左揃えの画像）
  fn fixed_block(height: f32) -> Block {
    return Block::Image {
      origin: image_origin(),
      path: ProjectPath::new("fixture.png"),
      width: Length::pt(10.0),
      height: pt(height),
      target_dpi: None,
      align: Align::Left,
    };
  }

  /// ページ内の [`fixed_block`] の上端 y を上から順に集める
  fn fixed_block_ys(page: &Page) -> Vec<Length> {
    return page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Image { y, .. } => return Some(*y),
        _ => return None,
      })
      .collect();
  }

  /// ページ内ブロックの底辺の最大値（版面下端に達したかの確認用）
  fn max_block_bottom(page: &Page) -> f32 {
    return page.blocks.iter().map(placed_block_bottom).fold(Length::from_sp(i64::MIN), Length::max).to_pt();
  }

  /// 2 つの `f32` がほぼ等しいか（配置座標の比較用）
  fn approx(a: f32, b: f32) -> bool { return (a - b).abs() < 1e-3; }

  #[test]
  fn flush_bottom_distributes_deficit_into_stretch_glue() {
    let geom = flush_geometry();
    let blocks = vec![
      fixed_block(10.0),                          // y=10..20
      Block::stretchable_space(pt(4.0), pt(4.0)), // 24
      fixed_block(10.0),                          // y=24..34
      Block::stretchable_space(pt(4.0), pt(4.0)), // 38
      fixed_block(10.0),                          // y=38..48（不足 2pt）
      fixed_block(10.0),                          // 溢れて改ページ（1 ページ目を確定＝下端揃え発火）
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2);
    assert_eq!(fixed_block_ys(&pages[0]), pts(&[10.0, 25.0, 40.0]));
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "{:?}", pages[0]);
    assert_eq!(fixed_block_ys(&pages[1]), pts(&[10.0]));
  }

  #[test]
  fn flush_bottom_disabled_keeps_ragged_bottom() {
    let geom = test_geometry();
    let blocks = vec![
      fixed_block(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      fixed_block(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      fixed_block(10.0),
      fixed_block(10.0),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(fixed_block_ys(&pages[0]), pts(&[10.0, 24.0, 38.0]));
    assert!(approx(max_block_bottom(&pages[0]), 48.0));
  }

  #[test]
  fn flush_bottom_skips_page_before_forced_break() {
    let geom = flush_geometry();
    let blocks = vec![
      fixed_block(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      fixed_block(10.0),
      Block::force_break(), // このページは強制改ページで終わる → 揃えない
      fixed_block(10.0),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2);
    assert_eq!(fixed_block_ys(&pages[0]), pts(&[10.0, 24.0]));
    assert_eq!(fixed_block_ys(&pages[1]), pts(&[10.0]));
  }

  #[test]
  fn flush_bottom_shifts_each_element_by_its_own_preceding_stretch() {
    // 伸縮アキ 2 個の前・間・後に行（リンク付き）とアンカーを置く。
    // 不足 = 50 − 44 = 6、分母 = 末尾行より前の stretch 8 → ratio 0.75。
    // 先行 stretch が 0 / 4 / 8 の要素はそれぞれ +0 / +3 / +6 動く
    let geom = flush_geometry();
    let link = || return Some(LinkTarget::External("https://example.com".to_string()));
    let blocks = vec![
      Block::Anchor(AnchorId::Label(LabelId::new("a0"))),
      composed_line(20.0, 8.0, 2.0, link()), // baseline 10（先行 stretch 0）
      Block::stretchable_space(pt(4.0), pt(4.0)),
      Block::Anchor(AnchorId::Label(LabelId::new("a1"))),
      composed_line(20.0, 8.0, 2.0, link()), // baseline 26（先行 stretch 4）
      Block::stretchable_space(pt(4.0), pt(4.0)),
      composed_line(20.0, 8.0, 2.0, link()), // baseline 42・下端 44（先行 stretch 8）
      fixed_block(30.0),                     // 溢れて改ページ（1 ページ目を確定＝下端揃え発火）
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 29.0, 48.0]));
    let anchor_ys: Vec<Length> = pages[0].anchors.iter().map(|a| return a.y).collect();
    assert_eq!(anchor_ys, pts(&[2.0, 21.0]), "アンカーは同じ着地の行と同量だけ動く: {:?}", pages[0].anchors);
    let link_ys: Vec<Length> = pages[0].links.iter().map(|l| return l.y).collect();
    assert_eq!(link_ys, pts(&[2.0, 21.0, 40.0]), "リンク矩形は自分の行と同量だけ動く: {:?}", pages[0].links);
  }

  #[test]
  fn flush_bottom_leaves_footnote_links_and_anchors_unshifted() {
    // 2 行目は本文リンクと 1 行脚注（リンク付き）を持つ。脚注エリア 14（gap 4 + 行 10）で
    // region_limit = 36、不足 = 36 − 28 = 8、分母 = 先行 stretch 4 → ratio 2、2 行目は +8。
    // 脚注は region_limit 36 + gap 4 = 40 から組まれ、揃えでは動かない
    let geom = flush_geometry();
    let blocks = vec![
      paragraph_of_lines(1), // baseline 10
      Block::stretchable_space(pt(4.0), pt(4.0)),
      single_line_paragraph(vec![
        HItem::LinkStart(LinkTarget::Internal(AnchorId::Label(LabelId::new("body")))),
        test_box(),
        HItem::LinkEnd,
        footnote_of_lines_with_link_at(1, 1, 0, "https://example.com"),
      ]), // baseline 26 → 揃え後 34
      fixed_block(30.0), // 溢れて改ページ
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 34.0]));
    let body_link = pages[0]
      .links
      .iter()
      .find(|l| return matches!(l.target, LinkTarget::Internal(_)))
      .expect("本文リンクがあるはず");
    assert!(close(body_link.y, 26.0), "本文リンクは行と一緒に +8 動く: {body_link:?}");
    assert_eq!(footnote_baselines(&pages[0], 1), pts(&[48.0]), "脚注本体は揃えで動かない");
    let footnote_links = external_links(&pages[0], "https://example.com");
    assert_eq!(footnote_links.len(), 1, "{:?}", pages[0].links);
    assert!(close(footnote_links[0].y, 40.0), "脚注のリンクは脚注行の上端のまま: {:?}", footnote_links[0]);
    let footnote_anchor = pages[0]
      .anchors
      .iter()
      .find(|a| return matches!(a.id, AnchorId::Footnote(_)))
      .expect("脚注先頭のアンカーがあるはず");
    assert!(close(footnote_anchor.y, 40.0), "脚注のアンカーは脚注の上端のまま: {footnote_anchor:?}");
  }

  #[test]
  fn table_redraws_the_repeated_head_after_a_break() {
    // head 1 行 + 本体 5 行（行高 10）は 2 ページに割れ、2 ページ目の先頭に再掲ヘッダが積まれる。
    // 再掲ヘッダの断片はリンクだけを持ち（索引語は表エンジンが除く）、初出の断片の索引語は 1 ページ目に帰属する
    let geom = test_geometry();
    let head =
      with_index(with_link(table_row("HEAD"), LinkTarget::External("https://example.com".to_string())), "ヘッダ語");
    let table = table_box(vec![head], (0..5).map(|i| return table_row(&format!("R{i}"))).collect());

    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
    );

    assert_eq!(pages.len(), 2, "{pages:?}");
    for (index, page) in pages.iter().enumerate() {
      assert_eq!(first_table_row_text(page).as_deref(), Some("HEAD"), "page {index}: 先頭行はヘッダ");
      let links = external_links(page, "https://example.com");
      assert_eq!(links.len(), 1, "page {index}: head のリンクは断片ごとに 1 つ: {:?}", page.links);
      assert!(close(links[0].y, 10.0), "page {index}: head は各断片の先頭行: {:?}", links[0]);
      assert!(close(links[0].height, 10.0), "page {index}: {:?}", links[0]);
    }
    let words: Vec<Vec<&str>> = pages
      .iter()
      .map(|page| return page.index_entries.iter().map(|e| return e.word.as_str()).collect())
      .collect();
    assert_eq!(words, vec![vec!["ヘッダ語"], vec![]], "索引語は初出の断片のページだけ");
  }

  #[test]
  fn pending_anchor_before_breakable_table_lands_with_the_first_row_on_the_next_page() {
    // 3 行段落の後（y=46）で表を開始するが、最初の行（高さ 10）は収まらず次ページへ送られる。
    // アンカーは表の実配置（2 ページ目の先頭行上端 = margin_top）で解決し、1 ページ目には残らない
    let geom = test_geometry();
    let table = table_box(Vec::new(), vec![table_row("R0"), table_row("R1")]);
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Anchor(AnchorId::Label(LabelId::new("tab:x"))),
      Block::Table {
        table,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]), None, "表は 1 ページ目に無い");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("R0"));
    assert!(pages[0].anchors.is_empty(), "前ページにアンカーは残らない: {:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!(close(pages[1].anchors[0].x, 0.0), "{:?}", pages[1].anchors[0]);
    assert!(close(pages[1].anchors[0].y, 10.0), "先頭行の上端 = margin_top: {:?}", pages[1].anchors[0]);
  }

  #[test]
  fn pending_anchor_before_breakable_table_lands_in_the_column_of_the_first_row() {
    // 2 段。左段 3 行（y=46）の後の表の先頭行（高さ 10）が収まらず右段へ送られる。
    // アンカーの x は着地段（右段）のオフセット 55、y は右段先頭 = margin_top
    let geom = two_column_geometry();
    let table = table_box(Vec::new(), vec![table_row("R0"), table_row("R1")]);
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Anchor(AnchorId::Label(LabelId::new("tab:x"))),
      Block::Table {
        table,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]).as_deref(), Some("R0"));
    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!("直前の matches! で確認済み");
    };
    assert!(close(rows[0].boxes[0].dx, 57.0), "先頭行は右段（55 + padding 2）: {:?}", rows[0].boxes[0]);
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!(close(pages[0].anchors[0].x, 55.0), "右段のオフセット: {:?}", pages[0].anchors[0]);
    assert!(close(pages[0].anchors[0].y, 10.0), "右段の先頭: {:?}", pages[0].anchors[0]);
  }

  #[test]
  fn pending_anchor_before_unbreakable_table_moves_with_the_whole_table() {
    // 分割禁止の表（高さ 20）は y=46 に収まらず、先読みで表ごと次ページへ送られる。
    // アンカーは 2 ページ目の表先頭で解決する
    let geom = test_geometry();
    let table = TableBox {
      breakable: false,
      ..table_box(Vec::new(), vec![table_row("R0"), table_row("R1")])
    };
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Anchor(AnchorId::Label(LabelId::new("tab:x"))),
      Block::Table {
        table,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("R0"));
    assert!(pages[0].anchors.is_empty(), "{:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!(close(pages[1].anchors[0].x, 0.0), "{:?}", pages[1].anchors[0]);
    assert!(close(pages[1].anchors[0].y, 10.0), "{:?}", pages[1].anchors[0]);
  }

  #[test]
  fn pending_anchor_survives_suppressed_blank_page() {
    // アンカーの後ろに強制改ページが 2 連続。1 つ目で 1 ページ目が確定し、2 つ目は内容が無いので
    // 白紙ページを作らない。その間アンカーは未解決のまま保持され、次の段落の先頭で 2 ページ目に解決する
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::Anchor(AnchorId::Label(LabelId::new("x"))),
      Block::force_break(),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].anchors.is_empty(), "{:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!(close(pages[1].anchors[0].y, 2.0), "{:?}", pages[1].anchors[0]);
  }

  /// 幅 `width`・高さ `height`・深さ `depth` の、テキストを持たない箱
  fn wide_box(width: f32, height: f32, depth: f32) -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Atom(Vec::new()),
      width: pt(width),
      height: pt(height),
      depth: pt(depth),
    });
  }

  /// 箱 1 つだけの左揃え段落（[`paragraph_origin`] に帰属する）
  fn one_box_paragraph(item: HItem) -> Block {
    return Block::Paragraph {
      origin: Some(paragraph_origin()),
      items: vec![item],
      leading: pt(12.0),
      indent: pt(0.0),
      right_indent: pt(0.0),
      alignment: TextAlignment::Left,
    };
  }

  /// 抜粋が空（テキストを持たない箱）で `origin` に帰属する行のはみ出し記録
  fn line_overflow(page_index: usize, axis: OverflowAxis, excess: f32, origin: Option<NodeId>) -> Overflow {
    return Overflow {
      page_index,
      kind: OverflowKind::Line {
        origin,
        excerpt: String::new(),
        axis,
        excess: pt(excess),
      },
    };
  }

  #[test]
  fn line_wider_than_column_is_recorded_once_with_its_excess() {
    let blocks = vec![one_box_paragraph(wide_box(130.0, 8.0, 2.0))];

    let (_, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(
      overflows,
      vec![line_overflow(
        0,
        OverflowAxis::Horizontal,
        30.0,
        Some(paragraph_origin())
      )]
    );
  }

  #[test]
  fn overflow_within_tolerance_is_not_recorded() {
    for width in [100.0, 100.05] {
      let blocks = vec![one_box_paragraph(wide_box(width, 8.0, 2.0))];

      let (_, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

      assert!(overflows.is_empty(), "許容量 0.1pt 以下は記録しないはず: width={width} {overflows:?}");
    }
  }

  #[test]
  fn line_wider_than_its_column_is_recorded_in_two_columns() {
    // 段幅 45。本文幅 100 には収まる
    let blocks = vec![one_box_paragraph(wide_box(50.0, 8.0, 2.0))];

    let (_, overflows) = break_pages(blocks, pt(100.0), &two_column_geometry(), &GreedyBreaker);

    assert_eq!(
      overflows,
      vec![line_overflow(
        0,
        OverflowAxis::Horizontal,
        5.0,
        Some(paragraph_origin())
      )]
    );
  }

  #[test]
  fn composed_line_wider_than_column_is_recorded() {
    let line = Line {
      boxes: vec![PlacedHBox {
        hbox: HBox {
          content: HBoxContent::Atom(Vec::new()),
          width: pt(120.0),
          height: pt(8.0),
          depth: pt(2.0),
        },
        dx: Length::ZERO,
        dy: Length::ZERO,
      }],
      ..test_line()
    };
    // 目次の行のように元の要素を持つ合成行は、その要素を指す
    let blocks = vec![
      Block::ComposedLine {
        origin: Some(paragraph_origin()),
        line: line.clone(),
        leading: pt(12.0),
      },
      Block::ComposedLine {
        origin: None,
        line,
        leading: pt(12.0),
      },
    ];

    let (_, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(
      overflows,
      vec![
        line_overflow(0, OverflowAxis::Horizontal, 20.0, Some(paragraph_origin())),
        line_overflow(0, OverflowAxis::Horizontal, 20.0, None),
      ]
    );
  }

  #[test]
  fn line_deeper_than_region_is_recorded_as_vertical() {
    // ベースライン 10 + 深さ 45 = 55 が page_limit 50 を超える。送っても改善しないのでその場に置く
    let blocks = vec![one_box_paragraph(wide_box(10.0, 8.0, 45.0))];

    let (pages, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(
      overflows,
      vec![line_overflow(
        0,
        OverflowAxis::Vertical,
        5.0,
        Some(paragraph_origin())
      )]
    );
  }

  #[test]
  fn footnote_body_line_overflow_points_at_the_footnote() {
    // 本文段落ではなく、はみ出した本体行を持つ脚注を指す。記録はその脚注が着地したページで 1 回
    let footnote = footnote_item(1, vec![wide_box(130.0, 8.0, 2.0)], pt(12.0));
    let mut paragraph = single_line_paragraph(vec![footnote]);
    if let Block::Paragraph { origin, .. } = &mut paragraph {
      *origin = Some(paragraph_origin());
    }

    let (_, overflows) = break_pages(vec![paragraph], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(
      overflows,
      vec![line_overflow(
        0,
        OverflowAxis::Horizontal,
        30.0,
        Some(footnote_origin(1))
      )]
    );
  }

  #[test]
  fn carried_footnote_line_is_recorded_on_the_page_it_lands_on() {
    // 4 行の脚注のうち 3 行が 1 ページ目、幅 130 の最終行が 2 ページ目へ繰り越される
    let body = vec![
      test_box(),
      HItem::ForcedBreak,
      test_box(),
      HItem::ForcedBreak,
      test_box(),
      HItem::ForcedBreak,
      wide_box(130.0, 8.0, 2.0),
    ];
    let blocks = vec![
      single_line_paragraph(vec![footnote_item(1, body, pt(12.0))]),
      single_line_paragraph(vec![]),
    ];

    let (pages, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(footnote_layout(&pages), vec![(1, vec![(1, false, 3)]), (1, vec![(1, true, 1)])], "{pages:?}");
    assert_eq!(
      overflows,
      vec![line_overflow(
        1,
        OverflowAxis::Horizontal,
        30.0,
        Some(footnote_origin(1))
      )]
    );
  }

  /// 幅 `width`・高さ `height` の左揃え画像（`x.png`）
  fn image_block(width: f32, height: f32) -> Block {
    return Block::Image {
      origin: image_origin(),
      path: ProjectPath::new("x.png"),
      width: pt(width),
      height: pt(height),
      target_dpi: None,
      align: Align::Left,
    };
  }

  /// `x.png` のはみ出し記録
  fn image_overflow(page_index: usize, axis: OverflowAxis, excess: f32) -> Overflow {
    return Overflow {
      page_index,
      kind: OverflowKind::Image {
        origin: image_origin(),
        path: ProjectPath::new("x.png"),
        axis,
        excess: pt(excess),
      },
    };
  }

  /// 本体が幅 `width`・高さ `height`・深さ `depth` の中央揃えディスプレイ数式（番号なし）
  fn math_block(width: f32, height: f32, depth: f32) -> Block {
    return Block::Math {
      origin: math_origin(),
      body: HBox {
        content: HBoxContent::Atom(Vec::new()),
        width: pt(width),
        height: pt(height),
        depth: pt(depth),
      },
      numbers: Vec::new(),
      numbers_on_right: true,
      align: Align::Center,
    };
  }

  /// 抜粋が空（テキストを持たない本体）のディスプレイ数式のはみ出し記録
  fn math_overflow(page_index: usize, axis: OverflowAxis, excess: f32) -> Overflow {
    return Overflow {
      page_index,
      kind: OverflowKind::Math {
        origin: math_origin(),
        excerpt: String::new(),
        axis,
        excess: pt(excess),
      },
    };
  }

  #[test]
  fn image_taller_than_region_is_recorded_once() {
    // 先頭 y=10 に置かれ下端 70 が page_limit 50 を超える
    let (pages, overflows) = break_pages(vec![image_block(20.0, 60.0)], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(overflows, vec![image_overflow(0, OverflowAxis::Vertical, 20.0)]);
  }

  #[test]
  fn image_overflowing_both_axes_is_recorded_horizontal_then_vertical() {
    let (_, overflows) = break_pages(vec![image_block(130.0, 60.0)], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(
      overflows,
      vec![
        image_overflow(0, OverflowAxis::Horizontal, 30.0),
        image_overflow(0, OverflowAxis::Vertical, 20.0)
      ]
    );
  }

  #[test]
  fn image_wider_than_its_column_is_recorded_in_two_columns() {
    // 段幅 45。本文幅 100 には収まる
    let (_, overflows) = break_pages(vec![image_block(50.0, 20.0)], pt(100.0), &two_column_geometry(), &GreedyBreaker);

    assert_eq!(overflows, vec![image_overflow(0, OverflowAxis::Horizontal, 5.0)]);
  }

  #[test]
  fn math_block_wider_than_column_is_recorded() {
    let (_, overflows) = break_pages(vec![math_block(130.0, 8.0, 2.0)], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(overflows, vec![math_overflow(0, OverflowAxis::Horizontal, 30.0)]);
  }

  #[test]
  fn math_block_taller_than_region_is_recorded() {
    // 先頭 top=10 で下端 70。先頭からでも収まらないので送らずその場に置く
    let (_, overflows) = break_pages(vec![math_block(20.0, 50.0, 10.0)], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(overflows, vec![math_overflow(0, OverflowAxis::Vertical, 20.0)]);
  }

  #[test]
  fn fitting_blocks_record_nothing() {
    // 画像は次ページへ送られて収まり、数式は下端がちょうど page_limit に乗る
    let blocks = vec![
      paragraph_of_lines(3),
      image_block(20.0, 30.0),
      math_block(20.0, 8.0, 2.0),
    ];

    let (pages, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(overflows.is_empty(), "{overflows:?}");
  }

  #[test]
  fn line_in_the_right_column_is_measured_against_its_column() {
    // 左段を 4 行で埋め、箱 1 つの段落を右段（段オフセット 55・段幅 45）へ送る
    for (width, expected) in [
      (44.0, vec![]),
      (
        50.0,
        vec![line_overflow(
          0,
          OverflowAxis::Horizontal,
          5.0,
          Some(paragraph_origin()),
        )],
      ),
    ] {
      let blocks = vec![
        paragraph_of_lines(4),
        one_box_paragraph(wide_box(width, 8.0, 2.0)),
      ];

      let (pages, overflows) = break_pages(blocks, pt(100.0), &two_column_geometry(), &GreedyBreaker);

      assert_eq!(pages.len(), 1, "{pages:?}");
      assert_eq!(overflows, expected, "width={width}");
    }
  }

  /// 表の行断片に脚注（[`footnote_item`] / [`footnote_of_lines`] が作るマーカー）を足すヘルパ
  fn with_footnote(mut row: TableRowFragment, footnote: HItem) -> TableRowFragment {
    let HItem::Footnote(footnote) = footnote else {
      panic!("脚注マーカーを渡すこと: {footnote:?}");
    };
    row.footnotes.push(footnote);
    return row;
  }

  /// ページの脚注断片の `(出現 index, 繰越か)` 列
  fn table_footnote_indices(page: &Page) -> Vec<(u32, bool)> {
    return page.footnotes.iter().map(|footnote| return (footnote.index, footnote.continued)).collect();
  }

  /// ページに置かれた表の行数（表ブロックをまたいだ合計）
  fn table_rows_of(page: &Page) -> usize {
    return page
      .blocks
      .iter()
      .map(|block| match block {
        PlacedBlock::Table { rows } => return rows.len(),
        _ => return 0,
      })
      .sum();
  }

  /// 本文行の表（ヘッダなし・分割可能）1 個のブロック
  fn table_block(rows: Vec<TableRowFragment>) -> Block {
    return Block::Table {
      table: table_box(Vec::new(), rows),
      align: Align::Left,
    };
  }

  /// 本体 1 行（高さ 10。`test_geometry` の予約は 4 + 10 = 14）の脚注マーカー
  fn one_line_footnote(number: u32) -> HItem { return footnote_item(number, vec![test_box()], pt(12.0)); }

  #[test]
  fn table_row_footnote_lands_below_the_table_in_the_row_page() {
    let geom = test_geometry();
    let blocks = vec![table_block(vec![with_footnote(
      table_row("A"),
      one_line_footnote(1),
    )])];

    let (pages, overflows) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(table_footnote_indices(&pages[0]), vec![(0, false)]);
    assert!(footnote_baselines(&pages[0], 1)[0] > pt(20.0), "脚注は表の行（10..20）より下");
    assert!(overflows.is_empty(), "{overflows:?}");
  }

  #[test]
  fn table_row_with_footnote_moves_to_next_page_without_leaving_reservation() {
    // 3 行で y = 40。4 行目は単独なら 40..50 に収まるが、脚注の予約 14 を足すと収まらない
    let geom = test_geometry();
    let mut rows: Vec<TableRowFragment> = (0..3).map(|i| return table_row(&format!("R{i}"))).collect();
    rows.push(with_footnote(table_row("R3"), one_line_footnote(1)));

    let (pages, _) = break_pages(vec![table_block(rows)], pt(100.0), &geom, &GreedyBreaker);

    assert!(pages[0].footnotes.is_empty(), "元のページに脚注エリアを残さない");
    assert_eq!(table_rows_of(&pages[0]), 3);
    assert_eq!(table_rows_of(&pages[1]), 1);
    assert_eq!(table_footnote_indices(&pages[1]), vec![(0, false)]);
  }

  #[test]
  fn footnotes_in_one_table_row_stack_in_source_order() {
    let geom = test_geometry();
    let row = with_footnote(with_footnote(table_row("A"), one_line_footnote(1)), one_line_footnote(2));

    let (pages, _) = break_pages(vec![table_block(vec![row])], pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(table_footnote_indices(&pages[0]), vec![(0, false), (1, false)]);
    assert!(footnote_baselines(&pages[0], 2)[0] > footnote_baselines(&pages[0], 1)[0], "出現順に積む");
  }

  #[test]
  fn long_table_row_footnote_carries_to_next_page() {
    let geom = test_geometry();
    let row = with_footnote(table_row("A"), footnote_of_lines(1, 4));

    let (pages, _) = break_pages(vec![table_block(vec![row])], pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(table_footnote_indices(&pages[0]), vec![(0, false)]);
    assert_eq!(table_footnote_indices(&pages[1]), vec![(0, true)]);
    assert_no_body_footnote_overlap(&pages);
  }

  #[test]
  fn head_footnote_is_placed_once_on_the_first_head_page() {
    // ヘッダ 1 行（脚注付き）+ 本文 6 行で 2 ページにまたがる
    let geom = test_geometry();
    let head = vec![with_footnote(table_row("H"), one_line_footnote(1))];
    let rows = (0..6).map(|i| return table_row(&format!("R{i}"))).collect();
    let blocks = vec![Block::Table {
      table: table_box(head, rows),
      align: Align::Left,
    }];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert!(pages.len() >= 2, "表が改ページするはず（空振り検知）: {pages:?}");
    let placed: Vec<usize> = pages
      .iter()
      .enumerate()
      .filter(|(_, page)| return !page.footnotes.is_empty())
      .map(|(index, _)| return index)
      .collect();
    assert_eq!(placed, vec![0], "本体は初出ヘッダのページにだけ出る");
  }

  #[test]
  fn head_footnote_moves_with_the_head_to_next_page() {
    // 3 行の表で y = 40。次の表のヘッダ行は単独なら 40..50 に収まるが、脚注の予約 14 を足すと収まらない
    let geom = test_geometry();
    let filler = (0..3).map(|i| return table_row(&format!("F{i}"))).collect();
    let head = vec![with_footnote(table_row("H"), one_line_footnote(1))];
    let blocks = vec![
      table_block(filler),
      Block::Table {
        table: table_box(head, vec![table_row("R0")]),
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert!(pages[0].footnotes.is_empty(), "{:?}", pages[0].footnotes);
    assert_eq!(table_rows_of(&pages[0]), 3);
    assert_eq!(table_footnote_indices(&pages[1]), vec![(0, false)]);
  }

  #[test]
  fn table_row_footnote_overflow_is_reported_like_a_body_line() {
    // 空のページでも行 10 + 予約（4 + 本体 52）が page_limit 50 を超える
    let geom = test_geometry();
    let row = with_footnote(table_row("A"), footnote_item(1, vec![tall_box(50.0)], pt(12.0)));

    let (pages, overflows) = break_pages(vec![table_block(vec![row])], pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(table_footnote_indices(&pages[0]), vec![(0, false)], "はみ出したまま置く");
    let expected = OverflowKind::FootnoteLine {
      footnotes: vec![(1, footnote_origin(1))],
    };
    assert!(overflows.iter().any(|overflow| return overflow.kind == expected), "{overflows:?}");
  }

  #[test]
  fn heading_moves_with_table_whose_first_row_footnote_does_not_fit() {
    // 1 行目（baseline 10）の後、見出し役の 1 行段落（baseline 22）→ 分割禁止 → 表。
    // 表の先頭行は y = 34 から 44 で単独なら収まるが、予約 14 を足すと収まらない
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(Vec::new()),
      single_line_paragraph(Vec::new()),
      forbid_break(),
      table_block(vec![with_footnote(table_row("A"), one_line_footnote(1))]),
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(page_baselines(&pages[0]).len(), 1, "見出し役の行は表と一緒に次ページへ");
    assert_eq!(page_baselines(&pages[1]).len(), 1);
    assert_eq!(table_rows_of(&pages[1]), 1);
    assert_eq!(table_footnote_indices(&pages[1]), vec![(0, false)]);
  }

  #[test]
  fn unbreakable_table_defers_whole_when_its_footnotes_do_not_fit() {
    // 1 行段落の後 y = 22。分割禁止の 2 行表（高さ 20）は単独なら 42 で収まるが、予約 14 を足すと収まらない
    let geom = test_geometry();
    let rows = vec![
      table_row("R0"),
      with_footnote(table_row("R1"), one_line_footnote(1)),
    ];
    let table = TableBox {
      breakable: false,
      ..table_box(Vec::new(), rows)
    };
    let blocks = vec![
      single_line_paragraph(Vec::new()),
      Block::Table {
        table,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(table_rows_of(&pages[0]), 0, "表は途中で割れずに丸ごと送られる");
    assert_eq!(table_rows_of(&pages[1]), 2);
    assert_eq!(table_footnote_indices(&pages[1]), vec![(0, false)]);
  }

  #[test]
  fn table_row_after_carry_does_not_overlap_carried_footnote() {
    // 1 行段落の 4 行脚注がページ 0 で分割され、残りがページ 1 の脚注エリア先頭を占める。
    // 続く表の脚注付きの行は、繰越と自分の脚注が同じリージョンに入らなければさらに送られる
    let geom = test_geometry();
    let rows = (0..4u32)
      .map(|i| return with_footnote(table_row(&format!("R{i}")), footnote_of_lines(i + 2, 1)))
      .collect();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      table_block(rows),
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert!(
      pages.iter().any(|page| return page.footnotes.iter().any(|footnote| return footnote.continued)),
      "繰越が起きるはず（空振り検知）: {:?}",
      footnote_layout(&pages)
    );
    assert_eq!(pages.iter().map(table_rows_of).sum::<usize>(), 4);
    assert_no_body_footnote_overlap(&pages);
  }

  #[test]
  fn table_row_footnote_lands_in_the_column_of_its_row() {
    // 2 段（段幅 45・右段オフセット 55）。左段に 3 行 → 4 行目（脚注付き）は右段へ
    let geom = two_column_geometry();
    let mut rows: Vec<TableRowFragment> = (0..3).map(|i| return table_row(&format!("R{i}"))).collect();
    rows.push(with_footnote(table_row("R3"), one_line_footnote(1)));

    let (pages, _) = break_pages(vec![table_block(rows)], pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    let footnote = footnote_numbered(&pages[0], 1).expect("右段の脚注エリアに置かれるはず");
    let xs: Vec<Length> = footnote
      .blocks
      .iter()
      .filter_map(|block| match block {
        PlacedBlock::Line { line, .. } => return line.boxes.first().map(|placed| return placed.dx),
        _ => return None,
      })
      .collect();
    assert_eq!(xs, vec![pt(55.0)], "本体の行は右段の左端から");
  }

  #[test]
  fn table_footnote_coexists_with_flush_bottom() {
    let geom = flush_geometry();
    let rows = (0..6u32)
      .map(|i| return with_footnote(table_row(&format!("R{i}")), footnote_of_lines(i + 1, 1)))
      .collect();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::Glue {
        natural: Length::ZERO,
        stretch: pt(10.0),
      },
      table_block(rows),
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert!(pages.len() >= 2, "満杯で閉じるリージョンがあるはず（空振り検知）: {pages:?}");
    assert_eq!(pages.iter().map(|page| return page.footnotes.len()).sum::<usize>(), 6);
    assert_no_body_footnote_overlap(&pages);
  }

  #[test]
  fn repeated_head_is_not_stacked_onto_a_region_filled_by_carry() {
    // R0 の 8 行脚注がページ 0 で分割され、残りが後続ページの脚注エリアを埋める。再掲ヘッダは R1 と一緒に
    // 入るリージョンにだけ積み、繰越で埋まったリージョンにヘッダだけを置いて脚注と重ねない
    let geom = test_geometry();
    let head = vec![table_row("H")];
    let rows = vec![
      with_footnote(table_row("R0"), footnote_of_lines(1, 8)),
      table_row("R1"),
    ];
    let blocks = vec![Block::Table {
      table: table_box(head, rows),
      align: Align::Left,
    }];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert!(
      pages.iter().any(|page| return page.footnotes.iter().any(|footnote| return footnote.continued)),
      "繰越が起きるはず（空振り検知）"
    );
    assert_no_body_footnote_overlap(&pages);
    let rows_per_page: Vec<usize> = pages.iter().map(table_rows_of).collect();
    assert!(
      rows_per_page.iter().all(|&rows| return rows != 1),
      "ヘッダだけのページを作らない: {rows_per_page:?}"
    );
  }

  #[test]
  fn unbreakable_table_moves_whole_past_a_page_with_footnote_reservation() {
    // 1 行段落の脚注で予約 14 のページに、分割禁止の 2 行表（2 行目に脚注）。ここには入らないが、予約の無い
    // 新しいページなら 10 + 20 + 14 = 44 ≤ 50 で丸ごと入る
    let geom = test_geometry();
    let rows = vec![
      table_row("R0"),
      with_footnote(table_row("R1"), one_line_footnote(2)),
    ];
    let table = TableBox {
      breakable: false,
      ..table_box(Vec::new(), rows)
    };
    let blocks = vec![
      single_line_paragraph(vec![one_line_footnote(1)]),
      Block::Table {
        table,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(table_rows_of(&pages[0]), 0, "表は途中で割れずに丸ごと送られる");
    assert_eq!(table_rows_of(&pages[1]), 2);
  }

  /// 表の記録だけを取り出すヘルパ（表の行と表自体の記録）
  fn table_overflows(overflows: &[Overflow]) -> Vec<&Overflow> {
    return overflows
      .iter()
      .filter(|overflow| {
        return matches!(overflow.kind, OverflowKind::Table { .. } | OverflowKind::TableRow { .. });
      })
      .collect();
  }

  #[test]
  fn wide_table_is_reported_once_on_the_page_of_its_first_row() {
    // 版面の高さ 40 に 4 行入るので 5 行表は 2 ページにまたがる。表幅 130 は段幅 100 を 30 超える
    let table = TableBox {
      width: pt(130.0),
      ..table_box(Vec::new(), (0..5).map(|i| return table_row(&format!("R{i}"))).collect())
    };
    let blocks = vec![Block::Table {
      table,
      align: Align::Left,
    }];

    let (pages, overflows) = break_pages(blocks, pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(
      overflows,
      vec![Overflow {
        page_index: 0,
        kind: OverflowKind::Table {
          origin: table_origin(),
          excess: pt(30.0),
        },
      }]
    );
  }

  #[test]
  fn row_overrun_is_reported_on_the_page_where_the_row_lands() {
    let mut rows: Vec<TableRowFragment> = (0..5).map(|i| return table_row(&format!("R{i}"))).collect();
    rows[4].overrun = pt(5.0);

    let (pages, overflows) = break_pages(vec![table_block(rows)], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(
      overflows,
      vec![Overflow {
        page_index: 1,
        kind: OverflowKind::TableRow {
          origin: table_row_origin(),
          axis: OverflowAxis::Horizontal,
          excess: pt(5.0),
        },
      }]
    );
  }

  #[test]
  fn row_taller_than_an_empty_region_is_reported_vertically() {
    // 行帯 60 は版面（10..50）の高さ 40 に入らない。下端 70 が下限 50 を 20 超える
    let tall = table_fragment(table_text_box("T"), 60.0, 10.0);

    let (_, overflows) = break_pages(vec![table_block(vec![tall])], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert_eq!(
      overflows,
      vec![Overflow {
        page_index: 0,
        kind: OverflowKind::TableRow {
          origin: table_row_origin(),
          axis: OverflowAxis::Vertical,
          excess: pt(20.0),
        },
      }]
    );
  }

  #[test]
  fn row_taller_than_an_empty_region_stays_on_the_page_of_its_head() {
    // 行帯 60 は再掲ヘッダの下（20..）どころか空の版面（10..50）にも入らないので、送らずヘッダ（10..20）の
    // 直下に置く。下端 80 が下限 50 を 30 超える
    let tall = table_fragment(table_text_box("T"), 60.0, 10.0);
    let table = table_box(vec![table_row("HEAD")], vec![tall]);

    let (pages, overflows) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      pt(100.0),
      &test_geometry(),
      &GreedyBreaker,
    );

    assert_eq!(pages.len(), 1, "ヘッダだけのページを作らない: {pages:?}");
    assert_eq!(table_rows_of(&pages[0]), 2);
    assert_eq!(
      overflows,
      vec![Overflow {
        page_index: 0,
        kind: OverflowKind::TableRow {
          origin: table_row_origin(),
          axis: OverflowAxis::Vertical,
          excess: pt(30.0),
        },
      }]
    );
  }

  #[test]
  fn heading_stays_with_table_whose_first_row_is_taller_than_an_empty_region() {
    // 見出し役の行（baseline 22）の後 y = 34。先頭行（行帯 60）は空の版面にも入らないので実配置は送らず、
    // keep-with-next の計画も見出しを送らない
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(Vec::new()),
      single_line_paragraph(Vec::new()),
      forbid_break(),
      table_block(vec![table_fragment(table_text_box("T"), 60.0, 10.0)]),
    ];

    let (pages, _) = break_pages(blocks, pt(100.0), &geom, &GreedyBreaker);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]).len(), 2);
    assert_eq!(table_rows_of(&pages[0]), 1);
  }

  #[test]
  fn fitting_table_reports_nothing() {
    let rows = (0..5).map(|i| return table_row(&format!("R{i}"))).collect();

    let (_, overflows) = break_pages(vec![table_block(rows)], pt(100.0), &test_geometry(), &GreedyBreaker);

    assert!(table_overflows(&overflows).is_empty(), "{overflows:?}");
  }
}
