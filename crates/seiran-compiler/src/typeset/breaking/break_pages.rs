//! (d) 縦組版 — ブロック列をページへ配置する

use tracing::debug;

use super::break_lines::LineBreaker;
use crate::{
  length::Length,
  style::TextAlignment,
  typeset::{
    boxes::{
      Align, AnchorMark, Block, FootnoteId, HBox, HItem, Line, MathRowNumber, PENALTY_FORBID_BREAK,
      PENALTY_FORCE_BREAK, Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedIndexEntry, PlacedLink,
      PlacedMathNumber, PlacedTableRow, PlacedTableRule, TableBox, TableRowBox, collect_row_links,
      max_font_size_in_items, position_table_row_boxes, resolve_column_widths, table_row_height,
    },
    geometry::column_width,
  },
};

mod footnote_packing;

use footnote_packing::{
  FootnoteCharges, FootnoteDemand, LineFootnoteFit, fit_line_footnotes, footnote_area_full, pack_footnotes,
  split_pending,
};

/// ページの物理ジオメトリと既定の行送りパラメータ
#[derive(Debug, Clone, Copy)]
pub struct PageGeometry {
  /// 本文の水平原点（pt）= 用紙左端から本文左端まで（`style.page.margin_left`）。
  ///
  /// ページ内の確定座標は本文左端からの相対値なので、この値は組版では使わず
  /// [`crate::typeset::Page::content_origin_x`] へそのまま載せて描画側の加算に使わせる。
  pub content_origin_x: Length,
  /// 上マージン（pt）。ページ先頭のベースライン位置
  pub margin_top: Length,
  /// 本文下限（pt）= ページ高さ − 下マージン。超えると改ページ（または改段）
  pub page_limit: Length,
  /// 既定フォントサイズ（pt）。表の行高のフォールバックに使用
  pub default_font_size: Length,
  /// 行高係数。表の行高の算出に使用
  pub line_height_factor: f32,
  /// 表セルの内側余白（pt、左右各）。列幅の解決に使用
  pub table_cell_padding: Length,
  /// 段組み数（1 = 単段）。本文を左段 → 右段 → 次ページの順に流す段の本数
  pub num_columns: usize,
  /// 段間（gutter、pt）。隣り合う段の間隔
  pub column_gap: Length,
  /// 下端揃え（flush bottom）を有効にするか（`style.toml` の `[page] flush_bottom`）。
  pub flush_bottom: bool,
  /// 脚注: 本文と区切り罫線の間隔（`style.footnote.top_margin`）
  pub footnote_top_margin: Length,
  /// 脚注: 区切り罫線の長さ（`style.footnote.rule_length`）
  pub footnote_rule_length: Length,
  /// 脚注: 区切り罫線の太さ（0 のとき描画しない、`style.footnote.rule_thickness`）
  pub footnote_rule_thickness: Length,
  /// 脚注: 区切り罫線の色（RGB）。`None` は黒。呼び出し側が `crate::color::Color::rgb()` で
  /// 変換済みの値を渡す（`RunningSlots.rule_color` と同じ規約）
  pub footnote_rule_color: Option<[u8; 3]>,
  /// 脚注: 区切り罫線〜最初の脚注、および脚注どうしの間隔（`style.footnote.rule_gap`）
  pub footnote_rule_gap: Length,
  /// 表: 罫線の太さ（0 のとき描画しない、`style.table.rule_thickness`）
  pub table_rule_thickness: Length,
  /// 表: 罫線の色（RGB）。`None` は黒。呼び出し側が `crate::color::Color::rgb()` で
  /// 変換済みの値を渡す（`footnote_rule_color` と同じ規約）
  pub table_rule_color: Option<[u8; 3]>,
  /// ページ背景色（RGB）。`None` は塗りつぶさない（`style.background_color`）。
  /// 呼び出し側が `crate::color::Color::rgb()` で変換済みの値を渡す
  pub background_color: Option<[u8; 3]>,
}

/// 脚注がリージョンに収まらないまま配置された事実（#382）。
///
/// 診断そのものではなく純データで、ページの指し方も**この [`break_pages`] 呼び出しが返すページ列の
/// 中での index**。前付け・本文・後付けを連結した物理ページ番号や印字ラベルは
/// `typeset::pagination` が確定させる（この段は自分が組んだページ列しか知らないため）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FootnoteOverflow {
  /// はみ出しが起きたページの 0 起点 index（返されるページ列の中での位置）
  pub page_index: usize,
  /// はみ出し方
  pub kind: FootnoteOverflowKind,
}

/// [`FootnoteOverflow`] のはみ出し方
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FootnoteOverflowKind {
  /// 1 行に付いた脚注群が、空のリージョンでもページ全高に収まらなかった（表示番号は出現順）
  Line {
    /// はみ出した脚注群の表示番号
    numbers: Vec<u32>,
  },
  /// 繰越脚注の先頭 1 行がページ全高を超えた
  SingleLine {
    /// はみ出した脚注の表示番号
    number: u32,
  },
}

/// 縦組版の内部状態（現在ページ・カーソル）
struct PageComposer {
  /// 確定済みページ
  pages: Vec<Page>,
  /// 現在ページに配置済みのブロック
  current: Vec<PlacedBlock>,
  /// 現在ページに解決済みのリンク到達先アンカー（機構 A）
  current_anchors: Vec<PlacedAnchor>,
  /// 現在ページに確定済みのクリック可能リンク領域（機構 B）
  current_links: Vec<PlacedLink>,
  /// 現在ページに確定済みの索引語（重複除去済み、出現順）
  current_index_entries: Vec<PlacedIndexEntry>,
  /// 未解決のアンカー。次に配置される実ブロックの確定座標で解決する
  pending_anchors: Vec<AnchorMark>,
  /// カーソル位置（ページ上端からの距離、pt）。基本は「次のベースライン位置」
  y: Length,
  /// 直前のブロックが底辺基準（画像・表・罫線）で終わったか
  cursor_at_edge: bool,
  /// 段組み数（1 = 単段）
  num_columns: usize,
  /// 1 段あたりの幅（pt）。行分割・揃え・表の列幅解決に使う
  column_width: Length,
  /// 段間（gutter、pt）
  column_gap: Length,
  /// 現在の段インデックス（0 = 左段）。`column_offset` の算出に使う
  col: usize,
  /// 直前の [`Block::Penalty`] から引き継いだ分割コスト（次の内容ブロックの改ページ判定で参照）
  pending_penalty: i32,
  /// 現在リージョン（段）の先頭 index（`current` 内）。下端揃え（#169）がここから末尾までを揃える。
  region_start: usize,
  /// 現在リージョンの先頭 index（`current_links` 内）。下端揃えでリンク矩形を本体と同量シフトする。
  region_link_start: usize,
  /// 現在リージョンの先頭 index（`current_anchors` 内）。下端揃えでアンカーを本体と同量シフトする。
  region_anchor_start: usize,
  /// 現在リージョンの伸縮アキ（下端揃え用）。各アキ通過時点の `current` / `current_links` /
  /// `current_anchors` の要素数（＝そのアキより後に来る要素の開始 index）と stretch を記録し、
  /// [`PageComposer::end_region`] が不足高さを配置順ベースで比例配分する。
  region_glues: Vec<GlueMark>,
  /// 現在リージョン（段）に集約された脚注（出現順、行分割済み）。[`PageComposer::end_region`] が
  /// ページ下部の確定座標へ変換して `page_footnotes` へ移す。
  region_footnotes: Vec<PendingFootnote>,
  /// 現在リージョンの脚注が占有する高さ（pt、脚注間・本文とのアキ込み）。0 は脚注なし。
  /// [`PageComposer::region_limit`] が本文の実効下限からこの分を差し引く。
  region_footnote_height: Length,
  /// 現在ページに確定済みの脚注（複数リージョンにまたがり得る）。
  /// ページ確定時（[`PageComposer::start_new_page`] / [`PageComposer::finish`]）に `Page::footnotes` へ渡す。
  page_footnotes: Vec<PlacedFootnote>,
  /// 次リージョンへ繰り越す脚注の残り（#227、出現順）。
  carry: Vec<PendingFootnote>,
  /// 収まらないまま配置した脚注の記録（#382、検出順＝ページ順）。
  /// 純粋関数（[`place_lines`] / [`pack_footnotes`]）が返した「はみ出した」という事実に、
  /// ページ index と脚注番号を添えるのはページを組んでいるこの型の責務。
  overflows: Vec<FootnoteOverflow>,
}

/// 現在リージョンに集約された脚注 1 個（行分割済み、未確定座標）
struct PendingFootnote {
  /// 発番済みの表示番号
  number: u32,
  /// 出現順の識別子（0 起点。`PlacedFootnote` へ素通しする）
  index: u32,
  /// 前リージョンからの繰越（続き）か。`PlacedFootnote` へ素通しする
  continued: bool,
  /// 行分割済みの本体（このリージョンに置く分だけに切り詰め済み）
  lines: Vec<Line>,
  /// 行送り（[`FootnoteDemand::new`] の見積り / 確定配置の両方が使う）
  leading: Length,
}

/// 下端揃え（#169）用の伸縮アキ記録
struct GlueMark {
  /// 伸長能力（pt）。配分の重み
  stretch: Length,
  /// このアキ記録時点の `current` の要素数（このアキより後のブロックはこの index 以降）
  block_at: usize,
  /// このアキ記録時点の `current_links` の要素数
  link_at: usize,
  /// このアキ記録時点の `current_anchors` の要素数
  anchor_at: usize,
}

impl PageComposer {
  /// 先頭ページの初期状態で `PageComposer` を生成する
  fn new(geom: &PageGeometry, column_width: Length) -> Self {
    return PageComposer {
      pages: Vec::new(),
      current: Vec::new(),
      current_anchors: Vec::new(),
      current_links: Vec::new(),
      current_index_entries: Vec::new(),
      pending_anchors: Vec::new(),
      y: geom.margin_top,
      cursor_at_edge: false,
      num_columns: geom.num_columns.max(1),
      column_width,
      column_gap: geom.column_gap,
      col: 0,
      pending_penalty: 0,
      region_start: 0,
      region_link_start: 0,
      region_anchor_start: 0,
      region_glues: Vec::new(),
      region_footnotes: Vec::new(),
      region_footnote_height: Length::ZERO,
      page_footnotes: Vec::new(),
      carry: Vec::new(),
      overflows: Vec::new(),
    };
  }

  /// 現在リージョンの実効下限（pt）。脚注が占有する高さぶん `geom.page_limit` を縮める。
  fn region_limit(&self, geom: &PageGeometry) -> Length { return geom.page_limit - self.region_footnote_height; }

  /// 現在の段の左端 x オフセット（本文左端基準、pt）。段 `k` は `k * (段幅 + 段間)` だけ右へ寄る
  fn column_offset(&self) -> Length {
    // 段インデックスは実用上 0〜1。f32 で精度を失う桁数にはならない
    #[allow(clippy::cast_precision_loss)]
    let col = self.col as f32;
    return (self.column_width + self.column_gap) * col;
  }

  /// ページ下限を超えたときの遷移。次の段があれば改段、なければ改ページし、繰越脚注を新リージョンへ詰める。
  fn advance_region(&mut self, geom: &PageGeometry) {
    // 満杯になったリージョン（段）を先に確定する。下端揃え（#169）が有効なら不足高さを段内の
    // 伸縮アキへ配分してから次段 / 次ページへ移る。強制改ページ・最終ページはこの経路を通らない
    // （それぞれ `force_new_page`・`finish` が確定）ため揃えられない。
    self.end_region(geom, true);
    self.next_region(geom);
    self.seed_carry(geom);
  }

