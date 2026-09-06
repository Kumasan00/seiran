//! 現在ページの配置台帳 `PageDraft`（#524）— ページ帰属データの抽出・下端揃え・`Page` への確定。
//!
//! 親の `PageComposer` は改段・改ページの判断と脚注の予約・繰越だけを持ち、「この内容の着地が確定した」
//! 時点でこの型の操作を呼ぶ。台帳の entry は本文 block と、その block から導出した anchor / link を
//! 同じ単位に持つので、帰属データを配置経路ごとに集め直す約束（#515 の登録漏れの形）が要らない。
//! 下端揃え（#169）は entry ごとの「先行 stretch 累積量」から配分するので、block / link / anchor の
//! index を別々に同期する必要もない。
//!
//! この module は改ページ可否・widow / orphan・脚注の詰め込み・フォント・style・publication を知らない。
//! 座標系は [`Page`] と同じ（`x` は本文左端、`y` はページ上端からの距離）。

use crate::{
  length::Length,
  typeset::{
    boxes::{
      AnchorMark, FootnoteId, HItem, Line, Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedIndexEntry,
      PlacedLink, PlacedTableRow, PlacedTableRule, TableColumn, TableRowBox, collect_row_links, max_font_size_in_items,
      position_table_row_boxes,
    },
    breaking::break_pages::{PageGeometry, PendingFootnote},
  },
};

/// 下端揃え（#169）で配分を行う不足高さの下限（pt）。これ未満は浮動小数の誤差とみなし揃えない。
const FLUSH_EPSILON: Length = Length::from_sp(66);

/// 配置順台帳の 1 entry
///
/// 本文・脚注・内容を伴わないアンカーを 1 本の列に持つのは、`Page::anchors` / `Page::links` の順序が
/// 「リージョンごとに本文 → 脚注」の交互だから（脚注はリージョン確定時に積まれる）。2 本に分けると
/// この順が再現できない。
enum Entry {
  /// 本文 block の着地。同じ着地で解決したアンカーと、block から導出したリンク矩形を一緒に持つ。
  /// 下端揃えの対象
  Block {
    /// 確定座標の block
    block: PlacedBlock,
    /// この着地で解決したアンカー
    anchors: Vec<PlacedAnchor>,
    /// block から導出したリンク矩形（退化矩形は除外済み）
    links: Vec<PlacedLink>,
    /// リージョン内でこの entry より前に通過した伸縮アキの stretch 累積量（下端揃えの配分重み）
    stretch_before: Length,
  },
  /// 内容を伴わない着地点（表の先頭・文書末尾）で解決したアンカー。本文と同じく下端揃えで動くが、
  /// ページの内容にはならない（これだけではページを排出しない）
  Anchors {
    /// 解決したアンカー
    anchors: Vec<PlacedAnchor>,
    /// リージョン内でこの entry より前に通過した伸縮アキの stretch 累積量
    stretch_before: Length,
  },
  /// 確定脚注。先頭断片の到達先アンカーと本体行のリンク矩形を持つ。下端揃えの対象外
  Footnote {
    /// 確定座標の脚注（区切り罫線を含む）
    footnote: PlacedFootnote,
    /// 到達先アンカー（繰越でない先頭断片だけが持つ）
    anchor: Option<PlacedAnchor>,
    /// 本体行から導出したリンク矩形
    links: Vec<PlacedLink>,
  },
}

impl Entry {
  /// 下端揃えの配分: 先行 stretch 累積量 × `ratio` だけ下方へ動かす。同じ entry の block / anchor / link は
  /// 同じ量だけ動く
  fn shift_by_stretch(&mut self, ratio: f64) {
    match self {
      Entry::Block {
        block,
        anchors,
        links,
        stretch_before,
      } => {
        let dy = stretch_before.scale(ratio);
        shift_placed_block(block, dy);
        for anchor in anchors {
          anchor.y += dy;
        }
        for link in links {
          link.y += dy;
        }
      },
      Entry::Anchors {
        anchors,
        stretch_before,
      } => {
        let dy = stretch_before.scale(ratio);
        for anchor in anchors {
          anchor.y += dy;
        }
      },
      Entry::Footnote { .. } => {
        unreachable!(
          "脚注 entry はリージョン確定時に積まれ、その直後に region_start が進むので確定中のリージョン範囲には入らない"
        )
      },
    }
  }
}

