//! 現在ページの配置台帳 `PageDraft` — ページ帰属データの抽出・下端揃え・`Page` への確定。
//!
//! この module は改ページ可否・widow / orphan・脚注の詰め込み・フォント・style・publication を知らない。
//! 座標系は [`Page`] と同じ（`x` は本文左端、`y` はページ上端からの距離）。

use crate::{
  length::Length,
  typeset::{
    boxes::{
      AnchorId, FootnoteId, IndexTerm, Line, Page, PlacedAnchor, PlacedBlock, PlacedFootnote, PlacedLink,
      PlacedTableRule, TableRowFragment,
    },
    breaking::break_pages::PendingFootnote,
    geometry::PageGeometry,
  },
};

/// 下端揃えで配分を行う不足高さの下限。これ以下は浮動小数の誤差とみなし揃えない。
const FLUSH_EPSILON: Length = Length::from_sp(66);

/// 着地を待つ表の断片 — 1 つのリージョンに積んだ行断片と罫線（y はページ上端から、罫線の x は表左端から）
pub(super) struct PendingTableFragment {
  /// 断片の上端（断片を開いたときのカーソル。上端の枠罫の上端）
  pub(super) top_y: Length,
  /// 行断片と、それぞれの行帯上端の y
  pub(super) rows: Vec<(TableRowFragment, Length)>,
  /// 断片の罫線（上から順）
  pub(super) rules: Vec<PlacedTableRule>,
}

/// 配置順台帳の 1 entry
enum Entry {
  /// 本文 block の着地。下端揃えの対象
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
  /// 内容を伴わない着地点（文書末尾）で解決したアンカー。本文と同じく下端揃えで動くが、
  /// ページの内容にはならない（これだけではページを排出しない）
  Anchors {
    /// 解決したアンカー
    anchors: Vec<PlacedAnchor>,
    /// リージョン内でこの entry より前に通過した伸縮アキの stretch 累積量
    stretch_before: Length,
  },
  /// 確定脚注。下端揃えの対象外
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
  /// 下端揃えの配分: 先行 stretch 累積量 × `ratio` だけ下方へ動かす
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

/// 現在ページの配置台帳
pub(super) struct PageDraft {
  /// 配置順台帳
  entries: Vec<Entry>,
  /// 未解決のアンカー。次の着地点で解決する。ページ確定をまたいで保持する
  pending_anchors: Vec<AnchorId>,
  /// このページの索引語（`(word, reading)` で初出順に重複除去済み）
  index_entries: Vec<IndexTerm>,
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
  pub(super) fn defer_anchor(&mut self, id: AnchorId) { self.pending_anchors.push(id); }

  /// 伸縮アキを通過した（下端揃えの配分重みを累積する）
  pub(super) fn pass_stretch(&mut self, stretch: Length) { self.region_stretch += stretch; }

  /// 未解決アンカーを `(x, y)` で解決して取り出す
  fn take_pending_anchors(&mut self, x: Length, y: Length) -> Vec<PlacedAnchor> {
    return self.pending_anchors.drain(..).map(|id| return PlacedAnchor { id, x, y }).collect();
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
      self.push_index_entry(entry);
    }
    self.push_block(PlacedBlock::Line { line, baseline_y }, anchors, links);
  }

  /// 行・表以外の block（画像・ディスプレイ数式）を置いた。未解決アンカーは `(anchor_x, anchor_y)` で解決する
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