  /// 次のリージョン（段 / ページ）へ移るだけの遷移（リージョン確定・繰越の詰め込みは行わない）
  fn next_region(&mut self, geom: &PageGeometry) {
    if self.col + 1 < self.num_columns {
      self.col += 1;
      self.y = geom.margin_top;
      self.cursor_at_edge = false;
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
    // はみ出しの記録に使う繰越先頭の表示番号（`pack_footnotes` が `overflowed` を立てるのは
    // 先頭の脚注に限られる）。`carry` を取り出す前に控える
    let leading_number = self.carry[0].number;
    // 繰越は「そのページの自前の脚注より前」に置くので、常にエリア先頭（`base_reserved` = 0）から詰める。
    // `require_first_line = false` の詰め込みは最低 1 行を強制するので必ず成功する
    let packing = pack_footnotes(&demands, Length::ZERO, geom.page_limit - geom.margin_top, charges, false)
      .expect("繰越の詰め込みは先頭に最低 1 行を強制するので None にならない");
    if packing.overflowed {
      self.overflows.push(FootnoteOverflow {
        page_index: self.pages.len(),
        kind: FootnoteOverflowKind::SingleLine {
          number: leading_number,
        },
      });
    }
    self.region_footnote_height = packing.height;
    let mut rest = Vec::new();
    for (pending, &placed) in std::mem::take(&mut self.carry).into_iter().zip(&packing.splits) {
      let (head, tail) = split_pending(pending, placed);
      if let Some(head) = head {
        self.region_footnotes.push(head);
      }
      if let Some(tail) = tail {
        rest.push(tail);
      }
    }
    self.carry = rest;
  }

  /// 直前の [`Block::Penalty`] から引き継いだ分割コストを読み取ってリセットする。
  fn take_pending_penalty(&mut self) -> i32 { return std::mem::replace(&mut self.pending_penalty, 0); }

  /// ブロック配置前の改ページ判定（分割コスト参照の一本化ポイント）。
  fn consider_break(&mut self, next_height: Length, penalty: i32, geom: &PageGeometry) {
    if penalty == PENALTY_FORBID_BREAK {
      return;
    }
    if self.y + next_height > self.region_limit(geom) {
      self.advance_region(geom);
    }
  }

  /// 現在のリージョン（段 / ページ）の先頭にいて、これ以上前へは送れない（回避不能）かを返す。
  fn at_region_top(&self, geom: &PageGeometry) -> bool { return self.y <= geom.margin_top && !self.cursor_at_edge; }

  /// 現在ページを確定し、新しいページを開始する
  fn start_new_page(&mut self, geom: &PageGeometry) {
    // 強制改ページ（[`PENALTY_FORCE_BREAK`]）はこのメソッドを [`PageComposer::advance_region`] 経由せず
    // [`PageComposer::force_new_page`] から呼ぶため、現在リージョンに残っている脚注をここで必ず確定させる
    // （flush-bottom 対象ではないので `flush=false`。`advance_region` 経由の場合は既に確定済みで、
    // この呼び出しは無害な二重呼び出しになる）。
    self.end_region(geom, false);
    if self.current.is_empty() && self.page_footnotes.is_empty() {
      return;
    }
    self.pages.push(Page {
      blocks: std::mem::take(&mut self.current),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: std::mem::take(&mut self.page_footnotes),
      anchors: std::mem::take(&mut self.current_anchors),
      links: std::mem::take(&mut self.current_links),
      index_entries: std::mem::take(&mut self.current_index_entries),
      background_color: geom.background_color,
      content_origin_x: geom.content_origin_x,
    });
    self.y = geom.margin_top;
    self.cursor_at_edge = false;
    self.col = 0;
    // ページ確定で `current` / `current_links` / `current_anchors` を空にしたので、次ページ先頭段の
    // リージョン追跡を 0 から開始する。伸縮アキも新ページに持ち越さない。
    self.region_start = 0;
    self.region_link_start = 0;
    self.region_anchor_start = 0;
    self.region_glues.clear();
  }

  /// 未解決アンカーを確定座標 `(x, y)` で現在ページに解決する
  fn resolve_pending_anchors(&mut self, x: Length, y: Length) {
    for mark in self.pending_anchors.drain(..) {
      self.current_anchors.push(PlacedAnchor { mark, x, y });
    }
  }

  /// 行のリンク領域を確定座標へ展開し、現在ページに追加する
  fn collect_line_links(&mut self, line: &Line, baseline_y: Length) {
    let top = baseline_y - line.height;
    let height = line.height + line.depth;
    for link in &line.links {
      if link.x1 <= link.x0 {
        continue;
      }
      self.current_links.push(PlacedLink {
        target: link.target.clone(),
        x: link.x0,
        y: top,
        width: link.x1 - link.x0,
        height,
      });
    }
  }

  /// 行の索引語を現在ページの索引語集合へマージする
  fn collect_line_index_entries(&mut self, line: &Line) {
    for entry in &line.index_marks {
      let exists = self.current_index_entries.iter().any(|e| return e.word == entry.word && e.reading == entry.reading);
      if !exists {
        self.current_index_entries.push(PlacedIndexEntry {
          word: entry.word.clone(),
          reading: entry.reading.clone(),
        });
      }
    }
  }

  /// 全ブロックの配置後に最終ページを確定し、ページ列と脚注のはみ出し記録を返す
  fn finish(mut self, geom: &PageGeometry) -> (Vec<Page>, Vec<FootnoteOverflow>) {
    // 末尾に残った未解決アンカーは現在カーソル位置（現在の段の左端）で解決する
    let y = self.y;
    let x = self.column_offset();
    self.resolve_pending_anchors(x, y);
    // 最終リージョンに残っている脚注を確定させる（flush-bottom 対象ではないので `flush=false`）
    self.end_region(geom, false);
    // 文書末尾の行で分割された脚注の繰越を出し切る。本文がもう無いので、繰越だけのリージョンを
    // 尽きるまで重ねる（[`PageComposer::seed_carry`] は 1 リージョンぶんしか詰めないため、
    // 最終ページより長い繰越には複数回まわす必要がある）。
    while !self.carry.is_empty() {
      self.next_region(geom);
      self.seed_carry(geom);
      self.end_region(geom, false);
    }
    // 本文も確定脚注も無い末尾ページは push しない（`start_new_page` と同じ述語）。ただし 1 ページも
    // 確定していなければ push する（空文書でも最低 1 ページを返す `break_pages` の事後条件のため）。
    if !self.pages.is_empty() && self.current.is_empty() && self.page_footnotes.is_empty() {
      return (self.pages, self.overflows);
    }
    self.pages.push(Page {
      blocks: self.current,
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: self.page_footnotes,
      anchors: self.current_anchors,
      links: self.current_links,
      index_entries: self.current_index_entries,
      background_color: geom.background_color,
      content_origin_x: geom.content_origin_x,
    });
    return (self.pages, self.overflows);
  }

  /// 現在リージョン（段）を確定し、下端揃え（#169）が有効なら不足高さを段内の伸縮アキへ配分する。
  fn end_region(&mut self, geom: &PageGeometry, flush: bool) {
    let glues = std::mem::take(&mut self.region_glues);
    let region_start = self.region_start;
    let link_start = self.region_link_start;
    let anchor_start = self.region_anchor_start;
    let has_blocks = region_start < self.current.len();
    if flush && geom.flush_bottom && has_blocks && !glues.is_empty() {
      let last_index = self.current.len() - 1;
      // リージョン下端 = 段内ブロックの底辺の最大値（末尾ブロックが最下）。
      let region_bottom = self.current[region_start..]
        .iter()
        .map(placed_block_bottom)
        .fold(Length::from_sp(i64::MIN), Length::max);
      // 脚注がこのリージョンにあれば、下端揃えの目標も版面下限ではなく脚注エリア手前
      // （`region_limit`）にする（そうしないと本文が脚注エリアへ伸びて重なる）。
      let deficit = self.region_limit(geom) - region_bottom;
      // 分母 = 末尾ブロックより前に記録されたアキの stretch 合計（末尾アキは除く）。
      let effective: Length = glues.iter().filter(|g| return g.block_at <= last_index).map(|g| return g.stretch).sum();
      // 不足高さ・分母が正のときだけ配分する（負・ゼロは揃えても意味がないので自然高のまま）。
      if deficit > FLUSH_EPSILON && effective.is_positive() {
        let ratio = deficit.ratio(effective);
        for (offset, block) in self.current[region_start..].iter_mut().enumerate() {
          let idx = region_start + offset;
          let stretch: Length = glues.iter().filter(|g| return g.block_at <= idx).map(|g| return g.stretch).sum();
          shift_placed_block(block, stretch.scale(ratio));
        }
        for (offset, link) in self.current_links[link_start..].iter_mut().enumerate() {
          let idx = link_start + offset;
          let stretch: Length = glues.iter().filter(|g| return g.link_at <= idx).map(|g| return g.stretch).sum();
          link.y += stretch.scale(ratio);
        }
        for (offset, anchor) in self.current_anchors[anchor_start..].iter_mut().enumerate() {
          let idx = anchor_start + offset;
          let stretch: Length = glues.iter().filter(|g| return g.anchor_at <= idx).map(|g| return g.stretch).sum();
          anchor.y += stretch.scale(ratio);
        }
      }
    }
    // 脚注はリージョンが閉じるたびに常に確定する（flush-bottom の対象ではないため `flush` を問わない。
    // 強制改ページ・最終ページでも脚注を落とさないようにする）。開始 Y は `region_limit`
    // （脚注ぶんを差し引く前段、= 脚注エリアの上端）。区切り罫線は「このリージョンで最初の脚注」の
    // 直前にのみ 1 本出す（`top_margin` + 罫線 + `rule_gap`）。2 個目以降は `rule_gap` のみを挟む
    // （[`FootnoteCharges::entry_overhead`] の課金と対称になるよう揃える）。
    //
    // `region_footnotes` は分割済み（[`place_paragraph`] / [`PageComposer::seed_carry`] が
    // [`pack_footnotes`] の決めた行数へ切り詰め済み）なので、ここは「あるものをそのまま積む」だけでよい。
    // 高さの漸化式は見積り側（[`FootnoteDemand::new`]）と一致していなければならない
    // （1 行でもずれると本文と脚注が重なる）。
    if !self.region_footnotes.is_empty() {
      let mut top = self.region_limit(geom);
      let column_x = self.column_offset();
      for (index, pending) in std::mem::take(&mut self.region_footnotes).into_iter().enumerate() {
        let mut blocks = Vec::new();
        if index == 0 {
          top += geom.footnote_top_margin;
          if geom.footnote_rule_thickness.is_positive() {
            blocks.push(PlacedBlock::Rule {
              x: column_x,
              y: top,
              width: geom.footnote_rule_length,
              height: geom.footnote_rule_thickness,
              color: geom.footnote_rule_color,
            });
            top += geom.footnote_rule_thickness;
          }
        }
        top += geom.footnote_rule_gap;
        // 繰越でない（＝本体先頭の、マーカーを持つ行を含む）断片だけに到達先アンカーを打つ。
        // 長い脚注のページ間分割（#227）が入っても、本文中マーカーからは常に本体の先頭位置へ
        // 飛べるようにするため。座標 `top` はこの箱の上端で、`collect_line_links` が定義する
        // 「行 box の上端」（`baseline_y - line.height`）と同じ意味。
        if !pending.continued {
          self.current_anchors.push(PlacedAnchor {
            mark: AnchorMark::Footnote(FootnoteId::new(pending.index)),
            x: column_x,
            y: top,
          });
        }
        blocks.reserve(pending.lines.len());
        let mut baseline = top + pending.lines.first().map_or(Length::ZERO, |line| return line.height);
        let mut prev_depth = Length::ZERO;
        for (i, line) in pending.lines.into_iter().enumerate() {
          if i > 0 {
            baseline += pending.leading.max(prev_depth + line.height);
          }
          prev_depth = line.depth;
          blocks.push(PlacedBlock::Line {
            line,
            baseline_y: baseline,
          });
        }
        top = baseline + prev_depth;
        self.page_footnotes.push(PlacedFootnote {
          number: pending.number,
          index: pending.index,
          continued: pending.continued,
          blocks,
        });
      }
      self.region_footnote_height = Length::ZERO;
    }
    self.region_start = self.current.len();
    self.region_link_start = self.current_links.len();
    self.region_anchor_start = self.current_anchors.len();
  }
}

/// 下端揃え（#169）で配分を行う不足高さの下限（pt）。これ未満は浮動小数の誤差とみなし揃えない。
const FLUSH_EPSILON: Length = Length::from_sp(66);

/// [`PlacedBlock`] の底辺（ページ上端からの距離、pt）を返す。下端揃えのリージョン下端算出に使う。
fn placed_block_bottom(block: &PlacedBlock) -> Length {
  return match block {
    PlacedBlock::Line { line, baseline_y } => *baseline_y + line.depth,
    PlacedBlock::MathBlock {
      body, baseline_y, ..
    } => *baseline_y + body.depth,
    PlacedBlock::Image { y, height, .. } | PlacedBlock::Rule { y, height, .. } => *y + *height,
    PlacedBlock::Table { rows, .. } => rows.last().map_or(Length::from_sp(i64::MIN), |r| return r.top_y + r.height),
  };
}

/// [`PlacedBlock`] とその内部の確定座標を下方へ `dy` だけずらす（下端揃えの配分で使う）。
fn shift_placed_block(block: &mut PlacedBlock, dy: Length) {
  if dy == Length::ZERO {
    return;
  }
  match block {
    PlacedBlock::Line { baseline_y, .. } => *baseline_y += dy,
    PlacedBlock::MathBlock {
      baseline_y,
      numbers,
      ..
    } => {
      *baseline_y += dy;
      for number in numbers {
        number.baseline_y += dy;
      }
    },
    PlacedBlock::Image { y, .. } | PlacedBlock::Rule { y, .. } => *y += dy,
    PlacedBlock::Table { rows } => {
      for row in rows {
        row.top_y += dy;
        row.baseline_y += dy;
        if let Some(rule) = &mut row.rule {
          rule.y += dy;
        }
      }
    },
  }
}

/// ブロック列をページへ配置し、確定ページ列と脚注のはみ出し記録（#382）を返す。
///
/// はみ出し記録は検出順＝ページ順で、`page_index` は返すページ列の中での 0 起点 index。
#[must_use]
pub fn break_pages(
  blocks: Vec<Block>,
  text_width: Length,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
) -> (Vec<Page>, Vec<FootnoteOverflow>) {
  let col_width = column_width(text_width, geom.num_columns, geom.column_gap);
  let mut composer = PageComposer::new(geom, col_width);
  let block_count = blocks.len();
  let mut blocks = blocks;

  // keep-with-next（見出し直後の分割禁止・#168）を尊重しつつ前から順に配置する。FORBID penalty で
  // 連結された見出し群（keep グループ）の先頭で一度だけ、末尾ブロックの先頭チャンクが現在のリージョンに
  // 収まるかを判定し、収まらなければグループごと次リージョンへ送る（見出しがページ末尾に孤立するのを防ぐ）。
  let mut i = 0;
  let mut gated_end: Option<usize> = None;
  while i < blocks.len() {
    if gated_end.is_none_or(|e| return i > e)
      && is_content_block(&blocks[i])
      && let Some(end) = keep_group_end(&blocks, i)
    {
      if keep_group_orphaned(&composer, geom, breaker, alignment, col_width, &blocks[i..=end])
        && !composer.at_region_top(geom)
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
        shrink: Length::ZERO,
      },
    );
    match block {
      Block::Paragraph {
        items,
        leading,
        indent,
        right_indent,
        align,
      } => {
        place_paragraph(
          &mut composer,
          geom,
          breaker,
          alignment,
          &items,
          leading,
          col_width,
          indent,
          right_indent,
          align,
        );
      },
      Block::ComposedLine { line, leading } => {
        place_single_line(&mut composer, geom, line, leading);
      },
      // 伸縮アキ。カーソルへは自然値のみ加算する（下端揃え無効時は VSpace と同一挙動）。stretch を持つ
      // アキは下端揃え（#169）用に配置順（各配置ベクタの現在長）と stretch を記録しておき、リージョン確定時に
      // 不足高さを配分する。cursor_at_edge は触らない（アキはフラグを変えない）。
      Block::Glue {
        natural, stretch, ..
      } => {
        if stretch != Length::ZERO {
          composer.region_glues.push(GlueMark {
            stretch,
            block_at: composer.current.len(),
            link_at: composer.current_links.len(),
            anchor_at: composer.current_anchors.len(),
          });
        }
        composer.y += natural;
      },
      // 分割コスト。強制改ページ（−∞）は eager に改ページ。有限は次の内容ブロックへ持ち越す。
      // 分割禁止（+∞）は keep-with-next のグループ連結マーカーで、ゲート（keep_group_*）が処理済み
      // なのでここでは配置上の副作用を持たない（pending にも積まない）。
      Block::Penalty { value } => {
        if value == PENALTY_FORCE_BREAK {
          composer.force_new_page(geom);
        } else if value != PENALTY_FORBID_BREAK {
          composer.pending_penalty = value;
        }
      },
      Block::Rule {
        width,
        height,
        align,
      } => {
        let penalty = composer.take_pending_penalty();
        composer.consider_break(height, penalty, geom);
        let col_off = composer.column_offset();
        composer.resolve_pending_anchors(col_off, composer.y);
        composer.current.push(PlacedBlock::Rule {
          x: col_off + align.offset(col_width, width),
          y: composer.y,
          width,
          height,
          color: None,
        });
        composer.y += height;
        composer.cursor_at_edge = true;
      },
      Block::Image {
        path,
        width,
        height,
        target_dpi,
        align,
      } => {
        // 縦組版は確定済みサイズを前提とする（resolve_images prepass 後）。未解決は 0 扱い
        let width = width.unwrap_or(Length::ZERO);
        let height = height.unwrap_or(Length::ZERO);
        let penalty = composer.take_pending_penalty();
        composer.consider_break(height, penalty, geom);
        let col_off = composer.column_offset();
        composer.resolve_pending_anchors(col_off, composer.y);
        composer.current.push(PlacedBlock::Image {
          path,
          x: col_off + align.offset(col_width, width),
          y: composer.y,
          width,
          height,
          target_dpi,
        });
        composer.y += height;
        composer.cursor_at_edge = true;
      },
      Block::Table { table, align } => {
        place_table(&mut composer, geom, &table, col_width, align);
        composer.cursor_at_edge = true;
      },
      Block::Math {
        body,
        numbers,
        numbers_on_right,
        align,
      } => {
        place_math_block(&mut composer, geom, body, numbers, numbers_on_right, align, col_width);
        composer.cursor_at_edge = true;
      },
      // アンカーはゼロサイズ。次の実ブロックの確定座標で解決するため pending に積む
      Block::Anchor(mark) => {
        composer.pending_anchors.push(mark);
      },
    }
    i += 1;
  }