/// 表断片の確定に要る、表 1 個で固定の幾何（列定義・列幅・セル余白・罫線・段内揃えオフセット）
///
/// 断片ごとに変わる段オフセットは [`PageDraft::place_table_fragment`] の引数。断片の左端 x は
/// `段オフセット + align_offset`、アンカーは揃えオフセット抜きの段オフセットで解決する（行・数式と同じ規則）。
pub(super) struct TableFrame<'a> {
  /// 列定義（揃え）
  pub(super) columns: &'a [TableColumn],
  /// 確定済み列幅
  pub(super) col_widths: &'a [Length],
  /// セル内側余白（左右各）
  pub(super) cell_padding: Length,
  /// 横罫線の太さ
  pub(super) rule_thickness: Length,
  /// 横罫線の色（RGB）。`None` は黒
  pub(super) rule_color: Option<[u8; 3]>,
  /// 段幅の中で表を揃えるための段内オフセット（全幅の表では 0）
  pub(super) align_offset: Length,
}

/// ページ内の着地段が確定するまで保持する表の 1 行
pub(super) struct PendingTableRow {
  /// シェーピング済みの行内容
  pub(super) row: TableRowBox,
  /// 行帯のページ上端からの距離
  pub(super) top_y: Length,
  /// 行帯の高さ
  pub(super) height: Length,
  /// `\head` 行か（改ページのたびに再描画される複製なので索引語を収集しない）
  pub(super) is_head: bool,
}

/// 現在ページの配置台帳
///
/// 内部ベクタは公開しない。親は「行を置いた」「ブロックを置いた」「表断片を置いた」「アンカーを着地させた」
/// 「リージョンを閉じた」「ページを確定した」だけを伝える。
pub(super) struct PageDraft {
  /// 配置順台帳
  entries: Vec<Entry>,
  /// 未解決のアンカー。次の着地点で解決する。ページ確定をまたいで保持する
  pending_anchors: Vec<AnchorMark>,
  /// このページの索引語（`(word, reading)` で初出順に重複除去済み）。座標を持たないので台帳の外
  index_entries: Vec<PlacedIndexEntry>,
  /// 現在リージョンの先頭 index（`entries` 内）。下端揃えはここから末尾までを対象にする
  region_start: usize,
  /// 現在リージョンで通過した伸縮アキの stretch 累積量
  region_stretch: Length,
}

impl PageDraft {
  /// 空のページから始める
  pub(super) fn new() -> Self {
    return PageDraft {
      entries: Vec::new(),
      pending_anchors: Vec::new(),
      index_entries: Vec::new(),
      region_start: 0,
      region_stretch: Length::ZERO,
    };
  }

  /// アンカーを未解決として積む（次の着地点で解決する）
  pub(super) fn defer_anchor(&mut self, mark: AnchorMark) { self.pending_anchors.push(mark); }

  /// 伸縮アキを通過した（下端揃えの配分重みを累積する）
  pub(super) fn pass_stretch(&mut self, stretch: Length) { self.region_stretch += stretch; }

  /// 未解決アンカーを `(x, y)` で解決して取り出す
  fn take_pending_anchors(&mut self, x: Length, y: Length) -> Vec<PlacedAnchor> {
    return self.pending_anchors.drain(..).map(|mark| return PlacedAnchor { mark, x, y }).collect();
  }

  /// 内容を伴わない着地点 `(x, y)`（文書末尾）で未解決アンカーを解決する。未解決が無ければ何もしない
  pub(super) fn land_anchors(&mut self, x: Length, y: Length) {
    if self.pending_anchors.is_empty() {
      return;
    }
    let anchors = self.take_pending_anchors(x, y);
    self.entries.push(Entry::Anchors {
      anchors,
      stretch_before: self.region_stretch,
    });
  }

  /// 本文行を `baseline_y` に置いた。未解決アンカーは行の上端 `(column_x, baseline_y − height)` で解決し、
  /// リンク矩形・索引語は行から導出する
  ///
  /// 段落の 2 行目以降では未解決アンカーは常に空（アンカーは block 境界でしか発行されない）。
  pub(super) fn place_line(&mut self, line: Line, baseline_y: Length, column_x: Length) {
    let anchors = self.take_pending_anchors(column_x, baseline_y - line.height);
    let links = line_links(&line, baseline_y);
    for entry in &line.index_marks {
      self.push_index_entry(&entry.word, entry.reading.as_deref());
    }
    self.push_block(PlacedBlock::Line { line, baseline_y }, anchors, links);
  }

  /// 行・表以外の block（画像・ディスプレイ数式）を置いた。未解決アンカーは `(anchor_x, anchor_y)` で解決する
  ///
  /// アンカー点を block から導かないのは、数式ブロックのアンカーが上端（`baseline_y − height`）で
  /// 解決される一方、`PlacedBlock::MathBlock` は上端を持たないため。
  pub(super) fn place_block(&mut self, block: PlacedBlock, anchor_x: Length, anchor_y: Length) {
    match &block {
      PlacedBlock::Line { .. } | PlacedBlock::Table { .. } => {
        unreachable!("行と表は帰属データの導出が要るので place_line / place_table_fragment が着地させる")
      },
      PlacedBlock::Image { .. } | PlacedBlock::MathBlock { .. } | PlacedBlock::Rule { .. } => {},
    }
    let anchors = self.take_pending_anchors(anchor_x, anchor_y);
    self.push_block(block, anchors, Vec::new());
  }