  /// 表の断片（このリージョンに描く行断片と罫線）を段オフセット `column_x` に置いた。行断片と罫線は表左端を
  /// `column_x + align_offset` に置いて 1 つの `PlacedBlock::Table` として積み、リンク矩形・索引語は断片が持つものを
  /// このページへ移す。行の無い断片は何も積まず、未解決アンカーも消費しない
  ///
  /// 未解決アンカー（`\ref{tab:...}` の到達先）は揃えオフセットを含まない断片の上端 `(column_x, top_y)` で解決する。
  /// 先頭行が現在リージョンに収まらず次リージョンへ送られる場合、アンカーはその次リージョンで着地する最初の断片で
  /// 解決される。表は空でない（frontend が空の表を拒否する）ので、どの表も少なくとも 1 つの行を持つ断片を着地させる。
  /// 再掲ヘッダの索引語の重複除去は表エンジンが済ませている（再掲用の断片は索引語を持たない）。
  pub(super) fn place_table_fragment(
    &mut self,
    fragment: PendingTableFragment,
    column_x: Length,
    align_offset: Length,
  ) {
    let PendingTableFragment { top_y, rows, rules } = fragment;
    if rows.is_empty() {
      return;
    }
    let anchors = self.take_pending_anchors(column_x, top_y);
    let x = column_x + align_offset;
    let mut links = Vec::new();
    let mut placed_rows = Vec::with_capacity(rows.len());
    for (fragment, top_y) in rows {
      let (row, row_links, index_marks) = fragment.place(x, top_y);
      for term in &index_marks {
        self.push_index_entry(term);
      }
      links.extend(row_links);
      placed_rows.push(row);
    }
    let rules = rules
      .into_iter()
      .map(|rule| {
        return PlacedTableRule {
          x: rule.x + x,
          ..rule
        };
      })
      .collect();
    self.push_block(
      PlacedBlock::Table {
        rows: placed_rows,
        rules,
      },
      anchors,
      links,
    );
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
  /// 同一ページ内の同じ `(語, reading)` は 1 出現に畳む。同一性は `IndexTerm` の `Eq`。
  fn push_index_entry(&mut self, term: &IndexTerm) {
    if !self.index_entries.contains(term) {
      self.index_entries.push(term.clone());
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

  /// 下端揃え: 現在リージョンの本文 entry を、不足高さ `target_bottom − 本文下端` を先行 stretch
  /// 累積量に比例配分して下方へ動かす
  ///
  /// 不足高さが `FLUSH_EPSILON` 以下、または分母（最後の本文 block の先行累積量）が正でなければ動かさない。
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
  /// ページ間分割が入っても、本文中マーカーからは常に本体の先頭位置へ飛べるようにするため。
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
          id: AnchorId::Footnote(FootnoteId::new(pending.index)),
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
          self.push_index_entry(entry);
        }
        blocks.push(PlacedBlock::Line {
          line,
          baseline_y: baseline,
        });
      }
      top = baseline + prev_depth;
      self.entries.push(Entry::Footnote {
        footnote: PlacedFootnote {
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

/// 行のリンク領域を確定座標の矩形へ展開する（退化矩形の扱いは [`LineLink::place`](crate::typeset::boxes::LineLink::place)）
///
/// 矩形の上端は `baseline_y − height`、高さは `height + depth`（行 box 全体）。
fn line_links(line: &Line, baseline_y: Length) -> Vec<PlacedLink> {
  let top = baseline_y - line.height;
  let height = line.height + line.depth;
  return line.links.iter().filter_map(|link| return link.place(Length::ZERO, top, height)).collect();
}

/// [`PlacedBlock`] の底辺（ページ上端からの距離）を返す
pub(super) fn placed_block_bottom(block: &PlacedBlock) -> Length {
  return match block {
    PlacedBlock::Line { line, baseline_y } => *baseline_y + line.depth,
    PlacedBlock::MathBlock {
      body, baseline_y, ..
    } => *baseline_y + body.depth,
    PlacedBlock::Image { y, height, .. } | PlacedBlock::Rule { y, height, .. } => *y + *height,
    PlacedBlock::Table { rows, rules } => rows
      .iter()
      .map(|row| return row.top_y + row.height)
      .chain(rules.iter().map(|rule| return rule.y + rule.height))
      .fold(Length::from_sp(i64::MIN), Length::max),
  };
}

/// [`PlacedBlock`] とその内部の確定座標を下方へ `dy` だけずらす
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
    PlacedBlock::Table { rows, rules } => {
      for row in rows {
        row.top_y += dy;
        row.baseline_y += dy;
      }
      for rule in rules {
        rule.y += dy;
      }
    },
  }
}

#[cfg(test)]
mod tests {
  use super::{PageDraft, PendingTableFragment};
  use crate::{
    document::NodeId,
    length::Length,
    project::{FontFace, FontType, ProjectPath},
    publication::GlyphRun,
    semantics::LabelId,
    source::SourceId,
    typeset::{
      boxes::{
        AnchorId, HBox, HBoxContent, IndexTerm, Line, LineLink, LinkTarget, Page, PlacedBlock, PlacedHBox,
        PlacedTableRow, PlacedTableRule, TableBoundary, TableRowFragment,
      },
      breaking::break_pages::PendingFootnote,
      geometry::PageGeometry,
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
      num_columns: 1,
      column_gap: Length::ZERO,
      flush_bottom: true,
      footnote_top_margin: Length::ZERO,
      footnote_rule_length: Length::ZERO,
      footnote_rule_thickness: Length::ZERO,
      footnote_rule_color: None,
      footnote_rule_gap: pt(4.0),
      background_color: None,
    };
  }

  /// 高さ 8・深さ 2・幅 20 の行。`link` があれば行全幅のリンクを 1 つ、`index_word` があれば索引語を 1 つ持つ
  fn line(link: Option<LinkTarget>, index_word: Option<&str>) -> Line {
    return Line {
      boxes: Vec::new(),
      height: pt(8.0),
      depth: pt(2.0),
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
          return vec![IndexTerm {
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
      page_index: None,
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
      origin: NodeId::for_test(SourceId::new(0), index),
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
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorId::Label(LabelId::new("a")));
    draft.place_line(line(Some(external("A")), None), pt(10.0), Length::ZERO);
    draft.close_region(&geom, Length::ZERO, pt(36.0), false, vec![footnote(0, "F1", false)]);
    draft.defer_anchor(AnchorId::Label(LabelId::new("b")));
    draft.place_line(line(Some(external("B")), None), pt(10.0), pt(55.0));
    draft.close_region(&geom, pt(55.0), pt(36.0), false, vec![footnote(1, "F2", true)]);

    let page = draft.take_page(&geom);

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
      .map(|a| match &a.id {
        AnchorId::Label(_) => return "label",
        AnchorId::Footnote(_) => return "footnote",
        _ => return "other",
      })
      .collect();
    assert_eq!(anchor_kinds, vec!["label", "footnote", "label"], "繰越断片は脚注アンカーを持たない");
    assert_eq!(page.footnotes.len(), 2);
    assert_eq!(page.blocks.len(), 2);
  }

  #[test]
  fn anchors_alone_are_not_content_and_survive_until_a_block_lands() {
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorId::Label(LabelId::new("tab")));
    draft.land_anchors(Length::ZERO, pt(46.0));
    assert!(!draft.has_content(), "アンカーだけではページ内容にならない");

    draft.close_region(&geom, Length::ZERO, pt(50.0), false, Vec::new());
    draft.place_block(image(10.0, 10.0), Length::ZERO, pt(10.0));
    let page = draft.take_page(&geom);

    assert_eq!(page.anchors.len(), 1);
    assert_eq!(page.anchors[0].y, pt(46.0), "着地点の座標のまま");
    assert_eq!(image_ys(&page), vec![pt(10.0)]);
  }

  #[test]
  fn flush_uses_stretch_before_last_block_as_denominator() {
    // A(10..20), stretch 4, B(24..34), stretch 4（末尾アキ）。不足 50 − 34 = 16、分母は B の先行 4
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_block(image(10.0, 10.0), Length::ZERO, pt(10.0));
    draft.pass_stretch(pt(4.0));
    draft.place_block(image(24.0, 10.0), Length::ZERO, pt(24.0));
    draft.pass_stretch(pt(4.0));

    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());
    let page = draft.take_page(&geom);

    // ratio 4: A は +0、B は 4 × 4 = +16 で下端 50
    assert_eq!(image_ys(&page), vec![pt(10.0), pt(40.0)]);
  }

  #[test]
  fn flush_moves_block_anchor_and_link_of_the_same_entry_together() {
    // 行 1 (baseline 10)、stretch 4、行 2 (baseline 26、アンカー・リンク付き)。不足 50 − 28 = 22、分母 4
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_line(line(None, None), pt(10.0), Length::ZERO);
    draft.pass_stretch(pt(4.0));
    draft.defer_anchor(AnchorId::Label(LabelId::new("x")));
    draft.place_line(line(Some(external("L")), None), pt(26.0), Length::ZERO);

    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());
    let page = draft.take_page(&geom);

    // 行 2 は +22 で baseline 48、アンカーとリンクは行上端 18 + 22 = 40
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
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_block(image(10.0, 10.0), Length::ZERO, pt(10.0));
    draft.place_block(image(24.0, 10.0), Length::ZERO, pt(24.0));

    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());
    let page = draft.take_page(&geom);

    assert_eq!(image_ys(&page), vec![pt(10.0), pt(24.0)]);
  }

  #[test]
  fn footnotes_are_stacked_from_region_limit_and_not_flushed() {
    // 本文 1 block + stretch。脚注 2 個: 1 個目は gap 4 の後、2 個目はさらに gap 4
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.pass_stretch(pt(4.0));
    draft.place_block(image(14.0, 10.0), Length::ZERO, pt(14.0));

    draft.close_region(&geom, pt(5.0), pt(30.0), true, vec![footnote(0, "F1", false), footnote(1, "F2", false)]);
    let page = draft.take_page(&geom);

    // 本文は下端 30 へ +6。脚注 1: top 34、baseline 42、下端 44。脚注 2: top 48、baseline 56
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
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.pass_stretch(pt(4.0));
    draft.place_block(image(14.0, 10.0), Length::ZERO, pt(14.0));
    draft.close_region(&geom, Length::ZERO, pt(50.0), true, Vec::new());

    draft.close_region(&geom, Length::ZERO, pt(50.0), false, Vec::new());
    let page = draft.take_page(&geom);

    // 1 回目の揃え（不足 26、分母 4 → +26）のまま
    assert_eq!(image_ys(&page), vec![pt(40.0)]);
  }

  #[test]
  fn degenerate_line_links_are_dropped() {
    let geom = geometry();
    let mut draft = PageDraft::new();
    let mut degenerate = line(Some(external("D")), None);
    degenerate.links[0].x1 = degenerate.links[0].x0;
    draft.place_line(degenerate, pt(10.0), Length::ZERO);

    let page = draft.take_page(&geom);

    assert!(page.links.is_empty(), "{:?}", page.links);
  }

  /// 幅 20・高さ 10 の箱を表左端 + 余白 2 に置いた、行帯 10・ベースライン 10 の表の行断片。`word` があれば索引語を持つ
  fn table_row(word: Option<&str>) -> TableRowFragment {
    return TableRowFragment {
      row: PlacedTableRow {
        top_y: Length::ZERO,
        height: pt(10.0),
        baseline_y: pt(10.0),
        boxes: vec![PlacedHBox {
          hbox: HBox {
            content: HBoxContent::Glyphs(GlyphRun {
              font_size: pt(10.0),
              text: "x".to_string(),
              glyphs: Vec::new(),
              face: FontFace::primary(FontType::Serif),
              color: None,
            }),
            width: pt(20.0),
            height: pt(10.0),
            depth: Length::ZERO,
          },
          dx: pt(2.0),
          dy: Length::ZERO,
        }],
      },
      origin: NodeId::for_test(SourceId::new(0), 1),
      overrun: Length::ZERO,
      rules: Vec::new(),
      above: TableBoundary::default(),
      links: Vec::new(),
      index_marks: word
        .map(|word| {
          return IndexTerm {
            word: word.to_string(),
            reading: None,
          };
        })
        .into_iter()
        .collect(),
      footnotes: Vec::new(),
    };
  }

  #[test]
  fn index_entries_dedup_across_line_table_and_footnote() {
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.place_line(line(None, Some("語")), pt(10.0), Length::ZERO);
    // 先頭は再掲ヘッダの断片（表エンジンが索引語を除いている）
    draft.place_table_fragment(
      PendingTableFragment {
        top_y: pt(22.0),
        rows: vec![
          (table_row(None), pt(22.0)),
          (table_row(Some("語")), pt(32.0)),
          (table_row(Some("表語")), pt(42.0)),
        ],
        rules: Vec::new(),
      },
      Length::ZERO,
      Length::ZERO,
    );
    let mut in_footnote = footnote(0, "F", false);
    in_footnote.lines[0].index_marks.push(IndexTerm {
      word: "語".to_string(),
      reading: None,
    });

    draft.close_region(&geom, Length::ZERO, pt(36.0), false, vec![in_footnote]);
    let page = draft.take_page(&geom);

    // 「語」は初出 1 件、順序は出現順
    let words: Vec<&str> = page.index_entries.iter().map(|e| return e.word.as_str()).collect();
    assert_eq!(words, vec!["語", "表語"]);
  }

  #[test]
  fn place_block_resolves_anchor_at_the_given_point_not_the_block() {
    // 数式ブロック相当: アンカーは上端 10、block の baseline は 18
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorId::Label(LabelId::new("eq")));
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

    let page = draft.take_page(&geom);

    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(3.0), pt(10.0)));
    assert!(matches!(page.anchors[0].id, AnchorId::Label(_)));
  }

  #[test]
  fn place_table_fragment_resolves_pending_anchor_at_column_x_and_fragment_top() {
    // 断片の上端 19（上端の枠罫の上端）に罫線、行は 22 / 32
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorId::Label(LabelId::new("tab")));
    draft.place_table_fragment(
      PendingTableFragment {
        top_y: pt(19.0),
        rows: vec![
          (table_row(Some("R0")), pt(22.0)),
          (table_row(Some("R1")), pt(32.0)),
        ],
        rules: vec![PlacedTableRule {
          x: Length::ZERO,
          y: pt(19.0),
          width: pt(24.0),
          height: pt(1.0),
          color: None,
        }],
      },
      pt(55.0),
      pt(5.0),
    );
    let page = draft.take_page(&geom);

    assert_eq!(page.anchors.len(), 1, "{:?}", page.anchors);
    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(55.0), pt(19.0)), "段左端 × 断片の上端");
    assert!(matches!(page.anchors[0].id, AnchorId::Label(_)));
    let PlacedBlock::Table { rows, rules } = &page.blocks[0] else {
      unreachable!("place_table_fragment は Table block を積む");
    };
    assert_eq!(rows[0].boxes[0].dx, pt(62.0), "セル x は段オフセット + 揃えオフセット + padding");
    assert_eq!((rules[0].x, rules[0].y), (pt(60.0), pt(19.0)), "罫線 x は段オフセット + 揃えオフセット");
  }

  #[test]
  fn empty_table_fragment_keeps_pending_anchors_for_the_next_landing() {
    let geom = geometry();
    let mut draft = PageDraft::new();
    draft.defer_anchor(AnchorId::Label(LabelId::new("tab")));

    draft.place_table_fragment(
      PendingTableFragment {
        top_y: Length::ZERO,
        rows: Vec::new(),
        rules: Vec::new(),
      },
      Length::ZERO,
      Length::ZERO,
    );
    assert!(!draft.has_content(), "空断片は内容にならない");
    draft.place_block(image(30.0, 10.0), pt(7.0), pt(30.0));
    let page = draft.take_page(&geom);

    assert_eq!(page.anchors.len(), 1, "{:?}", page.anchors);
    assert_eq!((page.anchors[0].x, page.anchors[0].y), (pt(7.0), pt(30.0)), "次の着地点で解決する");
  }
}