  let (pages, overflows) = composer.finish(geom);
  debug!(block_count, page_count = pages.len(), "ページ分割が完了しました");
  return (pages, overflows);
}

/// widow/orphan 制御でまとめて送る最小行数。
const MIN_LINES_AT_BREAK: usize = 2;

/// 段落 1 行の配置計画（純粋な幾何判定の結果）
#[derive(Debug, Clone, PartialEq)]
struct LinePlacement {
  /// 行のベースライン（ページ上端からの距離、pt）
  baseline: Length,
  /// この行から新しいリージョン（次段 / 次ページ）が始まるか
  starts_region: bool,
  /// この行を確定した時点でのリージョンの脚注予約高さ（pt）。新リージョンが始まった行は
  /// そのリージョンで最初の予約（＝この行自身の脚注ぶんのみ）になる。
  /// [`place_paragraph`] が確定ループで `composer.region_footnote_height` へそのまま反映する。
  reserved_after: Length,
  /// この行の脚注ごとに、この行が乗るリージョンへ置く行数（行の脚注と同順・同長。脚注が無ければ空）
  own_splits: Vec<usize>,
  /// この行の脚注群が空のリージョンにも収まらず、はみ出したまま置かれるか（#382）。
  /// 計画は widow / orphan 補正で何度も立て直されるので、ここでは事実を載せるだけにして、
  /// 警告は確定した計画を配置する [`place_paragraph`] だけが組み立てる
  overflowed: bool,
}

/// 強制改リージョン点（`forced`）を尊重しつつ、貪欲にベースラインを送って各行を配置する（純粋関数）
#[allow(clippy::too_many_arguments)]
fn place_lines(
  lines: &[Line],
  y0: Length,
  cursor_at_edge: bool,
  leading: Length,
  margin_top: Length,
  page_limit: Length,
  forced: &[bool],
  demands: &[Vec<FootnoteDemand>],
  initial_reserved: Length,
  charges: FootnoteCharges,
  carry_pending: bool,
) -> (Vec<LinePlacement>, bool) {
  let mut plan = Vec::with_capacity(lines.len());
  let mut baseline = y0;
  let mut prev_depth: Option<Length> = None;
  let mut reserved = initial_reserved;
  for (i, line) in lines.iter().enumerate() {
    match prev_depth {
      // 段落先頭行: 直前が底辺基準ブロックならアセント分下げる
      None => {
        if cursor_at_edge {
          baseline += line.height;
        }
      },
      // 2 行目以降: leading か「前の行の深さ + この行の高さ」の大きい方だけ送る
      Some(depth) => {
        baseline += leading.max(depth + line.height);
      },
    }
    let mut fit = if forced[i] {
      LineFootnoteFit::Rejected
    } else {
      fit_line_footnotes(&demands[i], reserved, baseline + line.depth, page_limit, charges)
    };
    let starts_region = matches!(fit, LineFootnoteFit::Rejected);
    if starts_region {
      if carry_pending {
        // 次リージョンの脚注エリアは繰越で埋まる。どれだけ埋まるかを詰めるまでこの行の
        // ベースラインは決められないので、ここで計画を打ち切る（呼び出し元が seed して計画し直す）
        return (plan, true);
      }
      baseline = margin_top;
      fit = fit_line_footnotes(&demands[i], Length::ZERO, baseline + line.depth, page_limit, charges);
    }
    let split_here = matches!(fit, LineFootnoteFit::Split(..));
    let mut overflowed = false;
    let (reserved_after, own_splits) = match fit {
      LineFootnoteFit::Full(area) => (area, demands[i].iter().map(FootnoteDemand::line_count).collect()),
      LineFootnoteFit::Split(area, splits) => (area, splits),
      // 空のリージョンでも収まらない病的ケース（脚注の先頭 1 行がページ全高を超える等）。
      // 次リージョンへ送っても改善しないので、オーバーフローを許容してそのまま置く
      LineFootnoteFit::Rejected => {
        overflowed = !demands[i].is_empty();
        (
          footnote_area_full(&demands[i], Length::ZERO, charges),
          demands[i].iter().map(FootnoteDemand::line_count).collect(),
        )
      },
    };
    reserved = reserved_after;
    plan.push(LinePlacement {
      baseline,
      starts_region,
      reserved_after,
      own_splits,
      overflowed,
    });
    prev_depth = Some(line.depth);
    if split_here {
      return (plan, true);
    }
  }
  return (plan, false);
}

/// 配置計画から widow/orphan 違反を 1 つ検出し、追加すべき強制改リージョン点を返す（純粋関数）
fn pick_correction(
  plan: &[LinePlacement],
  min_lines: usize,
  is_paragraph_start: bool,
  is_paragraph_end: bool,
) -> Option<usize> {
  let n = plan.len();
  if n < 2 {
    return None;
  }
  // orphan: 先頭リージョン（index 0 から最初の改リージョンまで）の行数が最小行数未満
  // （先頭行が既にリージョン先頭なら回避不能なので補正しない）
  if is_paragraph_start
    && let Some(first_break) = (1..n).find(|&i| return plan[i].starts_region)
    && first_break < min_lines
    && !plan[0].starts_region
  {
    return Some(0);
  }
  if !is_paragraph_end {
    return None;
  }
  // widow: 末尾リージョン（最後の改リージョンから末尾まで）の行数が最小行数未満
  if let Some(last_break) = (1..n).rev().find(|&i| return plan[i].starts_region)
    && n - last_break < min_lines
  {
    // 前側に最小行数を残せるなら末尾 min_lines 行だけを送る。残せない短い段落は全体を送る
    let target = if n >= 2 * min_lines { n - min_lines } else { 0 };
    // 全体を送っても先頭が既にリージョン先頭なら回避不能
    if target == 0 && plan[0].starts_region {
      return None;
    }
    return Some(target);
  }
  return None;
}