  /// 表の断片（このリージョンに描く行の並び）を段オフセット `column_x` に置いた。行ごとにセル内容・
  /// リンク・罫線の絶対座標を確定し、1 つの `PlacedBlock::Table` として積む。空の断片は何も積まず、
  /// 未解決アンカーも消費しない
  ///
  /// 未解決アンカー（`\ref{tab:...}` の到達先）は先頭行の上端 `(column_x, rows[0].top_y)` で解決する。
  /// 先頭行が現在リージョンに収まらず空断片のあと次リージョンへ送られる場合、アンカーはその
  /// 次リージョンで着地する最初の断片で解決される（#525）。表は空でない（frontend が空の表を拒否する）
  /// ので、どの表も少なくとも 1 つの空でない断片を着地させる。
  ///
  /// 索引語は `\head` 行を除いて集める（改ページのたびに再描画される複製なので、同じ語をページごとに
  /// 積まない）。リンクは head 行も含めて全行から集める（再描画のたびにクリック矩形が要る）。
  pub(super) fn place_table_fragment(&mut self, rows: Vec<PendingTableRow>, frame: &TableFrame<'_>, column_x: Length) {
    let Some(first_top_y) = rows.first().map(|pending| return pending.top_y) else {
      return;
    };
    let anchors = self.take_pending_anchors(column_x, first_top_y);
    let x = column_x + frame.align_offset;
    let table_width: Length = frame.col_widths.iter().copied().sum();
    for pending in rows.iter().filter(|pending| return !pending.is_head) {
      for cell in &pending.row.cells {
        for item in &cell.items {
          if let HItem::IndexMark { word, reading } = item {
            self.push_index_entry(word, reading.as_deref());
          }
        }
      }
    }
    let mut links = Vec::new();
    let mut placed_rows = Vec::with_capacity(rows.len());
    for pending in rows {
      let mut boxes = position_table_row_boxes(&pending.row, frame.columns, frame.col_widths, frame.cell_padding);
      for positioned in &mut boxes {
        positioned.x += x;
      }
      for link in collect_row_links(&pending.row, frame.columns, frame.col_widths, frame.cell_padding) {
        if link.x1 <= link.x0 {
          continue; // 退化矩形は出力しない（行のリンクと同じ規則）
        }
        links.push(PlacedLink {
          target: link.target,
          x: x + link.x0,
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
        x,
        y: pending.top_y,
        width: table_width,
        height: frame.rule_thickness,
        color: frame.rule_color,
      });
      placed_rows.push(PlacedTableRow {
        top_y: pending.top_y,
        height: pending.height,
        baseline_y: pending.top_y + baseline_offset,
        boxes,
        rule,
      });
    }
    self.push_block(PlacedBlock::Table { rows: placed_rows }, anchors, links);
  }

  /// block entry を台帳へ積む（先行 stretch 累積量を写す）
  fn push_block(&mut self, block: PlacedBlock, anchors: Vec<PlacedAnchor>, links: Vec<PlacedLink>) {
    self.entries.push(Entry::Block {
      block,
      anchors,
      links,
      stretch_before: self.region_stretch,
    });
  }

  /// 索引語 1 件をこのページの索引語集合へ加える
  ///
  /// 同一ページ内の同じ `(語, reading)` は 1 出現に畳む（#246 の規則）。本文行・脚注行・表の
  /// 本体行のどこから来たマーカーも同じページの同じ集合へ入るので、畳みは経路をまたいで効く。
  fn push_index_entry(&mut self, word: &str, reading: Option<&str>) {
    let exists = self.index_entries.iter().any(|e| return e.word == word && e.reading.as_deref() == reading);
    if !exists {
      self.index_entries.push(PlacedIndexEntry {
        word: word.to_string(),
        reading: reading.map(str::to_string),
      });
    }
  }

  /// 現在リージョン（段）を閉じる: `flush` かつ下端揃えが有効なら本文 entry を `region_limit` へ揃え、
  /// そのあとリージョンの脚注を `region_limit` から下へ確定座標で積む。閉じ済みリージョンへの再呼び出し
  /// （脚注なし）は無害
  ///
  /// `region_limit` は脚注の予約高さを差し引いた本文の実効下限（= 脚注エリアの上端）。揃えの目標と
  /// 脚注の開始 y が同じ値なのは偶然ではなく、本文が脚注エリアへ伸びないための規則。
  pub(super) fn close_region(
    &mut self,
    geom: &PageGeometry,
    column_x: Length,
    region_limit: Length,
    flush: bool,
    footnotes: Vec<PendingFootnote>,
  ) {
    if flush && geom.flush_bottom {
      self.flush_region(region_limit);
    }
    self.place_footnotes(geom, column_x, region_limit, footnotes);
    self.region_start = self.entries.len();
    self.region_stretch = Length::ZERO;
  }

  /// 下端揃え（#169）: 現在リージョンの本文 entry を、不足高さ `target_bottom − 本文下端` を先行 stretch
  /// 累積量に比例配分して下方へ動かす
  ///
  /// 分母は**最後の本文 block** の先行累積量（最後の block より後のアキは分母に入れない）。不足高さが
  /// `FLUSH_EPSILON` 以下、または分母が正でなければ動かさない（自然高のまま）。
  fn flush_region(&mut self, target_bottom: Length) {
    let region = &mut self.entries[self.region_start..];
    let Some(last_block) = region.iter().rposition(|entry| return matches!(entry, Entry::Block { .. })) else {
      return;
    };
    let region_bottom = region
      .iter()
      .filter_map(|entry| match entry {
        Entry::Block { block, .. } => return Some(placed_block_bottom(block)),
        Entry::Anchors { .. } | Entry::Footnote { .. } => return None,
      })
      .fold(Length::from_sp(i64::MIN), Length::max);
    let deficit = target_bottom - region_bottom;
    let Entry::Block { stretch_before, .. } = &region[last_block] else {
      unreachable!("last_block は直前の rposition が Block と判定した index");
    };
    let effective = *stretch_before;
    if deficit <= FLUSH_EPSILON || !effective.is_positive() {
      return;
    }
    let ratio = deficit.ratio(effective);
    for entry in region.iter_mut() {
      entry.shift_by_stretch(ratio);
    }
  }

  /// リージョンの脚注を脚注エリア（上端 `region_limit`）へ確定座標で積む
  ///
  /// 区切り罫線は「このリージョンで最初の脚注」の直前にのみ 1 本出す（`top_margin` + 罫線 + `rule_gap`）。
  /// 2 個目以降は `rule_gap` のみを挟む（`FootnoteCharges::entry_overhead` の課金と対称）。
  /// 高さの漸化式は見積り側（`FootnoteDemand::new`）と一致していなければならない（1 行でもずれると
  /// 本文と脚注が重なる）。
  ///
  /// 繰越でない（＝本体先頭の、マーカーを持つ行を含む）断片だけに到達先アンカーを打つ。長い脚注の
  /// ページ間分割（#227）が入っても、本文中マーカーからは常に本体の先頭位置へ飛べるようにするため。
  /// 脚注本体は段幅で行分割されているが x は行頭基準のままなので、着地する段が確定したここで段オフセットを
  /// 足し（#518）、その**後**にリンク矩形・索引語を導出する（クリック矩形が実描画位置と一致する）。
  fn place_footnotes(
    &mut self,
    geom: &PageGeometry,
    column_x: Length,
    region_limit: Length,
    footnotes: Vec<PendingFootnote>,
  ) {
    let mut top = region_limit;
    for (index, pending) in footnotes.into_iter().enumerate() {
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
      let anchor = (!pending.continued).then(|| {
        return PlacedAnchor {
          mark: AnchorMark::Footnote(FootnoteId::new(pending.index)),
          x: column_x,
          y: top,
        };
      });
      let mut links = Vec::new();
      blocks.reserve(pending.lines.len());
      let mut baseline = top + pending.lines.first().map_or(Length::ZERO, |line| return line.height);
      let mut prev_depth = Length::ZERO;
      for (i, mut line) in pending.lines.into_iter().enumerate() {
        if i > 0 {
          baseline += pending.leading.max(prev_depth + line.height);
        }
        prev_depth = line.depth;
        line.shift_x(column_x);
        links.extend(line_links(&line, baseline));
        for entry in &line.index_marks {
          self.push_index_entry(&entry.word, entry.reading.as_deref());
        }
        blocks.push(PlacedBlock::Line {
          line,
          baseline_y: baseline,
        });
      }
      top = baseline + prev_depth;
      self.entries.push(Entry::Footnote {
        footnote: PlacedFootnote {
          number: pending.number,
          index: pending.index,
          continued: pending.continued,
          blocks,
        },
        anchor,
        links,
      });
    }
  }

  /// 本文 block か確定脚注を 1 つでも持つか。帰属データ（アンカー・索引語）だけではページを作らない
  pub(super) fn has_content(&self) -> bool {
    return self
      .entries
      .iter()
      .any(|entry| return matches!(entry, Entry::Block { .. } | Entry::Footnote { .. }));
  }

  /// 台帳を `Page` へ確定して排出し、次ページの空の台帳になる。未解決アンカーは持ち越す
  ///
  /// `Page` の各フィールドは台帳の配置順（リージョンごとに本文 → 脚注）で並ぶ。
  pub(super) fn take_page(&mut self, geom: &PageGeometry) -> Page {
    let entries = std::mem::take(&mut self.entries);
    self.region_start = 0;
    self.region_stretch = Length::ZERO;
    let mut page = Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: Vec::new(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: std::mem::take(&mut self.index_entries),
      background_color: geom.background_color,
      content_origin_x: geom.content_origin_x,
    };
    for entry in entries {
      match entry {
        Entry::Block {
          block,
          anchors,
          links,
          ..
        } => {
          page.blocks.push(block);
          page.anchors.extend(anchors);
          page.links.extend(links);
        },
        Entry::Anchors { anchors, .. } => page.anchors.extend(anchors),
        Entry::Footnote {
          footnote,
          anchor,
          links,
        } => {
          page.footnotes.push(footnote);
          page.anchors.extend(anchor);
          page.links.extend(links);
        },
      }
    }
    return page;
  }
}

/// 行のリンク領域を確定座標の矩形へ展開する（退化矩形 `x1 <= x0` は捨てる）
///
/// 矩形の上端は `baseline_y − height`、高さは `height + depth`（行 box 全体）。
fn line_links(line: &Line, baseline_y: Length) -> Vec<PlacedLink> {
  let top = baseline_y - line.height;
  let height = line.height + line.depth;
  return line
    .links
    .iter()
    .filter(|link| return link.x1 > link.x0)
    .map(|link| {
      return PlacedLink {
        target: link.target.clone(),
        x: link.x0,
        y: top,
        width: link.x1 - link.x0,
        height,
      };
    })
    .collect();
}

/// [`PlacedBlock`] の底辺（ページ上端からの距離、pt）を返す。下端揃えのリージョン下端算出に使う。
pub(super) fn placed_block_bottom(block: &PlacedBlock) -> Length {
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

#[cfg(test)]
mod tests {
  use super::{PageDraft, PendingTableRow, TableFrame};
  use crate::{
    document::{ColumnAlign, ColumnWidth},
    length::Length,
    project::{FontType, ProjectPath},
    semantics::LabelId,
    typeset::{
      boxes::{
        AnchorMark, HBox, HBoxContent, HItem, Line, LineIndexEntry, LineLink, LinkTarget, Page, PlacedBlock,
        TableCellBox, TableColumn, TableRowBox,
      },
      breaking::break_pages::{PageGeometry, PendingFootnote},
      font::GlyphRun,
    },
  };

  /// pt 値から `Length` を作る短縮子
  fn pt(value: f32) -> Length { return Length::pt(value); }

  /// テスト用ジオメトリ（`margin_top=10`, `page_limit=50`、単段、下端揃え有効、脚注 gap 4）
  fn geometry() -> PageGeometry {
    return PageGeometry {
      content_origin_x: Length::ZERO,
      margin_top: pt(10.0),
      page_limit: pt(50.0),
      default_font_size: pt(10.0),
      line_height_factor: 1.0,
      table_cell_padding: pt(2.0),
      num_columns: 1,
      column_gap: Length::ZERO,
      flush_bottom: true,
      footnote_top_margin: Length::ZERO,
      footnote_rule_length: Length::ZERO,
      footnote_rule_thickness: Length::ZERO,
      footnote_rule_color: None,
      footnote_rule_gap: pt(4.0),
      table_rule_thickness: Length::ZERO,
      table_rule_color: None,
      background_color: None,
    };
  }

  /// 高さ 8・深さ 2・幅 20 の行。`link` があれば行全幅のリンクを 1 つ、`index_word` があれば索引語を 1 つ持つ
  fn line(link: Option<LinkTarget>, index_word: Option<&str>) -> Line {
    return Line {
      boxes: Vec::new(),
      height: pt(8.0),
      depth: pt(2.0),
      is_last: true,
      links: link
        .map(|target| {
          return vec![LineLink {
            target,
            x0: Length::ZERO,
            x1: pt(20.0),
          }];
        })
        .unwrap_or_default(),
      footnotes: Vec::new(),
      index_marks: index_word
        .map(|word| {
          return vec![LineIndexEntry {
            word: word.to_string(),
            reading: None,
          }];
        })
        .unwrap_or_default(),
    };
  }

  /// 外部リンク `uri` の行き先
  fn external(uri: &str) -> LinkTarget { return LinkTarget::External(uri.to_string()); }

  /// 高さ `height` の画像 block（上端 `y`）
  fn image(y: f32, height: f32) -> PlacedBlock {
    return PlacedBlock::Image {
      path: ProjectPath::new("fixture.png"),
      x: Length::ZERO,
      y: pt(y),
      width: pt(10.0),
      height: pt(height),
      target_dpi: None,
    };
  }

  /// 1 行（リンク `uri` 付き）の脚注。`continued` で繰越断片にできる
  fn footnote(index: u32, uri: &str, continued: bool) -> PendingFootnote {
    return PendingFootnote {
      number: index + 1,
      index,
      continued,
      lines: vec![line(Some(external(uri)), None)],
      leading: pt(12.0),
    };
  }

  /// ページの画像 block の上端 y を上から順に集める
  fn image_ys(page: &Page) -> Vec<Length> {
    return page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Image { y, .. } => return Some(*y),
        _ => return None,
      })
      .collect();
  }