/// 段落の行列を現在のカーソルから前から順に配置する計画を立てる（純粋関数・widow/orphan 制御込み）
#[allow(clippy::too_many_arguments)]
fn plan_paragraph_lines(
  lines: &[Line],
  y0: Length,
  cursor_at_edge: bool,
  leading: Length,
  margin_top: Length,
  page_limit: Length,
  demands: &[Vec<FootnoteDemand>],
  initial_reserved: Length,
  charges: FootnoteCharges,
  is_paragraph_start: bool,
  carry_pending: bool,
) -> (Vec<LinePlacement>, bool) {
  let mut forced = vec![false; lines.len()];
  loop {
    let (plan, truncated) = place_lines(
      lines,
      y0,
      cursor_at_edge,
      leading,
      margin_top,
      page_limit,
      &forced,
      demands,
      initial_reserved,
      charges,
      carry_pending,
    );
    // 打ち切られた計画の末尾は段落の末尾ではない（続きは繰越を詰めてから計画し直す）
    let is_paragraph_end = !truncated;
    match pick_correction(&plan, MIN_LINES_AT_BREAK, is_paragraph_start, is_paragraph_end) {
      // 新しい補正点なら強制して再フロー
      Some(idx) if !forced[idx] => forced[idx] = true,
      // 補正不要、または前進しない（回避不能）なら確定
      _ => return (plan, truncated),
    }
  }
}