  #[test]
  fn page_orders_anchors_and_links_body_then_footnote_per_region() {
    // Arrange — リージョン 1: 本文行(リンク A) + 脚注(リンク F1)、リージョン 2: 本文行(リンク B) + 脚注(リンク F2)
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorMark::Label(LabelId::new("a")));
    draft.place_line(line(Some(external("A")), None), pt(10.0), Length::ZERO);
    draft.close_region(&geom, Length::ZERO, pt(36.0), false, vec![footnote(0, "F1", false)]);
    draft.defer_anchor(AnchorMark::Label(LabelId::new("b")));
    draft.place_line(line(Some(external("B")), None), pt(10.0), pt(55.0));
    draft.close_region(&geom, pt(55.0), pt(36.0), false, vec![footnote(1, "F2", true)]);

    // Act
    let page = draft.take_page(&geom);

    // Assert
    let link_uris: Vec<&str> = page
      .links
      .iter()
      .map(|l| match &l.target {
        LinkTarget::External(uri) => return uri.as_str(),
        LinkTarget::Internal(_) => return "internal",
      })
      .collect();
    assert_eq!(link_uris, vec!["A", "F1", "B", "F2"], "リージョンごとに本文 → 脚注の順");
    let anchor_kinds: Vec<&str> = page
      .anchors
      .iter()
      .map(|a| match &a.mark {
        AnchorMark::Label(_) => return "label",
        AnchorMark::Footnote(_) => return "footnote",
        _ => return "other",
      })
      .collect();
    assert_eq!(anchor_kinds, vec!["label", "footnote", "label"], "繰越断片は脚注アンカーを持たない");
    assert_eq!(page.footnotes.len(), 2);
    assert_eq!(page.blocks.len(), 2);
  }

  #[test]
  fn anchors_alone_are_not_content_and_survive_until_a_block_lands() {
    // Arrange
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorMark::Label(LabelId::new("tab")));
    draft.land_anchors(Length::ZERO, pt(46.0));
    assert!(!draft.has_content(), "アンカーだけではページ内容にならない");

    // Act — 内容が無いのでページは取らず、次のリージョンで block を置く
    draft.close_region(&geom, Length::ZERO, pt(50.0), false, Vec::new());
    draft.place_block(image(10.0, 10.0), Length::ZERO, pt(10.0));
    let page = draft.take_page(&geom);

    // Assert
    assert_eq!(page.anchors.len(), 1);
    assert_eq!(page.anchors[0].y, pt(46.0), "着地点の座標のまま");
    assert_eq!(image_ys(&page), vec![pt(10.0)]);
  }

  #[test]
  fn flush_uses_stretch_before_last_block_as_denominator() {
    // Arrange — A(10..20), stretch 4, B(24..34), stretch 4（末尾アキ）。不足 50 − 34 = 16、分母は B の先行 4
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_block(image(10.0, 10.0), Length::ZERO, pt(10.0));
    draft.pass_stretch(pt(4.0));
    draft.place_block(image(24.0, 10.0), Length::ZERO, pt(24.0));
    draft.pass_stretch(pt(4.0));

    // Act
    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());
    let page = draft.take_page(&geom);

    // Assert — ratio 4: A は +0、B は 4 × 4 = +16 で下端 50
    assert_eq!(image_ys(&page), vec![pt(10.0), pt(40.0)]);
  }

  #[test]
  fn flush_moves_block_anchor_and_link_of_the_same_entry_together() {
    // Arrange — 行 1 (baseline 10)、stretch 4、行 2 (baseline 26、アンカー・リンク付き)。不足 50 − 28 = 22、分母 4
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_line(line(None, None), pt(10.0), Length::ZERO);
    draft.pass_stretch(pt(4.0));
    draft.defer_anchor(AnchorMark::Label(LabelId::new("x")));
    draft.place_line(line(Some(external("L")), None), pt(26.0), Length::ZERO);

    // Act
    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());
    let page = draft.take_page(&geom);

    // Assert — 行 2 は +22 で baseline 48、アンカーとリンクは行上端 18 + 22 = 40
    let baselines: Vec<Length> = page
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
    assert_eq!(baselines, vec![pt(10.0), pt(48.0)]);
    assert_eq!(page.anchors[0].y, pt(40.0));
    assert_eq!(page.links[0].y, pt(40.0));
  }

  #[test]
  fn flush_skips_region_without_stretch() {
    // Arrange — stretch 無し
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_block(image(10.0, 10.0), Length::ZERO, pt(10.0));
    draft.place_block(image(24.0, 10.0), Length::ZERO, pt(24.0));

    // Act
    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());
    let page = draft.take_page(&geom);

    // Assert
    assert_eq!(image_ys(&page), vec![pt(10.0), pt(24.0)]);
  }

  #[test]
  fn footnotes_are_stacked_from_region_limit_and_not_flushed() {
    // Arrange — 本文 1 block + stretch。脚注 2 個: 1 個目は gap 4 の後、2 個目はさらに gap 4
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.pass_stretch(pt(4.0));
    draft.place_block(image(14.0, 10.0), Length::ZERO, pt(14.0));

    // Act
    draft.close_region(&geom, pt(5.0), pt(30.0), true, vec![footnote(0, "F1", false), footnote(1, "F2", false)]);
    let page = draft.take_page(&geom);

    // Assert — 本文は下端 30 へ +6。脚注 1: top 34、baseline 42、下端 44。脚注 2: top 48、baseline 56
    assert_eq!(image_ys(&page), vec![pt(20.0)]);
    let footnote_baselines: Vec<Length> = page
      .footnotes
      .iter()
      .flat_map(|f| return &f.blocks)
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
    assert_eq!(footnote_baselines, vec![pt(42.0), pt(56.0)]);
    assert_eq!(page.anchors.len(), 2);
    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(5.0), pt(34.0)), "段オフセットと脚注上端");
    assert_eq!(page.links.len(), 2);
    assert_eq!((page.links[0].x, page.links[0].y), (pt(5.0), pt(34.0)), "行は段オフセット後にリンクを導出");
  }

  #[test]
  fn closing_an_already_closed_region_changes_nothing() {
    // Arrange
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.pass_stretch(pt(4.0));
    draft.place_block(image(14.0, 10.0), Length::ZERO, pt(14.0));
    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());

    // Act — 強制改ページ経路の二重 close
    draft.close_region(&geom, Length::ZERO, pt(50.0), false, Vec::new());
    let page = draft.take_page(&geom);

    // Assert — 1 回目の揃え（不足 26、分母 4 → +26）のまま
    assert_eq!(image_ys(&page), vec![pt(40.0)]);
  }

  #[test]
  fn degenerate_line_links_are_dropped() {
    // Arrange
    let geom = geometry();
    let mut draft = PageDraft::new();
    let mut degenerate = line(Some(external("D")), None);
    degenerate.links[0].x1 = degenerate.links[0].x0;
    draft.place_line(degenerate, pt(10.0), Length::ZERO);

    // Act
    let page = draft.take_page(&geom);

    // Assert
    assert!(page.links.is_empty(), "{:?}", page.links);
  }

  /// 1 列 1 セル、テキスト箱 1 つと索引語 `word` を持つ表の行を作る
  fn row_with_index(word: &str) -> TableRowBox {
    return TableRowBox {
      cells: vec![TableCellBox {
        items: vec![
          HItem::Box(HBox {
            content: HBoxContent::Glyphs(GlyphRun {
              font_size: pt(10.0),
              text: "x".to_string(),
              glyphs: Vec::new(),
              font_type: FontType::Serif,
              color: None,
            }),
            width: pt(20.0),
            height: pt(10.0),
            depth: Length::ZERO,
          }),
          HItem::IndexMark {
            word: word.to_string(),
            reading: None,
          },
        ],
        span: 1,
      }],
      rule_above: false,
    };
  }

  #[test]
  fn index_entries_dedup_across_line_table_and_footnote() {
    // Arrange
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_line(line(None, Some("語")), pt(10.0), Length::ZERO);
    let columns = vec![TableColumn {
      align: ColumnAlign::Left,
      width: ColumnWidth::Auto,
    }];
    let col_widths = vec![pt(30.0)];
    let frame = TableFrame {
      columns: &columns,
      col_widths: &col_widths,
      cell_padding: pt(2.0),
      rule_thickness: Length::ZERO,
      rule_color: None,
      align_offset: Length::ZERO,
    };
    draft.place_table_fragment(
      vec![
        PendingTableRow {
          row: row_with_index("ヘッダ語"),
          top_y: pt(22.0),
          height: pt(10.0),
          is_head: true,
        },
        PendingTableRow {
          row: row_with_index("語"),
          top_y: pt(32.0),
          height: pt(10.0),
          is_head: false,
        },
        PendingTableRow {
          row: row_with_index("表語"),
          top_y: pt(42.0),
          height: pt(10.0),
          is_head: false,
        },
      ],
      &frame,
      Length::ZERO,
    );
    let mut in_footnote = footnote(0, "F", false);
    in_footnote.lines[0].index_marks.push(LineIndexEntry {
      word: "語".to_string(),
      reading: None,
    });

    // Act
    draft.close_region(&geom, Length::ZERO, pt(36.0), false, vec![in_footnote]);
    let page = draft.take_page(&geom);

    // Assert — head の語は集めず、「語」は初出 1 件、順序は出現順
    let words: Vec<&str> = page.index_entries.iter().map(|e| return e.word.as_str()).collect();
    assert_eq!(words, vec!["語", "表語"]);
  }

  #[test]
  fn empty_table_fragment_pushes_nothing() {
    // Arrange
    let geom = geometry();
    let mut draft = PageDraft::new();
    let columns: Vec<TableColumn> = Vec::new();
    let col_widths: Vec<Length> = Vec::new();
    let frame = TableFrame {
      columns: &columns,
      col_widths: &col_widths,
      cell_padding: Length::ZERO,
      rule_thickness: Length::ZERO,
      rule_color: None,
      align_offset: Length::ZERO,
    };

    // Act
    draft.place_table_fragment(Vec::new(), &frame, Length::ZERO);

    // Assert
    assert!(!draft.has_content());
    let page = draft.take_page(&geom);
    assert!(page.blocks.is_empty());
  }

  #[test]
  fn place_block_resolves_anchor_at_the_given_point_not_the_block() {
    // Arrange — 数式ブロック相当: アンカーは上端 10、block の baseline は 18
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorMark::Label(LabelId::new("eq")));
    draft.place_block(
      PlacedBlock::MathBlock {
        body: HBox {
          content: HBoxContent::Atom(Vec::new()),
          width: pt(20.0),
          height: pt(8.0),
          depth: pt(2.0),
        },
        x: Length::ZERO,
        baseline_y: pt(18.0),
        numbers: Vec::new(),
      },
      pt(3.0),
      pt(10.0),
    );

    // Act
    let page = draft.take_page(&geom);

    // Assert
    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(3.0), pt(10.0)));
    assert!(matches!(page.anchors[0].mark, AnchorMark::Label(_)));
  }

  #[test]
  fn place_table_fragment_resolves_pending_anchor_at_column_x_and_first_row_top() {
    // Arrange — 段オフセット 55・揃えオフセット 5 の表断片。アンカーは揃えオフセット抜きの段左端 × 先頭行上端
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorMark::Label(LabelId::new("tab")));
    let columns = vec![TableColumn {
      align: ColumnAlign::Left,
      width: ColumnWidth::Auto,
    }];
    let col_widths = vec![pt(30.0)];
    let frame = TableFrame {
      columns: &columns,
      col_widths: &col_widths,
      cell_padding: pt(2.0),
      rule_thickness: Length::ZERO,
      rule_color: None,
      align_offset: pt(5.0),
    };

    // Act
    draft.place_table_fragment(
      vec![
        PendingTableRow {
          row: row_with_index("R0"),
          top_y: pt(22.0),
          height: pt(10.0),
          is_head: false,
        },
        PendingTableRow {
          row: row_with_index("R1"),
          top_y: pt(32.0),
          height: pt(10.0),
          is_head: false,
        },
      ],
      &frame,
      pt(55.0),
    );
    let page = draft.take_page(&geom);

    // Assert
    assert_eq!(page.anchors.len(), 1, "{:?}", page.anchors);
    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(55.0), pt(22.0)), "段左端 × 先頭行の上端");
    assert!(matches!(page.anchors[0].mark, AnchorMark::Label(_)));
    let PlacedBlock::Table { rows } = &page.blocks[0] else {
      unreachable!("place_table_fragment は Table block を積む");
    };
    assert_eq!(rows[0].boxes[0].x, pt(62.0), "セル x は段オフセット + 揃えオフセット + padding");
  }

  #[test]
  fn empty_table_fragment_keeps_pending_anchors_for_the_next_landing() {
    // Arrange — 先頭行が収まらないときの `flush(空)` 相当。空断片はアンカーを消費せず、次の着地点で解決する
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorMark::Label(LabelId::new("tab")));
    let columns: Vec<TableColumn> = Vec::new();
    let col_widths: Vec<Length> = Vec::new();
    let frame = TableFrame {
      columns: &columns,
      col_widths: &col_widths,
      cell_padding: Length::ZERO,
      rule_thickness: Length::ZERO,
      rule_color: None,
      align_offset: Length::ZERO,
    };

    // Act
    draft.place_table_fragment(Vec::new(), &frame, Length::ZERO);
    assert!(!draft.has_content(), "空断片は内容にならない");
    draft.place_block(image(30.0, 10.0), pt(7.0), pt(30.0));
    let page = draft.take_page(&geom);

    // Assert
    assert_eq!(page.anchors.len(), 1, "{:?}", page.anchors);
    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(7.0), pt(30.0)), "次の着地点で解決する");
  }
}