/// 内容ブロック（実際の高さを占め、リージョン配置の対象になるブロック）か。
fn is_content_block(block: &Block) -> bool {
  return matches!(
    block,
    Block::Paragraph { .. }
      | Block::Table { .. }
      | Block::Image { .. }
      | Block::Rule { .. }
      | Block::ComposedLine { .. }
      | Block::Math { .. }
  );
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

/// keep グループの末尾が段落でない（図表・数式・罫線・合成行）ときの配置シミュレーション。
fn atomic_place_sim(block: &Block, y: Length, cae: bool, geom: &PageGeometry) -> (bool, Length) {
  match block {
    Block::Image { height, .. } => {
      let h = height.unwrap_or(Length::ZERO);
      return (y + h > geom.page_limit, y + h);
    },
    Block::Rule { height, .. } => return (y + *height > geom.page_limit, y + *height),
    Block::Math { body, .. } => {
      let h = body.height + body.depth;
      return (y + h > geom.page_limit && geom.margin_top + h <= geom.page_limit, y + h);
    },
    Block::ComposedLine { line, leading } => {
      let baseline = if cae { y + line.height } else { y };
      return (baseline + line.depth > geom.page_limit, baseline + *leading);
    },
    Block::Table { table, .. } => {
      let row_h = |row| return table_row_height(row, geom.default_font_size, geom.line_height_factor);
      let total: Length = table.head.iter().chain(table.rows.iter()).map(row_h).sum();
      if table.breakable {
        let first = table.head.first().or_else(|| return table.rows.first());
        return (first.is_some_and(|r| return y + row_h(r) > geom.page_limit), y + total);
      }
      return (y + total > geom.page_limit && geom.margin_top + total <= geom.page_limit, y + total);
    },
    _ => return (false, y),
  }
}

/// keep グループを現在のカーソルから配置したとき、末尾の内容ブロックの先頭チャンクが見出しと
/// 別リージョンに落ちる（= 見出しが孤立する）かを返す純粋関数。リージョン改は行わず、収まらなければ `true`。
fn keep_group_orphaned(
  composer: &PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
  column_width: Length,
  group: &[Block],
) -> bool {
  let Some(last_content) = group.iter().rposition(is_content_block) else {
    return false;
  };
  let mut y = composer.y;
  let mut cae = composer.cursor_at_edge;
  // 同一段落内の直前行（baseline, depth, leading）。段落境界（glue 通過）でリセットする。
  let mut prev: Option<(Length, Length, Length)> = None;
  for (gi, block) in group.iter().enumerate() {
    match block {
      Block::Glue { natural, .. } => {
        y += *natural;
        prev = None;
      },
      Block::Paragraph {
        items,
        leading,
        indent,
        right_indent,
        align,
      } => {
        let available = (column_width - *indent - *right_indent).max(Length::ZERO);
        let effective = if *align == Align::Left {
          alignment
        } else {
          TextAlignment::RaggedRight
        };
        let lines = breaker.break_lines(items, available, effective);
        // 末尾（本文）は widow/orphan で丸ごと送られない最小行数だけを keep 対象にする。見出しは全行。
        let commit = if gi == last_content {
          MIN_LINES_AT_BREAK.min(lines.len())
        } else {
          lines.len()
        };
        let mut last_baseline = y;
        for (li, line) in lines.iter().enumerate() {
          let baseline = match prev {
            Some((pb, pd, pl)) => pb + pl.max(pd + line.height),
            None if cae => y + line.height,
            None => y,
          };
          if li < commit && baseline + line.depth > geom.page_limit {
            return true;
          }
          last_baseline = baseline;
          prev = Some((baseline, line.depth, *leading));
        }
        y = last_baseline + *leading;
        cae = false;
        prev = None;
      },
      _ if is_content_block(block) => {
        let (advance, y_after) = atomic_place_sim(block, y, cae, geom);
        if gi == last_content || advance {
          return advance;
        }
        y = y_after;
        cae = true;
        prev = None;
      },
      _ => {},
    }
  }
  return false;
}

/// 段落を行に割ってベースライン送りで配置する
#[allow(clippy::too_many_arguments)]
fn place_paragraph(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
  items: &[HItem],
  leading: Length,
  column_width: Length,
  indent: Length,
  right_indent: Length,
  align: Align,
) {
  let available = (column_width - indent - right_indent).max(Length::ZERO);
  // 両端揃えは左揃え段落にのみ適用する。中央・右寄せ段落は行を自然幅のまま組み、
  // 確定後に揃えオフセットでシフトする（伸縮すると余り幅が消えて揃え自体が無意味になる）
  let effective_alignment = if align == Align::Left {
    alignment
  } else {
    TextAlignment::RaggedRight
  };
  let mut lines = breaker.break_lines(items, available, effective_alignment);
  // 行は段左端 (x=0) 基準で組まれるため、インデント + 揃えオフセット（段内 [0, column_width]）を
  // 全行に加算する。揃えオフセットは行ごとに（行幅に応じて）異なる。段オフセットは段をまたぐと
  // 行ごとに変わるため、この事前ループには含めず、配置ループ内で着地段ごとに足す。
  for line in &mut lines {
    let line_width = line.boxes.iter().map(|b| return b.x + b.width).fold(Length::ZERO, Length::max);
    let shift = indent + align.offset(available, line_width);
    if shift != Length::ZERO {
      for positioned in &mut line.boxes {
        positioned.x += shift;
      }
      for link in &mut line.links {
        link.x0 += shift;
        link.x1 += shift;
      }
    }
  }
  // 各行に付いた脚注（`line.footnotes`）を行分割し、分割可能な需要（行ごとの積み上げ高さ）を作る。
  // ここで 1 回だけ計算し、widow/orphan の再フロー（`plan_paragraph_lines` 内のリトライ）や
  // chunk の再計画では再計算しない（脚注の構成は改リージョン点の選び方で変わらないため）。
  let charges = FootnoteCharges::of(geom);
  let mut demands: Vec<Vec<FootnoteDemand>> = Vec::with_capacity(lines.len());
  let mut bodies: Vec<Vec<PendingFootnote>> = Vec::with_capacity(lines.len());
  for line in &lines {
    let mut line_demands = Vec::with_capacity(line.footnotes.len());
    let mut line_bodies = Vec::with_capacity(line.footnotes.len());
    for footnote in &line.footnotes {
      let broken = breaker.break_lines(&footnote.items, column_width, TextAlignment::RaggedRight);
      line_demands.push(FootnoteDemand::new(&broken, footnote.leading));
      line_bodies.push(PendingFootnote {
        number: footnote.number,
        index: footnote.index,
        continued: false,
        lines: broken,
        leading: footnote.leading,
      });
    }
    demands.push(line_demands);
    bodies.push(line_bodies);
  }
  // 段落を前から chunk 単位で確定する。計画は「脚注が分割された行」「繰越が残っている状態での
  // 改リージョン」で打ち切られるので、そこまでを配置 → 改リージョンして繰越を詰める
  // （`advance_region` → `seed_carry`）→ 残りを計画し直す、と回す。**seed してから再計画する**のが
  // 要点で、逆にすると計画が繰越ぶんの予約を知らないままベースラインを決めてしまい、本文が
  // 繰越脚注に重なる。繰越が生じない段落ではループは 1 周で、移行前と同一の経路になる。
  let mut last_baseline = composer.y;
  let mut is_paragraph_start = true;
  while !lines.is_empty() {
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      composer.y,
      composer.cursor_at_edge,
      leading,
      geom.margin_top,
      geom.page_limit,
      &demands,
      composer.region_footnote_height,
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
    for (i, ((mut line, footnotes), placement)) in chunk_lines.into_iter().zip(chunk_bodies).zip(plan).enumerate() {
      if placement.starts_region {
        composer.advance_region(geom);
      }
      // 改リージョン後＝この行が実際に乗るページが確定してから記録する（#382）。計画そのものは
      // widow / orphan 補正で捨てられることがあるので、確定したこのループでだけ警告の種を作る
      if placement.overflowed {
        composer.overflows.push(FootnoteOverflow {
          page_index: composer.pages.len(),
          kind: FootnoteOverflowKind::Line {
            numbers: footnotes.iter().map(|footnote| return footnote.number).collect(),
          },
        });
      }
      let baseline = placement.baseline;
      last_baseline = baseline;
      let col_off = composer.column_offset();
      if col_off != Length::ZERO {
        for positioned in &mut line.boxes {
          positioned.x += col_off;
        }
        for link in &mut line.links {
          link.x0 += col_off;
          link.x1 += col_off;
        }
      }
      if is_paragraph_start && i == 0 {
        composer.resolve_pending_anchors(col_off, baseline - line.height);
      }
      composer.collect_line_links(&line, baseline);
      composer.collect_line_index_entries(&line);
      for (footnote, &placed) in footnotes.into_iter().zip(&placement.own_splits) {
        let (head, tail) = split_pending(footnote, placed);
        if let Some(head) = head {
          composer.region_footnotes.push(head);
        }
        if let Some(tail) = tail {
          composer.carry.push(tail);
        }
      }
      composer.region_footnote_height = placement.reserved_after;
      composer.current.push(PlacedBlock::Line {
        line,
        baseline_y: baseline,
      });
    }
    is_paragraph_start = false;
    // 打ち切った chunk の続きがあるなら、改リージョンして繰越を詰めてから次の chunk を計画する。
    // 続きが無い（段落末尾の行で分割した）場合の繰越は、次のブロックの改リージョンか
    // [`PageComposer::finish`] が拾う。
    if truncated && !lines.is_empty() {
      composer.advance_region(geom);
    }
  }
  composer.y = last_baseline + leading;
  composer.cursor_at_edge = false;
}

/// 合成済みの単一行（[`Block::ComposedLine`]）を 1 行として配置する
fn place_single_line(composer: &mut PageComposer, geom: &PageGeometry, mut line: Line, leading: Length) {
  let mut baseline = composer.y;
  if composer.cursor_at_edge {
    baseline += line.height;
  }
  if baseline + line.depth > composer.region_limit(geom) {
    composer.advance_region(geom);
    baseline = geom.margin_top;
  }
  let col_off = composer.column_offset();
  if col_off != Length::ZERO {
    for positioned in &mut line.boxes {
      positioned.x += col_off;
    }
    for link in &mut line.links {
      link.x0 += col_off;
      link.x1 += col_off;
    }
  }
  composer.resolve_pending_anchors(col_off, baseline - line.height);
  composer.collect_line_links(&line, baseline);
  composer.collect_line_index_entries(&line);
  composer.current.push(PlacedBlock::Line {
    line,
    baseline_y: baseline,
  });
  composer.y = baseline + leading;
  composer.cursor_at_edge = false;
}

/// ディスプレイ数式ブロックを配置する
fn place_math_block(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  body: HBox,
  numbers: Vec<MathRowNumber>,
  numbers_on_right: bool,
  align: Align,
  column_width: Length,
) {
  let total_height = body.height + body.depth;
  let limit = composer.region_limit(geom);
  if composer.y + total_height > limit && geom.margin_top + total_height <= limit {
    composer.advance_region(geom);
  }
  let col_off = composer.column_offset();
  composer.resolve_pending_anchors(col_off, composer.y);

  let x = col_off + align.offset(column_width, body.width);
  let baseline_y = composer.y + body.height;
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

  composer.current.push(PlacedBlock::MathBlock {
    body,
    x,
    baseline_y,
    numbers: placed_numbers,
  });
  composer.y += total_height;
}

/// ページ内の着地段が確定するまで保持する表の 1 行
struct PendingTableRow {
  /// シェーピング済みの行内容
  row: TableRowBox,
  /// 行帯のページ上端からの距離
  top_y: Length,
  /// 行帯の高さ
  height: Length,
}

/// 表を行単位で配置する（改段・改ページ時は先頭にヘッダ行を再描画する）
fn place_table(composer: &mut PageComposer, geom: &PageGeometry, table: &TableBox, column_width: Length, align: Align) {
  let col_widths = resolve_column_widths(table, column_width, geom.table_cell_padding);
  // 表全体の自然幅は確定済み列幅の総和。段幅の中で揃えオフセット（段内）を 1 回だけ算出する
  // （全幅の表ではオフセットが 0 になり段左端のまま）。段オフセットは flush 時に断片ごとに足す。
  let table_align_offset = align.offset(column_width, col_widths.iter().sum());
  let head_heights: Vec<Length> = table
    .head
    .iter()
    .map(|row| return table_row_height(row, geom.default_font_size, geom.line_height_factor))
    .collect();
  let row_heights: Vec<Length> = table
    .rows
    .iter()
    .map(|row| return table_row_height(row, geom.default_font_size, geom.line_height_factor))
    .collect();

  // 分割禁止の表は、現ページに収まらず新しいページなら収まる場合のみ先に改ページする
  let total_height: Length = head_heights.iter().chain(row_heights.iter()).sum();
  let limit = composer.region_limit(geom);
  if !table.breakable && composer.y + total_height > limit && geom.margin_top + total_height <= limit {
    composer.advance_region(geom);
  }

  // 表先頭の確定位置（改段・改ページ後）で未解決アンカー（`\ref{tab:...}` 到達先）を解決する
  composer.resolve_pending_anchors(composer.column_offset(), composer.y);

  let mut pending_rows: Vec<PendingTableRow> = Vec::new();
  // head 行・本体行・改ページ後のヘッダ再描画を同じ経路へ積む。セルの絶対 x は
  // 着地段が決まる flush 時に確定する。
  let push_row = |pending_rows: &mut Vec<PendingTableRow>, row: &TableRowBox, top_y: Length, height: Length| {
    pending_rows.push(PendingTableRow {
      row: row.clone(),
      top_y,
      height,
    });
  };
  // 現在の pending_rows を、セル内容・リンク・罫線の絶対座標まで確定した表断片へ変換する。
  // 断片の x は着地段のオフセット + 段内揃えオフセット（flush は advance_region の前に
  // 呼ばれるので column は正しい）。
  let flush = |composer: &mut PageComposer, pending_rows: &mut Vec<PendingTableRow>| {
    if pending_rows.is_empty() {
      return;
    }
    let table_x = composer.column_offset() + table_align_offset;
    let table_width: Length = col_widths.iter().copied().sum();
    let rows = std::mem::take(pending_rows).into_iter().map(|pending| {
      let mut boxes = position_table_row_boxes(&pending.row, &table.columns, &col_widths, geom.table_cell_padding);
      for positioned in &mut boxes {
        positioned.x += table_x;
      }
      for link in collect_row_links(&pending.row, &table.columns, &col_widths, geom.table_cell_padding) {
        if link.x1 <= link.x0 {
          continue; // 退化矩形はスキップ（collect_line_links と同じ規則）
        }
        composer.current_links.push(PlacedLink {
          target: link.target,
          x: table_x + link.x0,
          y: pending.top_y,
          width: link.x1 - link.x0,
          height: pending.height,
        });
      }
      let baseline_offset = pending
        .row
        .cells
        .iter()
        .filter_map(|cell| return max_font_size_in_items(&cell.items))
        .reduce(Length::max)
        .unwrap_or(pending.height);
      let rule = pending.row.rule_above.then_some(PlacedTableRule {
        x: table_x,
        y: pending.top_y,
        width: table_width,
        height: geom.table_rule_thickness,
        color: geom.table_rule_color,
      });
      return PlacedTableRow {
        top_y: pending.top_y,
        height: pending.height,
        baseline_y: pending.top_y + baseline_offset,
        boxes,
        rule,
      };
    });
    composer.current.push(PlacedBlock::Table {
      rows: rows.collect(),
    });
  };

  for (row, height) in table.head.iter().zip(&head_heights) {
    if composer.y + *height > composer.region_limit(geom) {
      flush(composer, &mut pending_rows);
      composer.advance_region(geom);
    }
    push_row(&mut pending_rows, row, composer.y, *height);
    composer.y += *height;
  }
  for (row, height) in table.rows.iter().zip(&row_heights) {
    if composer.y + *height > composer.region_limit(geom) {
      flush(composer, &mut pending_rows);
      composer.advance_region(geom);
      for (head_row, head_height) in table.head.iter().zip(&head_heights) {
        push_row(&mut pending_rows, head_row, composer.y, *head_height);
        composer.y += *head_height;
      }
    }
    push_row(&mut pending_rows, row, composer.y, *height);
    composer.y += *height;
  }
  flush(composer, &mut pending_rows);
}

#[cfg(test)]
mod tests {
  use super::{
    super::break_lines::GreedyBreaker, FootnoteCharges, FootnoteDemand, FootnoteOverflow, FootnoteOverflowKind,
    LinePlacement, PageGeometry, break_pages, is_content_block, keep_group_end, pack_footnotes, placed_block_bottom,
    plan_paragraph_lines,
  };
  use crate::{
    document::{ColumnAlign, ColumnWidth},
    length::Length,
    style::TextAlignment,
    typeset::{
      boxes::{
        Align, Block, HBox, HBoxContent, HItem, Line, LineLink, LinkTarget, PENALTY_FORBID_BREAK, Page, PlacedBlock,
        PositionedBox, TableBox, TableCellBox, TableColumn, TableRowBox,
      },
      font::GlyphRun,
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

  /// ページ末尾から本文先頭までの通し行ベースライン列（ページごと）を採取するヘルパ
  fn line_counts(pages: &[Page]) -> Vec<usize> {
    return pages.iter().map(|p| return page_baselines(p).len()).collect();
  }

  /// テスト用ジオメトリ（`margin_top=10`, `page_limit=50`、単段）
  fn test_geometry() -> PageGeometry {
    return PageGeometry {
      content_origin_x: Length::ZERO,
      margin_top: Length::pt(10.0),
      page_limit: Length::pt(50.0),
      default_font_size: Length::pt(10.0),
      line_height_factor: 1.0,
      table_cell_padding: Length::pt(2.0),
      num_columns: 1,
      column_gap: Length::pt(0.0),
      flush_bottom: false,
      footnote_top_margin: Length::ZERO,
      footnote_rule_length: Length::ZERO,
      footnote_rule_thickness: Length::ZERO,
      footnote_rule_color: None,
      footnote_rule_gap: Length::pt(4.0),
      table_rule_thickness: Length::ZERO,
      table_rule_color: None,
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

  /// テスト用ボックス（幅 10、高さ 8、深さ 2）
  fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: Length::pt(10.0),
        height: Length::pt(1.0),
      },
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
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    };
  }

  /// 幅 0 の脚注マーカー（`HItem::Footnote`）を作るテストヘルパ
  fn footnote_item(number: u32, body: Vec<HItem>, leading: Length) -> HItem {
    return HItem::Footnote {
      number,
      index: number - 1,
      items: body,
      leading,
    };
  }

  /// 1 行だけの段落（widow/orphan 補正の対象外）を作る。`items` の末尾に脚注マーカーを追加できる
  fn single_line_paragraph(mut items: Vec<HItem>) -> Block {
    items.insert(0, test_box());
    return Block::Paragraph {
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    };
  }

  /// ページの脚注のうち、指定番号の本体行ベースライン列を返す
  fn footnote_baselines(page: &Page, number: u32) -> Vec<Length> {
    return page
      .footnotes
      .iter()
      .find(|f| return f.number == number)
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
    return HItem::IndexMark {
      word: word.to_string(),
      reading: reading.map(str::to_string),
    };
  }

  #[test]
  fn index_entries_collected_per_page_in_order() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("A", None)]),
      single_line_paragraph(vec![index_mark_item("B", None)]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].index_entries.len(), 2);
    assert_eq!(pages[0].index_entries[0].word, "A");
    assert_eq!(pages[0].index_entries[1].word, "B");
  }

  #[test]
  fn index_entries_dedup_same_word_and_reading_on_same_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("語", None)]),
      single_line_paragraph(vec![index_mark_item("語", None)]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].index_entries.len(), 1);
    assert_eq!(pages[0].index_entries[0].word, "語");
  }

  #[test]
  fn index_entries_different_reading_are_separate_entries() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("語", None)]),
      single_line_paragraph(vec![index_mark_item("語", Some("よみ"))]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].index_entries.len(), 2);
  }

  #[test]
  fn index_entries_split_across_pages_are_not_merged() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![index_mark_item("語", None)]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![index_mark_item("語", None)]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{:?}", line_counts(&pages));
    assert_eq!(pages[0].index_entries.len(), 1);
    assert_eq!(pages[1].index_entries.len(), 1);
  }

  #[test]
  fn index_mark_does_not_affect_page_layout() {
    // Arrange
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

    // Act
    let (with_pages, _) = break_pages(with_marks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);
    let (without_pages, _) =
      break_pages(without_marks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&with_pages), line_counts(&without_pages));
    for (with_page, without_page) in with_pages.iter().zip(&without_pages) {
      assert_eq!(page_baselines(with_page), page_baselines(without_page));
    }
  }

  #[test]
  fn footnote_places_at_page_bottom_without_overlap() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_item(
      1,
      vec![test_box()],
      pt(12.0),
    )])];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    let body_baseline = page_baselines(&pages[0])[0];
    assert!(close(body_baseline, 10.0), "body baseline={}", body_baseline.to_pt());
    let footnote_baseline = footnote_baselines(&pages[0], 1)[0];
    assert!(footnote_baseline.to_pt() > body_baseline.to_pt() + 2.0, "脚注が本文より下にあるはず");
    assert!(footnote_baseline.to_pt() <= geom.page_limit.to_pt(), "脚注が page_limit を超えないはず");
  }

  #[test]
  fn line_with_overflowing_footnote_moves_to_next_page_with_it() {
    // Arrange
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

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![
      footnote_item(1, vec![test_box()], pt(12.0)),
      footnote_item(2, vec![test_box()], pt(12.0)),
    ])];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].footnotes.len(), 2);
    assert_eq!(pages[0].footnotes[0].number, 1);
    assert_eq!(pages[0].footnotes[1].number, 2);
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

  /// ページ 1 枚の要約: 本文行数と、脚注ごとの (番号, 繰越か, 本体行数)
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
            return (f.number, f.continued, lines);
          })
          .collect();
        return (page_baselines(page).len(), footnotes);
      })
      .collect();
  }

  #[test]
  fn long_footnote_splits_and_carries_remainder_to_next_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(footnote_layout(&pages), vec![(1, vec![(1, false, 3)]), (1, vec![(1, true, 1)])], "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0]));
    assert_eq!(footnote_baselines(&pages[0], 1), pts(&[24.0, 36.0, 48.0]));
    let carried = footnote_baselines(&pages[1], 1)[0];
    assert!(carried.to_pt() > 12.0, "繰越が本文と重ならないはず: {}", carried.to_pt());
    assert!(carried.to_pt() + 2.0 <= geom.page_limit.to_pt() + 1e-3, "繰越が page_limit を超えないはず");
  }

  #[test]
  fn footnote_anchor_is_placed_only_on_non_continued_fragment() {
    // Arrange
    use crate::typeset::boxes::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let anchors_on: fn(&Page) -> usize = |page| {
      return page
        .anchors
        .iter()
        .filter(
          |a| return matches!(&a.mark, AnchorMark::Footnote(id) if *id == crate::typeset::boxes::FootnoteId::new(0)),
        )
        .count();
    };
    assert_eq!(anchors_on(&pages[0]), 1, "{:?}", pages[0].anchors);
    assert_eq!(anchors_on(&pages[1]), 0, "繰越側にアンカーは無いはず: {:?}", pages[1].anchors);
    let anchor = pages[0]
      .anchors
      .iter()
      .find(|a| return matches!(&a.mark, AnchorMark::Footnote(id) if *id == crate::typeset::boxes::FootnoteId::new(0)))
      .expect("先頭断片のアンカーがあるはず");
    assert!(close(anchor.y, 16.0), "アンカーは脚注先頭行の上端のはず: {anchor:?}");
    assert!(close(anchor.x, 0.0));
  }

  #[test]
  fn carried_footnote_stacks_before_own_footnote_of_the_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![footnote_of_lines(2, 1)]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 10)]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_of_lines(1, 4)])];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
      content: HBoxContent::Rule {
        width: pt(10.0),
        height: pt(1.0),
      },
      width: pt(10.0),
      height: pt(height),
      depth: pt(2.0),
    });
  }

  #[test]
  fn footnote_taller_than_the_page_is_reported_as_an_overflow() {
    // Arrange
    let geom = cramped_geometry();
    let blocks = vec![paragraph_with_footnote_at(1, 0, footnote_of_lines(1, 1))];

    // Act
    let (pages, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1, "はみ出しても配置は続くので 1 ページに収まる: {pages:?}");
    assert_eq!(
      overflows,
      vec![FootnoteOverflow {
        page_index: 0,
        kind: FootnoteOverflowKind::Line { numbers: vec![1] },
      }],
      "はみ出した行の脚注番号とページを記録するはず"
    );
  }

  #[test]
  fn footnotes_on_the_same_overflowing_line_are_reported_together() {
    // Arrange
    let geom = cramped_geometry();
    let blocks = vec![single_line_paragraph(vec![
      footnote_of_lines(1, 1),
      footnote_of_lines(2, 1),
    ])];

    // Act
    let (_, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(
      overflows,
      vec![FootnoteOverflow {
        page_index: 0,
        kind: FootnoteOverflowKind::Line {
          numbers: vec![1, 2]
        },
      }],
      "1 行のはみ出しは 1 件で、その行の脚注番号を出現順に並べるはず"
    );
  }

  #[test]
  fn each_overflowing_line_is_reported_once_in_page_order() {
    // Arrange — 各行に脚注が付き、widow/orphan 補正で計画が立て直される長さの段落
    let geom = cramped_geometry();
    let mut items = Vec::new();
    for number in 1..=3_u32 {
      if number > 1 {
        items.push(HItem::ForcedBreak);
      }
      items.push(test_box());
      items.push(footnote_of_lines(number, 1));
    }
    let blocks = vec![Block::Paragraph {
      items,
      leading: pt(12.0),
      indent: Length::ZERO,
      right_indent: Length::ZERO,
      align: Align::Left,
    }];

    // Act
    let (_, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 計画は何度も立て直されるが、記録するのは確定した配置だけなので行ごとに 1 件
    let recorded: Vec<(usize, Vec<u32>)> = overflows
      .iter()
      .map(|overflow| {
        let FootnoteOverflowKind::Line { numbers } = &overflow.kind else {
          panic!("行のはみ出しとして記録されるはず: {overflow:?}");
        };
        return (overflow.page_index, numbers.clone());
      })
      .collect();
    assert_eq!(
      recorded,
      vec![(0, vec![1]), (1, vec![2]), (2, vec![3])],
      "はみ出しは行ごとに 1 件ずつ、ページ昇順で並ぶはず"
    );
  }

  #[test]
  fn carried_footnote_line_taller_than_the_page_is_reported_as_a_line_overflow() {
    // Arrange — 1 行目は入るが繰越になる 2 行目がページ全高を超える脚注
    let geom = test_geometry();
    let footnote = footnote_item(1, vec![test_box(), HItem::ForcedBreak, tall_box(40.0)], pt(12.0));
    let blocks = vec![single_line_paragraph(vec![footnote])];

    // Act
    let (pages, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "繰越は次ページへ送られる: {pages:?}");
    assert_eq!(
      overflows,
      vec![FootnoteOverflow {
        page_index: 1,
        kind: FootnoteOverflowKind::SingleLine { number: 1 },
      }],
      "繰越先のページと脚注番号を記録するはず"
    );
  }

  #[test]
  fn carried_footnote_line_overflow_is_reported_once_per_page_it_lands_on() {
    // Arrange — 繰越の 2 行がどちらもページ全高を超える（1 ページに 1 行ずつ置かれる）
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

    // Act
    let (_, overflows) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 「置いたページごとに 1 件」であり、脚注 1 個につき 1 件へ束ねてはいない
    assert_eq!(
      overflows,
      vec![
        FootnoteOverflow {
          page_index: 1,
          kind: FootnoteOverflowKind::SingleLine { number: 1 },
        },
        FootnoteOverflow {
          page_index: 2,
          kind: FootnoteOverflowKind::SingleLine { number: 1 },
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
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    };
  }

  /// ページごとの「本文の下端」と「脚注エリアの上端」を返す（重なり検証用。無ければ `None`）
  fn body_bottom_and_footnote_top(page: &Page) -> (Option<Length>, Option<Length>) {
    let body_bottom = page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, baseline_y } => return Some(*baseline_y + line.depth),
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
    // Arrange
    let geom = test_geometry();
    let blocks = vec![paragraph_with_footnote_at(6, 1, footnote_of_lines(1, 4))];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = flush_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::Glue {
        natural: Length::ZERO,
        stretch: pt(10.0),
        shrink: Length::ZERO,
      },
      paragraph_with_footnote_at(4, 0, footnote_of_lines(1, 4)),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let demands = vec![demand_of_lines(4)];

    // Act
    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), true).expect("先頭 1 行は入る");

    // Assert
    assert_eq!(packing.splits, vec![3]);
    assert!(close(packing.height, 38.0), "{}", packing.height.to_pt());
  }

  #[test]
  fn pack_footnotes_rejects_when_first_line_does_not_fit() {
    // Arrange
    let demands = vec![demand_of_lines(2)];

    // Act
    let packing = pack_footnotes(&demands, Length::ZERO, pt(13.0), gap_only_charges(), true);

    // Assert
    assert!(packing.is_none());
  }

  #[test]
  fn pack_footnotes_marks_overflow_when_the_carried_first_line_does_not_fit() {
    // Arrange — 繰越（`require_first_line = false`）で、先頭脚注の 1 行ぶんも無い予算
    let demands = vec![demand_of_lines(2)];

    // Act
    let packing =
      pack_footnotes(&demands, Length::ZERO, pt(5.0), gap_only_charges(), false).expect("繰越は必ず 1 行進める");

    // Assert
    assert!(packing.overflowed, "はみ出したまま置いた事実を返すはず");
    assert_eq!(packing.splits, vec![1], "停止条件として 1 行だけ進める");
  }

  #[test]
  fn pack_footnotes_does_not_mark_overflow_when_lines_fit() {
    // Arrange
    let demands = vec![demand_of_lines(2)];

    // Act
    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), false).expect("全行が入る");

    // Assert
    assert!(!packing.overflowed, "収まったときは記録しないはず");
  }

  #[test]
  fn pack_footnotes_reserves_a_first_line_for_later_footnotes_on_the_same_line() {
    // Arrange
    let demands = vec![demand_of_lines(4), demand_of_lines(1)];

    // Act
    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), true)
      .expect("予約すれば両方に先頭行が置ける");

    // Assert
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
    // Arrange
    let geom = geometry_with_footnote_rule();
    let blocks = vec![single_line_paragraph(vec![
      footnote_item(1, vec![test_box()], pt(12.0)),
      footnote_item(2, vec![test_box()], pt(12.0)),
    ])];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = PageGeometry {
      background_color: Some([10, 20, 30]),
      ..test_geometry()
    };
    let blocks = vec![paragraph_of_lines(1)];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages[0].background_color, Some([10, 20, 30]));
  }

  #[test]
  fn footnote_rule_reservation_does_not_overlap_when_footnote_starts_new_region() {
    // Arrange
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

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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

  #[test]
  fn paragraph_lines_advance_by_leading() {
    // Arrange
    let geom = test_geometry();

    // Act
    let (pages, _) =
      break_pages(vec![paragraph_of_lines(3)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    let baselines: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
    assert_eq!(baselines, pts(&[10.0, 22.0, 34.0]));
  }

  #[test]
  fn page_breaks_when_baseline_exceeds_limit() {
    // Arrange
    let geom = test_geometry();

    // Act
    let (pages, _) =
      break_pages(vec![paragraph_of_lines(5)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    let second_page_first = pages[1].blocks.first().expect("2 ページ目に行があるはず");
    let PlacedBlock::Line { baseline_y, .. } = second_page_first else {
      panic!("Line を期待: {second_page_first:?}");
    };
    assert!(close(*baseline_y, 10.0));
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
      is_last: false,
      links: Vec::new(),
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }

  /// 脚注を持たない `count` 行ぶんの需要（[`place_lines`] / [`plan_paragraph_lines`] のテスト用）
  fn no_footnotes(count: usize) -> Vec<Vec<FootnoteDemand>> { return (0..count).map(|_| return Vec::new()).collect(); }

  /// 課金ゼロの脚注パラメータ（脚注を使わない計画テスト用）
  fn no_charges() -> FootnoteCharges {
    return FootnoteCharges {
      top_margin: Length::ZERO,
      rule_thickness: Length::ZERO,
      rule_gap: Length::ZERO,
    };
  }

  #[test]
  fn orphan_first_line_moves_to_next_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(3), paragraph_of_lines(3)];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0, 34.0]), "先頭行を孤立させず先行段落のみ");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0, 34.0]), "後続段落は丸ごと 2 ページ目へ");
  }

  #[test]
  fn widow_last_line_kept_with_previous() {
    // Arrange
    let geom = test_geometry();

    // Act
    let (pages, _) =
      break_pages(vec![paragraph_of_lines(5)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0, 34.0]), "1 ページ目は 3 行（4 行目を繰り下げ）");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0]), "末尾 2 行が 2 ページ目に揃う");
  }

  #[test]
  fn short_paragraph_moves_whole_rather_than_split() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(2), paragraph_of_lines(3)];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0]), "先行段落の 2 行だけ");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0, 34.0]), "3 行段落は分割せず丸ごと次ページ");
  }

  #[test]
  fn oversized_paragraph_builds_without_hang() {
    // Arrange
    let geom = test_geometry();

    // Act
    let (pages, _) =
      break_pages(vec![paragraph_of_lines(20)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let total: usize = pages.iter().map(|p| return page_baselines(p).len()).sum();
    assert_eq!(total, 20, "行が欠落しない: {pages:?}");
    assert!(pages.len() >= 5, "4 行/ページなので 5 ページ以上に分かれる: {}", pages.len());
  }

  #[test]
  fn plan_leaves_fitting_paragraph_untouched() {
    // Arrange
    let lines = vec![test_line(), test_line(), test_line()];

    // Act
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      pt(10.0),
      false,
      pt(12.0),
      pt(10.0),
      pt(50.0),
      &no_footnotes(3),
      Length::ZERO,
      no_charges(),
      true,
      false,
    );

    // Assert
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
      ]
    );
  }

  #[test]
  fn plan_defers_orphan_first_line() {
    // Arrange
    let lines = vec![test_line(), test_line(), test_line()];

    // Act
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      pt(46.0),
      false,
      pt(12.0),
      pt(10.0),
      pt(50.0),
      &no_footnotes(3),
      Length::ZERO,
      no_charges(),
      true,
      false,
    );

    // Assert
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: true,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
      ]
    );
  }

  #[test]
  fn plan_pulls_widow_last_line_back() {
    // Arrange
    let lines = vec![
      test_line(),
      test_line(),
      test_line(),
      test_line(),
      test_line(),
    ];

    // Act
    let (plan, truncated) = plan_paragraph_lines(
      &lines,
      pt(10.0),
      false,
      pt(12.0),
      pt(10.0),
      pt(50.0),
      &no_footnotes(5),
      Length::ZERO,
      no_charges(),
      true,
      false,
    );

    // Assert
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(10.0),
          starts_region: true,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new(),
          overflowed: false
        },
      ]
    );
  }

  #[test]
  fn vspace_shifts_following_baseline() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(5.0)),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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
  fn page_break_block_starts_new_page() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[1].blocks.len(), 1);
  }

  #[test]
  fn paragraph_after_image_clears_ascent() {
    let geom = test_geometry();
    let blocks = vec![
      Block::Image {
        path: crate::project::ProjectPath::new("x.png"),
        width: Some(pt(20.0)),
        height: Some(pt(15.0)),
        target_dpi: None,
        align: Align::Left,
      },
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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
        path: crate::project::ProjectPath::new("x.png"),
        width: Some(pt(20.0)),
        height: Some(pt(30.0)),
        target_dpi: None,
        align: Align::Left,
      },
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Image { y, .. } = pages[1].blocks.first().expect("画像があるはず") else {
      panic!("Image を期待");
    };
    assert!(close(*y, 10.0), "画像はページ先頭 (margin_top) に置かれる");
  }

  /// テキスト入り（フォント 10）の 1 セル行を作るヘルパ
  fn table_row(text: &str) -> TableRowBox {
    return TableRowBox {
      cells: vec![TableCellBox {
        items: vec![HItem::Box(HBox {
          content: HBoxContent::Glyphs(GlyphRun {
            font_size: pt(10.0),
            text: text.to_string(),
            glyphs: Vec::new(),
            font_type: crate::project::FontType::Serif,
            color: None,
          }),
          width: Length::pt(20.0),
          height: Length::pt(10.0),
          depth: Length::pt(0.0),
        })],
        span: 1,
      }],
      rule_above: false,
    };
  }

  /// ページ内の最初の表ブロックの先頭行セルのテキストを取り出すヘルパ
  fn first_table_row_text(page: &Page) -> Option<String> {
    for block in &page.blocks {
      if let PlacedBlock::Table { rows, .. } = block
        && let Some(first) = rows.first()
        && let Some(positioned) = first.boxes.first()
        && let HBoxContent::Glyphs(run) = &positioned.content
      {
        return Some(run.text.clone());
      }
    }
    return None;
  }

  #[test]
  fn empty_blocks_yield_single_empty_page() {
    let geom = test_geometry();

    let (pages, _) = break_pages(vec![], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 1);
    assert!(pages[0].blocks.is_empty());
  }

  #[test]
  fn multiple_page_breaks_create_multiple_pages() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::force_break(),
      paragraph_of_lines(1),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 3);
  }

  #[test]
  fn leading_page_break_does_not_create_blank_page() {
    let geom = test_geometry();
    let blocks = vec![Block::force_break(), paragraph_of_lines(1)];

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].blocks.len(), 1, "本文は先頭ページに置かれる");
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

    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 2, "中間に白紙ページは生じない: {pages:?}");
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[1].blocks.len(), 1);
  }

  #[test]
  fn trailing_page_break_does_not_create_blank_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(1), Block::force_break()];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].blocks.len(), 1);
  }

  #[test]
  fn page_break_only_input_returns_single_empty_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![Block::force_break()];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1, "{pages:?}");
    assert!(pages[0].blocks.is_empty());
  }

  #[test]
  fn breakable_table_splits_across_pages_and_redraws_header() {
    // Arrange
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: vec![table_row("HEAD")],
      rows: (0..5).map(|i| return table_row(&format!("R{i}"))).collect(),
      breakable: true,
    };

    // Act
    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]).as_deref(), Some("HEAD"), "1 ページ目もヘッダ始まり");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("HEAD"), "2 ページ目はヘッダ再描画");
  }

  /// 1 列 1 行、セル内容がまるごとリンク（`LinkStart`, box(幅20), `LinkEnd`）の表を作る
  fn single_cell_link_table(target: LinkTarget) -> TableBox {
    return TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: Vec::new(),
      rows: vec![TableRowBox {
        cells: vec![TableCellBox {
          items: vec![
            HItem::LinkStart(target),
            HItem::Box(HBox {
              content: HBoxContent::Glyphs(GlyphRun {
                font_size: pt(10.0),
                text: "リンク".to_string(),
                glyphs: Vec::new(),
                font_type: crate::project::FontType::Serif,
                color: None,
              }),
              width: Length::pt(20.0),
              height: Length::pt(10.0),
              depth: Length::ZERO,
            }),
            HItem::LinkEnd,
          ],
          span: 1,
        }],
        rule_above: false,
      }],
      breakable: true,
    };
  }

  #[test]
  fn table_cell_link_becomes_placed_link_on_page() {
    // Arrange
    let geom = test_geometry();
    let target = LinkTarget::External("https://example.com".to_string());
    let table = single_cell_link_table(target.clone());

    // Act
    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert
    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert_eq!(link.target, target);
    assert!(close(link.x, 2.0), "{link:?}");
    assert!(close(link.y, 10.0), "{link:?}");
    assert!(close(link.width, 20.0), "{link:?}");
  }

  #[test]
  fn place_table_carries_padding_and_rule_from_geometry() {
    // Arrange
    let geom = PageGeometry {
      table_cell_padding: Length::pt(3.0),
      table_rule_thickness: Length::pt(1.5),
      table_rule_color: Some([9, 9, 9]),
      ..test_geometry()
    };
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: Vec::new(),
      rows: vec![TableRowBox {
        cells: vec![TableCellBox {
          items: vec![test_box()],
          span: 1,
        }],
        rule_above: true,
      }],
      breakable: false,
    };

    // Act
    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert
    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!("直前の matches! で確認済み");
    };
    assert_eq!(rows[0].boxes[0].x, Length::pt(3.0), "padding はセル内容の確定 x に反映されるはず");
    let rule = rows[0].rule.expect("rule_above=true なので確定罫線を持つはず");
    assert_eq!(rule.height, Length::pt(1.5));
    assert_eq!(rule.color, Some([9, 9, 9]));
  }

  #[test]
  fn table_link_shifts_with_its_row_under_flush_bottom() {
    // Arrange
    let geom = flush_geometry();
    let target = LinkTarget::Internal(crate::typeset::boxes::AnchorId::Label(crate::semantics::LabelId::new("fig:1")));
    let table = single_cell_link_table(target.clone());
    let blocks = vec![
      rule(8.0),                                  // idx0, bottom=18（シフト対象外）
      Block::stretchable_space(pt(4.0), pt(4.0)), // stretch アキ
      Block::Table {
        table,
        align: Align::Left,
      }, // idx1, 行高10。glue 後の y=22、bottom=32
      rule(40.0),                                 // 溢れて改ページ（不足 50-32=18 を配分）
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(rule_ys(&pages[0]), vec![Length::pt(10.0)], "先頭の罫線はシフトされない");
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
    // Arrange
    use crate::typeset::boxes::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading {
        key: crate::semantics::HeadingKey::new(0),
        label: None,
      }),
      paragraph_of_lines(1),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!(close(pages[0].anchors[0].y, 2.0));
    assert!(close(pages[0].anchors[0].x, 0.0));
    assert!(matches!(pages[0].anchors[0].mark, AnchorMark::Heading { label: None, .. }));
  }

  #[test]
  fn pending_anchor_resolves_on_page_after_break() {
    // Arrange
    use crate::typeset::boxes::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(4),
      Block::Anchor(AnchorMark::Label(crate::semantics::LabelId::new("tab:x"))),
      paragraph_of_lines(1),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].anchors.is_empty(), "ページ 1 にアンカーは無い: {:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!(close(pages[1].anchors[0].y, 2.0));
  }

  #[test]
  fn paragraph_link_becomes_placed_link() {
    // Arrange
    use crate::typeset::boxes::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(matches!(&link.target, LinkTarget::External(uri) if uri == "https://example.com"));
    assert!(close(link.x, 0.0));
    assert!(close(link.y, 2.0));
    assert!(close(link.width, 20.0));
    assert!(close(link.height, 10.0));
  }

  #[test]
  fn paragraph_indent_shifts_all_lines_and_reduces_width() {
    // Arrange
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
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(20.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(60.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
      assert!(first.x.to_pt() >= 20.0 - f32::EPSILON, "先頭ボックス x={} は indent(20) 以上", first.x.to_pt());
      for positioned in &line.boxes {
        assert!(
          (positioned.x + positioned.width).to_pt() <= 60.0 + f32::EPSILON,
          "x+width={} <= 60",
          (positioned.x + positioned.width).to_pt()
        );
      }
    }
  }

  #[test]
  fn paragraph_right_indent_reduces_available_width() {
    // Arrange
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
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(10.0),
      right_indent: Length::pt(10.0),
      align: Align::Left,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(60.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
      assert!(first.x.to_pt() >= 10.0 - f32::EPSILON, "先頭ボックス x={} は indent(10) 以上", first.x.to_pt());
      for positioned in &line.boxes {
        assert!(
          (positioned.x + positioned.width).to_pt() <= 50.0 + f32::EPSILON,
          "x+width={} <= text_width - right_indent = 50",
          (positioned.x + positioned.width).to_pt()
        );
      }
    }
  }

  #[test]
  fn paragraph_indent_shifts_links() {
    // Arrange
    use crate::typeset::boxes::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(15.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(close(link.x, 15.0), "link.x={}", link.x.to_pt());
    assert!(close(link.width, 20.0), "link.width={}", link.width.to_pt());
  }

  #[test]
  fn centered_paragraph_shifts_line_to_horizontal_center() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].x, 45.0), "box.x={}", line.boxes[0].x.to_pt());
  }

  #[test]
  fn right_aligned_paragraph_shifts_line_to_right_edge() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Right,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].x, 90.0), "box.x={}", line.boxes[0].x.to_pt());
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
  fn stretchable_paragraph(align: Align) -> Block {
    return Block::Paragraph {
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
      align,
    };
  }

  #[test]
  fn justify_stretches_left_aligned_paragraph() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(Align::Left)];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[1].x + line.boxes[1].width, 27.0), "{:?}", line.boxes);
  }

  #[test]
  fn justify_does_not_stretch_centered_paragraph() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(Align::Center)];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].x, 1.0), "{:?}", line.boxes);
    assert!(close(line.boxes[1].x - line.boxes[0].x, 15.0), "glue は自然幅のまま: {:?}", line.boxes);
  }

  #[test]
  fn justify_does_not_stretch_right_aligned_paragraph() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(Align::Right)];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].x, 2.0), "{:?}", line.boxes);
    assert!(close(line.boxes[1].x - line.boxes[0].x, 15.0), "glue は自然幅のまま: {:?}", line.boxes);
  }

  #[test]
  fn centered_overflowing_line_is_not_shifted_negative() {
    // Arrange
    let geom = test_geometry();
    let wide = HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: Length::pt(50.0),
        height: Length::pt(1.0),
      },
      width: Length::pt(50.0),
      height: Length::pt(8.0),
      depth: Length::pt(2.0),
    });
    let blocks = vec![Block::Paragraph {
      items: vec![wide],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(30.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .expect("行があるはず");
    assert!(close(line.boxes[0].x, 0.0), "box.x={}", line.boxes[0].x.to_pt());
  }

  #[test]
  fn centered_wrapped_lines_are_each_independently_centered() {
    // Arrange
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
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(35.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => return Some(line),
        _ => return None,
      })
      .collect();
    assert_eq!(lines.len(), 2, "text_width=35 で 2 行に折り返すはず: {} 行", lines.len());
    assert!(close(lines[0].boxes[0].x, 5.0), "1 行目先頭 x={}", lines[0].boxes[0].x.to_pt());
    assert!(close(lines[1].boxes[0].x, 12.5), "2 行目先頭 x={}", lines[1].boxes[0].x.to_pt());
  }

  #[test]
  fn centered_paragraph_shifts_links() {
    // Arrange
    use crate::typeset::boxes::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(close(link.x, 40.0), "link.x={}", link.x.to_pt());
    assert!(close(link.width, 20.0), "link.width={}", link.width.to_pt());
  }

  /// ページ内の最初の `PlacedBlock::Image` を取り出すヘルパ
  fn first_image(page: &Page) -> &PlacedBlock {
    return page.blocks.iter().find(|b| matches!(b, PlacedBlock::Image { .. })).expect("画像があるはず");
  }

  #[test]
  fn centered_image_shifts_x_to_horizontal_center() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: crate::project::ProjectPath::new("x.png"),
      width: Some(pt(20.0)),
      height: Some(pt(15.0)),
      target_dpi: None,
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!(close(*x, 40.0), "image.x={}", x.to_pt());
  }

  #[test]
  fn right_aligned_image_shifts_x_to_right_edge() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: crate::project::ProjectPath::new("x.png"),
      width: Some(pt(20.0)),
      height: Some(pt(15.0)),
      target_dpi: None,
      align: Align::Right,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!(close(*x, 80.0), "image.x={}", x.to_pt());
  }

  #[test]
  fn centered_rule_shifts_x_to_horizontal_center() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![Block::Rule {
      width: Length::pt(30.0),
      height: Length::pt(2.0),
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let PlacedBlock::Rule { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Rule { .. })).expect("罫線があるはず")
    else {
      unreachable!()
    };
    assert!(close(*x, 35.0), "rule.x={}", x.to_pt());
  }

  #[test]
  fn centered_table_shifts_all_rows_x() {
    // Arrange
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: vec![table_row("HEAD")],
      rows: vec![table_row("R0")],
      breakable: true,
    };
    let blocks = vec![Block::Table {
      table,
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!(close(rows[0].boxes[0].x, 40.0), "cell.x={}", rows[0].boxes[0].x.to_pt());
  }

  #[test]
  fn full_width_table_is_not_shifted_by_center() {
    // Arrange
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Ratio(1.0),
      }],
      head: vec![table_row("HEAD")],
      rows: vec![table_row("R0")],
      breakable: true,
    };
    let blocks = vec![Block::Table {
      table,
      align: Align::Center,
    }];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let PlacedBlock::Table { rows } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!(close(rows[0].boxes[0].x, 2.0), "cell.x={}", rows[0].boxes[0].x.to_pt());
  }

  #[test]
  fn no_line_baseline_exceeds_page_limit() {
    let geom = test_geometry();

    let (pages, _) =
      break_pages(vec![paragraph_of_lines(12)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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
  fn composed_line(width: f32, height: f32, depth: f32, link: Option<crate::typeset::boxes::LinkTarget>) -> Block {
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
      line: Line {
        boxes: vec![PositionedBox {
          content: HBoxContent::Rule { width, height },
          x: Length::ZERO,
          dy: Length::ZERO,
          width,
        }],
        height,
        depth,
        is_last: true,
        links,
        footnotes: Vec::new(),
        index_marks: Vec::new(),
      },
      leading: Length::pt(12.0),
    };
  }

  #[test]
  fn composed_line_places_at_baseline_with_leading() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      composed_line(20.0, 8.0, 2.0, None),
      composed_line(20.0, 8.0, 2.0, None),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    use crate::{
      semantics::HeadingKey,
      typeset::boxes::{AnchorId, AnchorMark, LinkTarget},
    };
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading {
        key: HeadingKey::new(0),
        label: None,
      }),
      composed_line(20.0, 8.0, 2.0, Some(LinkTarget::Internal(AnchorId::Heading(HeadingKey::new(5))))),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = test_geometry();
    let blocks: Vec<Block> = (0..5).map(|_| return composed_line(20.0, 8.0, 2.0, None)).collect();

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Line { baseline_y, .. } = pages[1].blocks.first().expect("2 ページ目に行があるはず")
    else {
      panic!("Line を期待");
    };
    assert!(close(*baseline_y, 10.0));
  }

  #[test]
  fn two_column_flow_fills_left_then_right_then_next_page() {
    // Arrange
    let geom = two_column_geometry();

    // Act
    let (pages, _) =
      break_pages(vec![paragraph_of_lines(9)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    let page_lines = |page: &Page| -> Vec<(f32, f32)> {
      return page
        .blocks
        .iter()
        .filter_map(|b| match b {
          PlacedBlock::Line { line, baseline_y } => return Some(((*baseline_y).to_pt(), line.boxes[0].x.to_pt())),
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
    // Arrange
    use crate::typeset::boxes::LinkTarget;
    let geom = two_column_geometry();
    let link_para = Block::Paragraph {
      items: vec![
        HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
        test_box(),
        test_box(),
        HItem::LinkEnd,
      ],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Left,
    };
    let blocks = vec![paragraph_of_lines(5), link_para];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
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
    // Arrange
    let geom = two_column_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: Vec::new(),
      rows: (0..6).map(|i| return table_row(&format!("R{i}"))).collect(),
      breakable: true,
    };

    // Act
    let (pages, _) = break_pages(
      vec![Block::Table {
        table,
        align: Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert
    assert_eq!(pages.len(), 1, "{pages:?}");
    let xs: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Table { rows } => return rows.first()?.boxes.first().map(|positioned| return positioned.x),
        _ => return None,
      })
      .collect();
    assert_eq!(xs.len(), 2, "左段断片 + 右段断片の 2 つ: {xs:?}");
    assert!(close(xs[0], 2.0), "1 つ目（左段）のセル x={}", xs[0].to_pt());
    assert!(close(xs[1], 57.0), "2 つ目（右段）のセル x={}", xs[1].to_pt());
  }

  #[test]
  fn is_content_block_classifies_variants() {
    // Arrange / Act
    assert!(is_content_block(&paragraph_of_lines(1)));
    assert!(is_content_block(&Block::Rule {
      width: Length::pt(1.0),
      height: Length::pt(1.0),
      align: Align::Left,
    }));
    assert!(!is_content_block(&Block::fixed_space(pt(5.0))));
    assert!(!is_content_block(&forbid_break()));
    assert!(!is_content_block(&Block::force_break()));
  }

  #[test]
  fn keep_group_end_links_heading_to_following_block() {
    // Arrange
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(2),
    ];

    // Act / Assert
    assert_eq!(keep_group_end(&blocks, 0), Some(3));
  }

  #[test]
  fn keep_group_end_none_without_forbid() {
    // Arrange
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      paragraph_of_lines(2),
    ];

    // Act / Assert
    assert_eq!(keep_group_end(&blocks, 0), None);
  }

  #[test]
  fn keep_group_end_chains_consecutive_headings() {
    // Arrange
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(2),
    ];

    // Act / Assert
    assert_eq!(keep_group_end(&blocks, 0), Some(6));
  }

  #[test]
  fn keep_group_end_severed_by_forced_break() {
    // Arrange
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      Block::force_break(),
      paragraph_of_lines(2),
    ];

    // Act / Assert
    assert_eq!(keep_group_end(&blocks, 0), None);
  }

  #[test]
  fn heading_kept_with_body_moves_to_next_page() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3), // filler
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&pages), vec![3, 3], "{pages:?}");
  }

  #[test]
  fn heading_without_keep_marker_is_orphaned() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1),
      Block::fixed_space(pt(0.0)),
      paragraph_of_lines(2),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&pages), vec![4, 2], "{pages:?}");
  }

  #[test]
  fn consecutive_headings_move_together() {
    // Arrange
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

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&pages), vec![3, 4], "{pages:?}");
  }

  #[test]
  fn heading_with_fitting_body_stays_in_place() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1), // filler → y=22
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&pages), vec![4], "{pages:?}");
  }

  #[test]
  fn doc_final_heading_without_body_does_not_hang() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1),
      Block::fixed_space(pt(0.0)),
      forbid_break(),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&pages), vec![4], "{pages:?}");
  }

  #[test]
  fn unavoidable_keep_at_region_top_does_not_add_blank_page() {
    // Arrange
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

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(line_counts(&pages), vec![1, 2], "{pages:?}");
  }

  /// 下端揃えを有効にしたテスト用ジオメトリ（他は `test_geometry` と同じ）
  fn flush_geometry() -> PageGeometry {
    return PageGeometry {
      flush_bottom: true,
      ..test_geometry()
    };
  }

  /// 高さ `height` の罫線ブロック（幅 10・左揃え）
  fn rule(height: f32) -> Block {
    return Block::Rule {
      width: Length::pt(10.0),
      height: pt(height),
      align: Align::Left,
    };
  }

  /// ページ内の罫線の上端 y を上から順に集める
  fn rule_ys(page: &Page) -> Vec<Length> {
    return page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Rule { y, .. } => return Some(*y),
        _ => return None,
      })
      .collect();
  }

  /// ページ内ブロックの底辺の最大値（版面下端に達したかの確認用）
  fn max_block_bottom(page: &Page) -> f32 {
    return page
      .blocks
      .iter()
      .map(super::placed_block_bottom)
      .fold(Length::from_sp(i64::MIN), Length::max)
      .to_pt();
  }

  /// 2 つの `f32` がほぼ等しいか（配置座標の比較用）
  fn approx(a: f32, b: f32) -> bool { return (a - b).abs() < 1e-3; }

  #[test]
  fn flush_bottom_distributes_deficit_into_stretch_glue() {
    // Arrange
    let geom = flush_geometry();
    let blocks = vec![
      rule(10.0),                                 // y=10..20
      Block::stretchable_space(pt(4.0), pt(4.0)), // 24
      rule(10.0),                                 // y=24..34
      Block::stretchable_space(pt(4.0), pt(4.0)), // 38
      rule(10.0),                                 // y=38..48（不足 2pt）
      rule(10.0),                                 // 溢れて改ページ（1 ページ目を確定＝下端揃え発火）
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2);
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 25.0, 40.0]));
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "{:?}", pages[0]);
    assert_eq!(rule_ys(&pages[1]), pts(&[10.0]));
  }

  #[test]
  fn flush_bottom_disabled_keeps_ragged_bottom() {
    // Arrange
    let geom = test_geometry();
    let blocks = vec![
      rule(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      rule(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      rule(10.0),
      rule(10.0),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 24.0, 38.0]));
    assert!(approx(max_block_bottom(&pages[0]), 48.0));
  }

  #[test]
  fn flush_bottom_shifts_paragraph_lines() {
    // Arrange
    let geom = flush_geometry();
    let blocks = vec![
      paragraph_of_lines(1),                      // baseline 10（bottom 12）、以後カーソルは +leading(12)
      Block::stretchable_space(pt(4.0), pt(4.0)), // ba=1
      paragraph_of_lines(1),                      // baseline 26
      Block::stretchable_space(pt(4.0), pt(4.0)), // ba=2
      paragraph_of_lines(1),                      // baseline 42（bottom 44・不足 6pt）
      rule(30.0),                                 // 溢れて改ページ
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 29.0, 48.0]));
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "{:?}", pages[0]);
  }

  #[test]
  fn flush_bottom_aligns_last_baseline_across_pages() {
    // Arrange
    let geom = flush_geometry();
    let mut blocks = Vec::new();
    for i in 0..7 {
      if i > 0 {
        blocks.push(Block::stretchable_space(pt(4.0), pt(4.0)));
      }
      blocks.push(rule(10.0));
    }

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 3);
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "page0 {:?}", pages[0]);
    assert!(approx(max_block_bottom(&pages[1]), 50.0), "page1 {:?}", pages[1]);
    assert!(max_block_bottom(&pages[2]) < 50.0 - 1e-3, "last page ragged {:?}", pages[2]);
  }

  #[test]
  fn flush_bottom_skips_page_before_forced_break() {
    // Arrange
    let geom = flush_geometry();
    let blocks = vec![
      rule(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      rule(10.0),
      Block::force_break(), // このページは強制改ページで終わる → 揃えない
      rule(10.0),
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2);
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 24.0]));
    assert_eq!(rule_ys(&pages[1]), pts(&[10.0]));
  }

  #[test]
  fn flush_bottom_skips_page_without_stretch() {
    // Arrange
    let geom = flush_geometry();
    let blocks = vec![
      rule(10.0),
      Block::fixed_space(pt(4.0)),
      rule(10.0),
      Block::fixed_space(pt(4.0)),
      rule(10.0),
      rule(10.0), // 溢れて改ページ
    ];

    // Act
    let (pages, _) = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 24.0, 38.0]));
  }
}
