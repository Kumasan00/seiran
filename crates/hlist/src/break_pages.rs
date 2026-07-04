//! (d) 縦組版 — ブロック列をページへ配置する
//!
//! [`break_pages`] が `Vec<Block>` を走査し、段落は [`LineBreaker`] で行に割って
//! ベースライン送り・改ページ・表の行分割（ヘッダ再描画を含む）を確定する。
//! box が計測済みの寸法を持つため、このパスはフォントに一切触れない。
//!
//! ## カーソルの意味
//!
//! カーソル `y` は基本的に「次のベースライン位置」を表す。
//! ただし画像・表・罫線はブロックの底辺で終わるため、直後の段落はベースラインを
//! 1 行分のアセント（`line.height`）だけ下げて重なりを防ぐ。

use tracing::debug;
use types::{AnchorMark, TextAlignment};

use crate::{
  block::{Block, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK},
  break_lines::LineBreaker,
  line::Line,
  page::{Page, PlacedAnchor, PlacedBlock, PlacedLink, PlacedMathNumber, PlacedTableRow},
  table_box::{TableBox, resolve_column_widths, table_row_height},
};

/// ページの物理ジオメトリと既定の行送りパラメータ
///
/// `read_config` / `read_style` に依存しないよう、呼び出し側（`build_pdf`）が
/// 設定から組み立てて渡す。
#[derive(Debug, Clone, Copy)]
pub struct PageGeometry {
  /// 上マージン（pt）。ページ先頭のベースライン位置
  pub margin_top: f32,
  /// 本文下限（pt）= ページ高さ − 下マージン。超えると改ページ（または改段）
  pub page_limit: f32,
  /// 既定フォントサイズ（pt）。表の行高のフォールバックに使用
  pub default_font_size: f32,
  /// 行高係数。表の行高の算出に使用
  pub line_height_factor: f32,
  /// 表セルの内側余白（pt、左右各）。列幅の解決に使用
  pub table_cell_padding: f32,
  /// 段組み数（1 = 単段）。本文を左段 → 右段 → 次ページの順に流す段の本数
  pub num_columns: usize,
  /// 段間（gutter、pt）。隣り合う段の間隔
  pub column_gap: f32,
}

/// 本文幅 `text_width` を `num_columns` 段に分けたときの 1 段あたりの幅（pt）を返す。
///
/// `(text_width - (num_columns - 1) * column_gap) / num_columns`。`build_pdf` の `resolve_images`
/// と `break_pages` の双方がこの 1 つの式を真実源として段幅を求める（重複した算出を避ける）。
#[must_use]
pub fn column_width(text_width: f32, num_columns: usize, column_gap: f32) -> f32 {
  // 段数は実用上 1〜2。f32 で精度を失う桁数にはならない
  #[allow(clippy::cast_precision_loss)]
  let n = num_columns.max(1) as f32;
  return (text_width - (n - 1.0) * column_gap) / n;
}

/// 縦組版の内部状態（現在ページ・カーソル）
struct PageComposer {
  pages: Vec<Page>,
  current: Vec<PlacedBlock>,
  /// 現在ページに解決済みのリンク到達先アンカー（機構 A）
  current_anchors: Vec<PlacedAnchor>,
  /// 現在ページに確定済みのクリック可能リンク領域（機構 B）
  current_links: Vec<PlacedLink>,
  /// 未解決のアンカー。次に配置される実ブロックの確定座標で解決する
  pending_anchors: Vec<AnchorMark>,
  /// カーソル位置（ページ上端からの距離、pt）。基本は「次のベースライン位置」
  y: f32,
  /// 直前のブロックが底辺基準（画像・表・罫線）で終わったか
  ///
  /// `true` のとき、次の段落の先頭行はベースラインをアセント分だけ下げる。
  cursor_at_edge: bool,
  /// 段組み数（1 = 単段）
  num_columns: usize,
  /// 1 段あたりの幅（pt）。行分割・揃え・表の列幅解決に使う
  column_width: f32,
  /// 段間（gutter、pt）
  column_gap: f32,
  /// 現在の段インデックス（0 = 左段）。`column_offset` の算出に使う
  col: usize,
  /// 直前の [`Block::Penalty`] から引き継いだ分割コスト（次の内容ブロックの改ページ判定で参照）
  ///
  /// 内容ブロックを配置するたびに [`PageComposer::take_pending_penalty`] で読み取ってリセットする。
  /// 既定 0（中立）。強制改ページ（−∞）は eager に処理するためここには積まない。本 issue（#166）では
  /// 発行者がいないため常に 0 で、[`PageComposer::consider_break`] は幾何判定のみに帰着する。
  pending_penalty: i32,
}

impl PageComposer {
  fn new(geom: &PageGeometry, column_width: f32) -> Self {
    return PageComposer {
      pages: Vec::new(),
      current: Vec::new(),
      current_anchors: Vec::new(),
      current_links: Vec::new(),
      pending_anchors: Vec::new(),
      y: geom.margin_top,
      cursor_at_edge: false,
      num_columns: geom.num_columns.max(1),
      column_width,
      column_gap: geom.column_gap,
      col: 0,
      pending_penalty: 0,
    };
  }

  /// 現在の段の左端 x オフセット（本文左端基準、pt）。段 `k` は `k * (段幅 + 段間)` だけ右へ寄る
  fn column_offset(&self) -> f32 {
    // 段インデックスは実用上 0〜1。f32 で精度を失う桁数にはならない
    #[allow(clippy::cast_precision_loss)]
    let col = self.col as f32;
    return col * (self.column_width + self.column_gap);
  }

  /// ページ下限を超えたときの遷移。次の段があれば改段、なければ改ページする。
  ///
  /// 改段はページを送らず、段インデックスを進めてカーソルを上端へ戻すだけ
  /// （次段の先頭は段落先頭行と同じ扱いにするため `cursor_at_edge` も倒す）。最終段で
  /// 超えたときだけ [`PageComposer::start_new_page`] へフォールスルーして実際に改ページする。
  fn advance_region(&mut self, geom: &PageGeometry) {
    if self.col + 1 < self.num_columns {
      self.col += 1;
      self.y = geom.margin_top;
      self.cursor_at_edge = false;
    } else {
      self.start_new_page(geom);
    }
  }

  /// 直前の [`Block::Penalty`] から引き継いだ分割コストを読み取ってリセットする。
  fn take_pending_penalty(&mut self) -> i32 { return std::mem::replace(&mut self.pending_penalty, 0); }

  /// ブロック配置前の改ページ判定（分割コスト参照の一本化ポイント）。
  ///
  /// `next_height` は次に置く内容ブロックの高さ、`penalty` は直前の境界の分割コスト
  /// （[`PageComposer::take_pending_penalty`] の戻り値）。判定は次の一本:
  ///
  /// - [`PENALTY_FORBID_BREAK`]（+∞）: 分割禁止。オーバーフローしても切らない（keep-with-next・#168 が発行）。
  /// - それ以外: 高さがページ下限を超えるなら [`PageComposer::advance_region`] で改段 / 改ページする（幾何判定）。
  ///
  /// 強制改ページ（[`PENALTY_FORCE_BREAK`]）はここを通さず [`break_pages`] の match アームで eager に処理する
  /// （直前のアキが旧ページ末尾に乗るのを避けるため）。本 issue（#166）では有限 / 禁止 penalty の発行者が
  /// いないため常に `penalty == 0` で、実質は幾何判定のみ。有限 penalty を使う widow/orphan（#167）は
  /// ここに badness 判定を足す。
  fn consider_break(&mut self, next_height: f32, penalty: i32, geom: &PageGeometry) {
    if penalty == PENALTY_FORBID_BREAK {
      return;
    }
    if self.y + next_height > geom.page_limit {
      self.advance_region(geom);
    }
  }

  /// 現在ページを確定し、新しいページを開始する
  ///
  /// 未解決アンカー（`pending_anchors`）は引き継ぐ。次の実ブロックがこの新ページに
  /// 配置されたときに解決されるため、移動はしない。
  ///
  /// 現在ページにまだ本文ブロックが 1 つも置かれていない場合は何もしない（ページを
  /// 送らない）。これにより、先頭が見出しのときの白紙の先頭ページや、内容を挟まない
  /// 連続改ページ（Part の `page_break_after` の直後に Chapter の `page_break_before`
  /// が続く等）による中間の白紙ページが生じない。判定は本文ブロック（`current`）の
  /// 有無のみで行う。走り文はページ数確定後の別パスで載るため判定に含めず、ゼロサイズの
  /// アンカー・リンクも本文ブロックではないため含めない。
  fn start_new_page(&mut self, geom: &PageGeometry) {
    if self.current.is_empty() {
      return;
    }
    self.pages.push(Page {
      blocks: std::mem::take(&mut self.current),
      header: Vec::new(),
      footer: Vec::new(),
      anchors: std::mem::take(&mut self.current_anchors),
      links: std::mem::take(&mut self.current_links),
    });
    self.y = geom.margin_top;
    self.cursor_at_edge = false;
    self.col = 0;
  }

  /// 未解決アンカーを確定座標 `(x, y)` で現在ページに解決する
  fn resolve_pending_anchors(&mut self, x: f32, y: f32) {
    for mark in self.pending_anchors.drain(..) {
      self.current_anchors.push(PlacedAnchor { mark, x, y });
    }
  }

  /// 行のリンク領域を確定座標へ展開し、現在ページに追加する
  ///
  /// `baseline_y` は行のベースライン。矩形は行の `height` / `depth` で縦範囲を取る。
  /// 退化した（幅 0 以下の）矩形はスキップする。
  fn collect_line_links(&mut self, line: &Line, baseline_y: f32) {
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

  /// 全ブロックの配置後に最終ページを確定して返す
  fn finish(mut self) -> Vec<Page> {
    // 末尾に残った未解決アンカーは現在カーソル位置（現在の段の左端）で解決する
    let y = self.y;
    let x = self.column_offset();
    self.resolve_pending_anchors(x, y);
    self.pages.push(Page {
      blocks: self.current,
      header: Vec::new(),
      footer: Vec::new(),
      anchors: self.current_anchors,
      links: self.current_links,
    });
    return self.pages;
  }
}

/// ブロック列をページへ配置する
///
/// # Arguments
///
/// * `blocks` - 配置するブロック列（画像サイズは解決済みであること）
/// * `text_width` - 本文幅（pt）。段組み時は段数で割って 1 段あたりの幅（`col_width`）を求める
/// * `geom` - ページジオメトリ（段組み数・段間を含む）
/// * `breaker` - 行分割アルゴリズム
/// * `alignment` - 本文段落の行末処理（`style.toml` の `[text] alignment`）。両端揃えは
///   左揃え段落にのみ適用し、中央・右寄せ段落（[`types::Align::Center`] / [`types::Align::Right`]）は
///   伸縮しない
///
/// 段組み時は本文を左段 → 右段 → 次ページの順に流す。各ブロックは 1 段あたりの幅 `col_width` で
/// 行分割・揃え・列幅解決を行い、確定 x には現在段の左端オフセット（[`PageComposer::column_offset`]）を
/// 加える。段下限を超えると [`PageComposer::advance_region`] が改段または改ページする。単段
/// （`num_columns == 1`）では `col_width == text_width`・オフセット 0 となり、従来と同一の出力になる。
#[must_use]
pub fn break_pages(
  blocks: Vec<Block>,
  text_width: f32,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
) -> Vec<Page> {
  let col_width = column_width(text_width, geom.num_columns, geom.column_gap);
  let mut composer = PageComposer::new(geom, col_width);
  let block_count = blocks.len();

  for block in blocks {
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
      // 伸縮アキ。本 issue（#166）では stretch / shrink を消費せず自然値のみカーソルへ加算する
      // （VSpace と同一挙動）。cursor_at_edge は触らない（アキはフラグを変えない）。
      Block::Glue { natural, .. } => {
        composer.y += natural;
      },
      // 分割コスト。強制改ページ（−∞）は eager に改ページ。有限 / 禁止は次の内容ブロックへ持ち越す。
      Block::Penalty { value } => {
        if value == PENALTY_FORCE_BREAK {
          composer.start_new_page(geom);
        } else {
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
        // 改段・改ページ後の段オフセットを読む（着地段に合わせる）
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
        let width = width.unwrap_or(0.0);
        let height = height.unwrap_or(0.0);
        let penalty = composer.take_pending_penalty();
        composer.consider_break(height, penalty, geom);
        // 改段・改ページ後の段オフセットを読む（着地段に合わせる）
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
      Block::MathBlock {
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
  }

  let pages = composer.finish();
  debug!(block_count, page_count = pages.len(), "ページ分割が完了しました");
  return pages;
}

/// 段落を行に割ってベースライン送りで配置する
///
/// ベースライン送り規則:
/// - 段落先頭行の baseline = 現在のカーソル `y`（直前が底辺基準ブロックならアセント分下げる）
/// - 2 行目以降は `baseline += max(leading, prev.depth + line.height)`
/// - 最終行を置いた後、カーソルを `last_baseline + leading` まで進める
/// - `baseline + line.depth > page_limit` で改段または改ページし、先頭の baseline は `margin_top`
///
/// `column_width` は 1 段あたりの幅（単段では本文幅）。`indent` / `right_indent`（段左右端からの
/// インデント、pt）は折り返し幅を `column_width - indent - right_indent` に縮め、確定した各行の
/// ボックス・リンク矩形を一律 `indent`（＋揃えオフセット）だけ段内で右へシフトする。
///
/// 段組みでは、改段・改ページの確定後にその行が属する段の左端オフセット
/// （[`PageComposer::column_offset`]）を**行ごとに**加算する。段落は段 0 → 段 1 へまたいで流れ得るため、
/// 事前の一括シフトに段オフセットを畳み込むと段 1 の行が左端に重なってしまう。段内シフト
/// （`indent + align`、`[0, column_width]`）と段オフセットを分けるのが要点。
///
/// `align` は確定した各行を利用可能幅（`column_width - indent - right_indent`）の中で水平にシフトする。
/// 中央寄せは `(利用可能幅 − 行幅) / 2`、右寄せは `利用可能幅 − 行幅` を `indent` に加算する。
/// 行幅が利用可能幅を超える場合のシフト量は 0 にクランプする（段左端からはみ出さない）。
///
/// `alignment` は行末処理（両端揃え / 左揃え）。両端揃えは左揃え段落（`align == Left`）にのみ
/// 適用し、中央・右寄せ段落は左揃えのまま行を組んで揃えオフセットでシフトする。
#[allow(clippy::too_many_arguments)]
fn place_paragraph(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
  items: &[crate::hitem::HItem],
  leading: f32,
  column_width: f32,
  indent: f32,
  right_indent: f32,
  align: types::Align,
) {
  let available = (column_width - indent - right_indent).max(0.0);
  // 両端揃えは左揃え段落にのみ適用する。中央・右寄せ段落は行を自然幅のまま組み、
  // 確定後に揃えオフセットでシフトする（伸縮すると余り幅が消えて揃え自体が無意味になる）
  let effective_alignment = if align == types::Align::Left {
    alignment
  } else {
    TextAlignment::RaggedRight
  };
  let mut lines = breaker.break_lines(items, available, effective_alignment);
  // 行は段左端 (x=0) 基準で組まれるため、インデント + 揃えオフセット（段内 [0, column_width]）を
  // 全行に加算する。揃えオフセットは行ごとに（行幅に応じて）異なる。段オフセットは段をまたぐと
  // 行ごとに変わるため、この事前ループには含めず、配置ループ内で着地段ごとに足す。
  for line in &mut lines {
    let line_width = line.boxes.iter().map(|b| b.x + b.width).fold(0.0f32, f32::max);
    let shift = indent + align.offset(available, line_width);
    if shift != 0.0 {
      for positioned in &mut line.boxes {
        positioned.x += shift;
      }
      for link in &mut line.links {
        link.x0 += shift;
        link.x1 += shift;
      }
    }
  }
  let mut baseline = composer.y;
  let mut prev_depth: Option<f32> = None;
  for mut line in lines {
    let is_first = prev_depth.is_none();
    match prev_depth {
      None => {
        if composer.cursor_at_edge {
          baseline += line.height;
        }
      },
      Some(depth) => {
        baseline += leading.max(depth + line.height);
      },
    }
    if baseline + line.depth > geom.page_limit {
      composer.advance_region(geom);
      baseline = geom.margin_top;
    }
    // 着地段（advance_region 後）の左端オフセットを行ごとに加算する。リンク矩形の収集より前に行う。
    let col_off = composer.column_offset();
    if col_off != 0.0 {
      for positioned in &mut line.boxes {
        positioned.x += col_off;
      }
      for link in &mut line.links {
        link.x0 += col_off;
        link.x1 += col_off;
      }
    }
    // 先頭行の確定位置（改段・改ページ後）で未解決アンカーを解決する。行の上端（段の左端）を指す
    if is_first {
      composer.resolve_pending_anchors(col_off, baseline - line.height);
    }
    prev_depth = Some(line.depth);
    composer.collect_line_links(&line, baseline);
    composer.current.push(PlacedBlock::Line {
      line,
      baseline_y: baseline,
    });
  }
  composer.y = baseline + leading;
  composer.cursor_at_edge = false;
}

/// 合成済みの単一行（[`Block::ComposedLine`]）を 1 行として配置する
///
/// 段落先頭行と同じ規則で配置する: 直前が底辺基準ブロックならアセント分下げ、段下限を
/// 超えるなら改段または改ページし、確定位置で未解決アンカーを解決してリンク矩形を収集する。配置後は
/// カーソルを `baseline + leading` まで進める。行分割（[`crate::break_lines`]）は通さない。
/// 段組みでは着地段の左端オフセットを行のボックス・リンクに加算する（行は段左端基準で組まれているため）。
fn place_single_line(composer: &mut PageComposer, geom: &PageGeometry, mut line: Line, leading: f32) {
  let mut baseline = composer.y;
  if composer.cursor_at_edge {
    baseline += line.height;
  }
  if baseline + line.depth > geom.page_limit {
    composer.advance_region(geom);
    baseline = geom.margin_top;
  }
  // 着地段（advance_region 後）の左端オフセットを行に加算する。リンク矩形の収集より前に行う。
  let col_off = composer.column_offset();
  if col_off != 0.0 {
    for positioned in &mut line.boxes {
      positioned.x += col_off;
    }
    for link in &mut line.links {
      link.x0 += col_off;
      link.x1 += col_off;
    }
  }
  // 行の上端を未解決アンカーの解決位置にする（段落先頭行と同じ）
  composer.resolve_pending_anchors(col_off, baseline - line.height);
  composer.collect_line_links(&line, baseline);
  composer.current.push(PlacedBlock::Line {
    line,
    baseline_y: baseline,
  });
  composer.y = baseline + leading;
  composer.cursor_at_edge = false;
}

/// ディスプレイ数式ブロックを配置する
///
/// 本体 Atom（`body`）は `layout` 段で局所座標まで組み上がっているため、ここでは段幅の中で
/// 揃え（`align`、既定は中央）オフセットを 1 回算出し、各行番号を段端（`numbers_on_right` で
/// 左右）へ寄せて確定座標を与えるだけ。当面は改ページ（段）単位の不可分ブロックとして扱う
/// （収まらない場合に新しい段／ページなら収まるときだけ先に改段・改ページする）。段組みでは
/// fit チェック後の段オフセットを本体・行番号の x に加算する。
fn place_math_block(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  body: crate::hitem::HBox,
  numbers: Vec<crate::block::MathRowNumber>,
  numbers_on_right: bool,
  align: types::Align,
  column_width: f32,
) {
  let total_height = body.height + body.depth;
  if composer.y + total_height > geom.page_limit && geom.margin_top + total_height <= geom.page_limit {
    composer.advance_region(geom);
  }
  // 改段・改ページ後の段オフセットを読む（着地段に合わせる）
  let col_off = composer.column_offset();
  composer.resolve_pending_anchors(col_off, composer.y);

  let x = col_off + align.offset(column_width, body.width);
  // 本体ベースライン = ブロック上端（カーソル）+ ベースラインより上の高さ
  let baseline_y = composer.y + body.height;
  let placed_numbers: Vec<PlacedMathNumber> = numbers
    .into_iter()
    .map(|n| {
      let number_x = col_off
        + if numbers_on_right {
          (column_width - n.content.width).max(0.0)
        } else {
          0.0
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

/// 表を行単位で配置する（改段・改ページ時は先頭にヘッダ行を再描画する）
///
/// 配置規則:
/// - 分割禁止（`breakable = false`）の表は、現段に収まらず新しい段／ページなら
///   収まる場合のみ先に改段・改ページする
/// - 行配置中に段下限を超えたら改段・改ページし、本体行の前にヘッダ行を再描画する
///
/// 段組みでは、段をまたぐ breakable な表は段ごとに別の `PlacedBlock::Table` 断片になり、断片ごとに
/// 着地段のオフセットが要る。揃えオフセット（段内）だけ先に確定し、`flush` 時に
/// [`PageComposer::column_offset`] を足して断片の `x` を与える（`flush` は必ず `advance_region` の前に
/// 呼ばれるので `col` は断片が属する段を指す）。
fn place_table(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  table: &TableBox,
  column_width: f32,
  align: types::Align,
) {
  let col_widths = resolve_column_widths(table, column_width, geom.table_cell_padding);
  // 表全体の自然幅は確定済み列幅の総和。段幅の中で揃えオフセット（段内）を 1 回だけ算出する
  // （全幅の表ではオフセットが 0 になり段左端のまま）。段オフセットは flush 時に断片ごとに足す。
  let table_align_offset = align.offset(column_width, col_widths.iter().sum());
  let head_heights: Vec<f32> = table
    .head
    .iter()
    .map(|row| table_row_height(row, geom.default_font_size, geom.line_height_factor))
    .collect();
  let row_heights: Vec<f32> = table
    .rows
    .iter()
    .map(|row| table_row_height(row, geom.default_font_size, geom.line_height_factor))
    .collect();

  // 分割禁止の表は、現ページに収まらず新しいページなら収まる場合のみ先に改ページする
  let total_height: f32 = head_heights.iter().chain(row_heights.iter()).sum();
  if !table.breakable
    && composer.y + total_height > geom.page_limit
    && geom.margin_top + total_height <= geom.page_limit
  {
    composer.advance_region(geom);
  }

  // 表先頭の確定位置（改段・改ページ後）で未解決アンカー（`\ref{tab:...}` 到達先）を解決する
  composer.resolve_pending_anchors(composer.column_offset(), composer.y);

  let mut placed_rows: Vec<PlacedTableRow> = Vec::new();
  // 現在の placed_rows を PlacedBlock::Table として確定するヘルパ。断片の x は着地段の
  // オフセット + 段内揃えオフセット（flush は advance_region の前に呼ばれるので col は正しい）。
  let flush = |composer: &mut PageComposer, placed_rows: &mut Vec<PlacedTableRow>| {
    if placed_rows.is_empty() {
      return;
    }
    let x = composer.column_offset() + table_align_offset;
    composer.current.push(PlacedBlock::Table {
      x,
      columns: table.columns.clone(),
      col_widths: col_widths.clone(),
      rows: std::mem::take(placed_rows),
    });
  };

  for (row, height) in table.head.iter().zip(&head_heights) {
    if composer.y + height > geom.page_limit {
      flush(composer, &mut placed_rows);
      composer.advance_region(geom);
    }
    placed_rows.push(PlacedTableRow {
      row: row.clone(),
      top_y: composer.y,
      height: *height,
    });
    composer.y += height;
  }
  for (row, height) in table.rows.iter().zip(&row_heights) {
    if composer.y + height > geom.page_limit {
      flush(composer, &mut placed_rows);
      composer.advance_region(geom);
      // 改段・改ページ後の先頭にヘッダ行を再描画する
      for (head_row, head_height) in table.head.iter().zip(&head_heights) {
        placed_rows.push(PlacedTableRow {
          row: head_row.clone(),
          top_y: composer.y,
          height: *head_height,
        });
        composer.y += head_height;
      }
    }
    placed_rows.push(PlacedTableRow {
      row: row.clone(),
      top_y: composer.y,
      height: *height,
    });
    composer.y += height;
  }
  flush(composer, &mut placed_rows);
}

#[cfg(test)]
mod tests {
  use types::{ColumnAlign, ColumnWidth, TableColumn, TextAlignment};

  use super::{PageGeometry, break_pages, column_width};
  use crate::{
    block::Block,
    break_lines::GreedyBreaker,
    glyph_run::GlyphRun,
    hitem::{HBox, HBoxContent, HItem},
    line::Line,
    page::{Page, PlacedBlock},
    table_box::{TableBox, TableCellBox, TableRowBox},
  };

  /// テスト用ジオメトリ（`margin_top=10`, `page_limit=50`、単段）
  fn test_geometry() -> PageGeometry {
    return PageGeometry {
      margin_top: 10.0,
      page_limit: 50.0,
      default_font_size: 10.0,
      line_height_factor: 1.0,
      table_cell_padding: 2.0,
      num_columns: 1,
      column_gap: 0.0,
    };
  }

  /// テスト用 2 段ジオメトリ（`num_columns=2`, `column_gap=10`）。
  /// `text_width=100` と組むと段幅 45・段オフセット(右段) 55 になる。
  fn two_column_geometry() -> PageGeometry {
    return PageGeometry {
      num_columns: 2,
      column_gap: 10.0,
      ..test_geometry()
    };
  }

  /// テスト用ボックス（幅 10、高さ 8、深さ 2）
  fn test_box() -> HItem {
    return HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: 10.0,
        height: 1.0,
      },
      width: 10.0,
      height: 8.0,
      depth: 2.0,
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
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Left,
    };
  }

  #[test]
  fn paragraph_lines_advance_by_leading() {
    // Arrange — 3 行の段落。ベースラインは margin_top から leading ずつ進む
    let geom = test_geometry();

    // Act
    let pages = break_pages(vec![paragraph_of_lines(3)], 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページに 3 行、baseline は 10, 22, 34
    assert_eq!(pages.len(), 1);
    let baselines: Vec<f32> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .collect();
    assert_eq!(baselines, vec![10.0, 22.0, 34.0]);
  }

  #[test]
  fn page_breaks_when_baseline_exceeds_limit() {
    // Arrange — page_limit=50, leading=12: baseline 10, 22, 34, 46 (depth 2 → 48 ≤ 50),
    // 5 行目は 58 + 2 > 50 で改ページ
    let geom = test_geometry();

    // Act
    let pages = break_pages(vec![paragraph_of_lines(5)], 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 ページに分かれ、2 ページ目の先頭 baseline は margin_top
    assert_eq!(pages.len(), 2, "{pages:?}");
    let second_page_first = pages[1].blocks.first().expect("2 ページ目に行があるはず");
    let PlacedBlock::Line { baseline_y, .. } = second_page_first else {
      panic!("Line を期待: {second_page_first:?}");
    };
    assert!((baseline_y - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn vspace_shifts_following_baseline() {
    // 固定アキ（glue）は次のブロックのベースラインを下へずらす
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(5.0),
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    let baselines: Vec<f32> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .collect();
    // 1 つ目: 10。段落後カーソル 10+12=22、VSpace で 27
    assert_eq!(baselines, vec![10.0, 27.0]);
  }

  #[test]
  fn page_break_block_starts_new_page() {
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 2);
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[1].blocks.len(), 1);
  }

  #[test]
  fn paragraph_after_image_clears_ascent() {
    // 画像の直後の段落は、先頭行のアセント分だけベースラインが下がり重ならない
    let geom = test_geometry();
    let blocks = vec![
      Block::Image {
        path: "x.png".to_string(),
        width: Some(20.0),
        height: Some(15.0),
        target_dpi: None,
        align: types::Align::Left,
      },
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // 画像 top=10, bottom=25。段落先頭行 baseline = 25 + height(8) = 33
    let baseline = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .expect("行があるはず");
    assert!((baseline - 33.0).abs() < f32::EPSILON, "baseline={baseline}");
  }

  #[test]
  fn oversized_image_moves_to_next_page() {
    // 現ページに収まらない画像は改ページしてから配置する
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Image {
        path: "x.png".to_string(),
        width: Some(20.0),
        height: Some(30.0),
        target_dpi: None,
        align: types::Align::Left,
      },
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Image { y, .. } = pages[1].blocks.first().expect("画像があるはず") else {
      panic!("Image を期待");
    };
    assert!((y - 10.0).abs() < f32::EPSILON, "画像はページ先頭 (margin_top) に置かれる");
  }

  /// テキスト入り（フォント 10）の 1 セル行を作るヘルパ
  fn table_row(text: &str) -> TableRowBox {
    return TableRowBox {
      cells: vec![TableCellBox {
        items: vec![HItem::Box(HBox {
          content: HBoxContent::Glyphs(GlyphRun {
            font_size: 10.0,
            text: text.to_string(),
            glyphs: Vec::new(),
            font_type: types::FontType::Serif,
            color: None,
          }),
          width: 20.0,
          height: 10.0,
          depth: 0.0,
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
        && let HItem::Box(hbox) = &first.row.cells[0].items[0]
        && let HBoxContent::Glyphs(run) = &hbox.content
      {
        return Some(run.text.clone());
      }
    }
    return None;
  }

  #[test]
  fn empty_blocks_yield_single_empty_page() {
    // 空入力でも 1 ページ（空ページ）を返す
    let geom = test_geometry();

    let pages = break_pages(vec![], 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 1);
    assert!(pages[0].blocks.is_empty());
  }

  #[test]
  fn multiple_page_breaks_create_multiple_pages() {
    // 内容を挟んだ PageBreak は都度ページを分ける
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::force_break(),
      paragraph_of_lines(1),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 3);
  }

  #[test]
  fn leading_page_break_does_not_create_blank_page() {
    // 先頭が改ページ（Part 見出しの page_break_before 相当）でも、白紙の先頭ページを作らない
    let geom = test_geometry();
    let blocks = vec![Block::force_break(), paragraph_of_lines(1)];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 1, "{pages:?}");
    assert_eq!(pages[0].blocks.len(), 1, "本文は先頭ページに置かれる");
  }

  #[test]
  fn consecutive_page_breaks_without_content_collapse() {
    // 内容を挟まない連続改ページはページ境界 1 つに畳まれる
    // （Part の page_break_after の直後に Chapter の page_break_before が続く状況に相当）。
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::force_break(),
      Block::force_break(),
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 2, "中間に白紙ページは生じない: {pages:?}");
    assert_eq!(pages[0].blocks.len(), 1);
    assert_eq!(pages[1].blocks.len(), 1);
  }

  #[test]
  fn breakable_table_splits_across_pages_and_redraws_header() {
    // Arrange — head + 本体 5 行。各行高 10、page_limit=50 で 2 ページに分割される
    let geom = test_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: vec![table_row("HEAD")],
      rows: (0..5).map(|i| table_row(&format!("R{i}"))).collect(),
      breakable: true,
    };

    // Act
    let pages = break_pages(
      vec![Block::Table {
        table,
        align: types::Align::Left,
      }],
      100.0,
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert — 2 ページに分割され、2 ページ目の表先頭行はヘッダの再描画
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(first_table_row_text(&pages[0]).as_deref(), Some("HEAD"), "1 ページ目もヘッダ始まり");
    assert_eq!(first_table_row_text(&pages[1]).as_deref(), Some("HEAD"), "2 ページ目はヘッダ再描画");
  }

  #[test]
  fn pending_anchor_resolves_to_next_paragraph_top() {
    // Arrange — Anchor の直後の段落の先頭行の上端にアンカーが解決される
    use types::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading {
        key: "heading:0".to_string(),
        label: None,
      }),
      paragraph_of_lines(1),
    ];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — baseline=10, line.height=8 → アンカー y = 2、x = 0
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!((pages[0].anchors[0].y - 2.0).abs() < f32::EPSILON);
    assert!((pages[0].anchors[0].x - 0.0).abs() < f32::EPSILON);
    assert!(matches!(pages[0].anchors[0].mark, AnchorMark::Heading { label: None, .. }));
  }

  #[test]
  fn pending_anchor_resolves_on_page_after_break() {
    // Arrange — ページ 1 を埋めた後の Anchor は、改ページした次段落とともにページ 2 に解決される
    use types::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(4),
      Block::Anchor(AnchorMark::Label("tab:x".to_string())),
      paragraph_of_lines(1),
    ];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — アンカーはページ 1 ではなくページ 2（改ページ後）に解決される
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].anchors.is_empty(), "ページ 1 にアンカーは無い: {:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!((pages[1].anchors[0].y - 2.0).abs() < f32::EPSILON);
  }

  #[test]
  fn paragraph_link_becomes_placed_link() {
    // Arrange — リンクマーカーで囲んだ段落から PlacedLink が確定する
    use types::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — baseline=10, height=8, depth=2 → top=2, height=10, x=0, width=20
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(matches!(&link.target, LinkTarget::External(uri) if uri == "https://example.com"));
    assert!((link.x - 0.0).abs() < f32::EPSILON);
    assert!((link.y - 2.0).abs() < f32::EPSILON);
    assert!((link.width - 20.0).abs() < f32::EPSILON);
    assert!((link.height - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn paragraph_indent_shifts_all_lines_and_reduces_width() {
    // Arrange — indent=20, text_width=60 → 利用可能幅 40。box(10) を glue(5) で連結し折り返す
    let geom = test_geometry();
    let mut items = Vec::new();
    for i in 0..6 {
      if i > 0 {
        items.push(HItem::Glue {
          natural: 5.0,
          stretch: 0.0,
          shrink: 0.0,
          breakable: true,
        });
      }
      items.push(test_box());
    }
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 20.0,
      right_indent: 0.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 60.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 利用可能幅 40 で折り返し（複数行）、全行の先頭ボックス x が indent(20) 以上
    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .collect();
    assert!(lines.len() >= 2, "利用可能幅 40 で折り返すはず: {} 行", lines.len());
    for line in &lines {
      let first = line.boxes.first().expect("各行にボックスがあるはず");
      assert!(first.x >= 20.0 - f32::EPSILON, "先頭ボックス x={} は indent(20) 以上", first.x);
      // どのボックスも本文幅 60 を超えない（はみ出さない）
      for positioned in &line.boxes {
        assert!(
          positioned.x + positioned.width <= 60.0 + f32::EPSILON,
          "x+width={} <= 60",
          positioned.x + positioned.width
        );
      }
    }
  }

  #[test]
  fn paragraph_right_indent_reduces_available_width() {
    // Arrange — indent=10, right_indent=10, text_width=60 → 利用可能幅 40。
    // box(10) を glue(5) で 6 連結し折り返す（右インデントぶん早く折り返す）
    let geom = test_geometry();
    let mut items = Vec::new();
    for i in 0..6 {
      if i > 0 {
        items.push(HItem::Glue {
          natural: 5.0,
          stretch: 0.0,
          shrink: 0.0,
          breakable: true,
        });
      }
      items.push(test_box());
    }
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 10.0,
      right_indent: 10.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 60.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 利用可能幅は 60-10-10=40。全行が右端 text_width - right_indent = 50 を超えない
    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .collect();
    assert!(lines.len() >= 2, "利用可能幅 40 で折り返すはず: {} 行", lines.len());
    for line in &lines {
      let first = line.boxes.first().expect("各行にボックスがあるはず");
      assert!(first.x >= 10.0 - f32::EPSILON, "先頭ボックス x={} は indent(10) 以上", first.x);
      for positioned in &line.boxes {
        assert!(
          positioned.x + positioned.width <= 50.0 + f32::EPSILON,
          "x+width={} <= text_width - right_indent = 50",
          positioned.x + positioned.width
        );
      }
    }
  }

  #[test]
  fn paragraph_indent_shifts_links() {
    // Arrange — indent=15 のリンク付き段落。リンク矩形も indent ぶん右へシフトされる
    use types::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 15.0,
      right_indent: 0.0,
      align: types::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — x0=0 → +15、幅 20 は不変
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!((link.x - 15.0).abs() < f32::EPSILON, "link.x={}", link.x);
    assert!((link.width - 20.0).abs() < f32::EPSILON, "link.width={}", link.width);
  }

  #[test]
  fn centered_paragraph_shifts_line_to_horizontal_center() {
    // Arrange — box(10) 単一行の段落を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = (100 - 10) / 2 = 45。box.x は 45
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 45.0).abs() < f32::EPSILON, "box.x={}", line.boxes[0].x);
  }

  #[test]
  fn right_aligned_paragraph_shifts_line_to_right_edge() {
    // Arrange — box(10) 単一行を align=Right、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Right,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = 100 - 10 = 90。box.x は 90（右端に揃う）
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 90.0).abs() < f32::EPSILON, "box.x={}", line.boxes[0].x);
  }

  /// テスト用の伸縮能力付き breakable glue（幅 5・伸長 2.5・収縮 5/3 = 単語間スペース相当）
  fn stretch_glue() -> HItem {
    return HItem::Glue {
      natural: 5.0,
      stretch: 2.5,
      shrink: 5.0 / 3.0,
      breakable: true,
    };
  }

  /// 伸縮能力付き glue で折り返す 2 行の段落（1 行目自然幅 25）を作る
  fn stretchable_paragraph(align: types::Align) -> Block {
    return Block::Paragraph {
      items: vec![
        test_box(),
        stretch_glue(),
        test_box(),
        stretch_glue(),
        test_box(),
      ],
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align,
    };
  }

  #[test]
  fn justify_stretches_left_aligned_paragraph() {
    // Arrange — text_width=27 で折り返す左揃え段落を justify 設定で配置する
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(types::Align::Left)];

    // Act
    let pages = break_pages(blocks, 27.0, &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert — 1 行目の余り 2 が glue に配分され、2 つ目の box の右端が版面右端（27）に一致する
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[1].x + line.boxes[1].width - 27.0).abs() < 1e-4, "{:?}", line.boxes);
  }

  #[test]
  fn justify_does_not_stretch_centered_paragraph() {
    // Arrange — 同じ段落を align=Center にすると justify 設定でも伸縮しない
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(types::Align::Center)];

    // Act
    let pages = break_pages(blocks, 27.0, &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert — 1 行目は自然幅 25 のまま中央へシフト（オフセット = (27 − 25) / 2 = 1）。
    // box 間隔は自然 glue 幅 5 を保つ
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 1.0).abs() < 1e-4, "{:?}", line.boxes);
    assert!((line.boxes[1].x - line.boxes[0].x - 15.0).abs() < 1e-4, "glue は自然幅のまま: {:?}", line.boxes);
  }

  #[test]
  fn justify_does_not_stretch_right_aligned_paragraph() {
    // Arrange — align=Right も伸縮せず、自然幅のまま右端へ寄る
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(types::Align::Right)];

    // Act
    let pages = break_pages(blocks, 27.0, &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert — 1 行目のオフセット = 27 − 25 = 2
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 2.0).abs() < 1e-4, "{:?}", line.boxes);
    assert!((line.boxes[1].x - line.boxes[0].x - 15.0).abs() < 1e-4, "glue は自然幅のまま: {:?}", line.boxes);
  }

  #[test]
  fn centered_overflowing_line_is_not_shifted_negative() {
    // Arrange — 幅 50 の単一 box を align=Center、text_width=30 で配置（行幅 > 利用可能幅）
    let geom = test_geometry();
    let wide = HItem::Box(HBox {
      content: HBoxContent::Rule {
        width: 50.0,
        height: 1.0,
      },
      width: 50.0,
      height: 8.0,
      depth: 2.0,
    });
    let blocks = vec![Block::Paragraph {
      items: vec![wide],
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 30.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — シフト量は 0 にクランプされ、box.x は左端 0 のまま（左へはみ出さない）
    let line = pages[0]
      .blocks
      .iter()
      .find_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .expect("行があるはず");
    assert!((line.boxes[0].x - 0.0).abs() < f32::EPSILON, "box.x={}", line.boxes[0].x);
  }

  #[test]
  fn centered_wrapped_lines_are_each_independently_centered() {
    // Arrange — box(10) glue(5) box(10) glue(5) box(10) を text_width=35 で折り返す。
    // 1 行目は box glue box（幅 25）、2 行目は box（幅 10）になり、行幅が異なる。
    let geom = test_geometry();
    let items = vec![
      test_box(),
      HItem::Glue {
        natural: 5.0,
        stretch: 0.0,
        shrink: 0.0,
        breakable: true,
      },
      test_box(),
      HItem::Glue {
        natural: 5.0,
        stretch: 0.0,
        shrink: 0.0,
        breakable: true,
      },
      test_box(),
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 35.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 行に折り返し、各行が自身の行幅で独立に中央寄せされる。
    // 1 行目: 幅 25 → オフセット (35-25)/2 = 5、2 行目: 幅 10 → オフセット (35-10)/2 = 12.5
    let lines: Vec<&Line> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { line, .. } => Some(line),
        _ => None,
      })
      .collect();
    assert_eq!(lines.len(), 2, "text_width=35 で 2 行に折り返すはず: {} 行", lines.len());
    assert!((lines[0].boxes[0].x - 5.0).abs() < f32::EPSILON, "1 行目先頭 x={}", lines[0].boxes[0].x);
    assert!((lines[1].boxes[0].x - 12.5).abs() < f32::EPSILON, "2 行目先頭 x={}", lines[1].boxes[0].x);
  }

  #[test]
  fn centered_paragraph_shifts_links() {
    // Arrange — box 2 つ（幅 20）を囲むリンクを align=Center、text_width=100 で配置する。
    // リンク矩形も中央オフセット分シフトされ、確定 PlacedLink に追従する。
    use types::LinkTarget;
    let geom = test_geometry();
    let items = vec![
      HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
      test_box(),
      test_box(),
      HItem::LinkEnd,
    ];
    let blocks = vec![Block::Paragraph {
      items,
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 行幅 20 → 中央オフセット (100-20)/2 = 40。link.x=40、幅 20 は不変
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!((link.x - 40.0).abs() < f32::EPSILON, "link.x={}", link.x);
    assert!((link.width - 20.0).abs() < f32::EPSILON, "link.width={}", link.width);
  }

  /// ページ内の最初の `PlacedBlock::Image` を取り出すヘルパ
  fn first_image(page: &Page) -> &PlacedBlock {
    return page.blocks.iter().find(|b| matches!(b, PlacedBlock::Image { .. })).expect("画像があるはず");
  }

  #[test]
  fn centered_image_shifts_x_to_horizontal_center() {
    // Arrange — 幅 20 の画像を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: "x.png".to_string(),
      width: Some(20.0),
      height: Some(15.0),
      target_dpi: None,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = (100 - 20) / 2 = 40
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!((x - 40.0).abs() < f32::EPSILON, "image.x={x}");
  }

  #[test]
  fn right_aligned_image_shifts_x_to_right_edge() {
    // Arrange — 幅 20 の画像を align=Right、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: "x.png".to_string(),
      width: Some(20.0),
      height: Some(15.0),
      target_dpi: None,
      align: types::Align::Right,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = 100 - 20 = 80（右端に揃う）
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!((x - 80.0).abs() < f32::EPSILON, "image.x={x}");
  }

  #[test]
  fn centered_rule_shifts_x_to_horizontal_center() {
    // Arrange — 幅 30 の罫線を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Rule {
      width: 30.0,
      height: 2.0,
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = (100 - 30) / 2 = 35
    let PlacedBlock::Rule { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Rule { .. })).expect("罫線があるはず")
    else {
      unreachable!()
    };
    assert!((x - 35.0).abs() < f32::EPSILON, "rule.x={x}");
  }

  #[test]
  fn centered_table_shifts_all_rows_x() {
    // Arrange — Auto 1 列（セル幅 20 + 左右 padding 2×2 = 24）の表を align=Center、text_width=100 で配置
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
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 表全体幅 24 → オフセット (100 - 24) / 2 = 38
    let PlacedBlock::Table { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!((x - 38.0).abs() < f32::EPSILON, "table.x={x}");
  }

  #[test]
  fn full_width_table_is_not_shifted_by_center() {
    // Arrange — Ratio(1.0) 列で本文幅いっぱいの表は中央寄せしても動かない（オフセット 0）
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
      align: types::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 表全体幅 = 本文幅 100 → オフセット 0
    let PlacedBlock::Table { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!((x - 0.0).abs() < f32::EPSILON, "table.x={x}");
  }

  #[test]
  fn no_line_baseline_exceeds_page_limit() {
    // 不変条件: どのページの行も baseline + depth がページ下限を超えない
    let geom = test_geometry();

    let pages = break_pages(vec![paragraph_of_lines(12)], 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert!(pages.len() >= 2, "複数ページに分かれる: {}", pages.len());
    for page in &pages {
      for block in &page.blocks {
        if let PlacedBlock::Line { line, baseline_y } = block {
          assert!(
            baseline_y + line.depth <= geom.page_limit + f32::EPSILON,
            "baseline={baseline_y} depth={} が page_limit={} を超えた",
            line.depth,
            geom.page_limit
          );
        }
      }
    }
  }

  /// 合成済み単一行（[`Block::ComposedLine`]）のテスト用ヘルパ。幅・高さ・深さと任意のリンクを持つ
  fn composed_line(width: f32, height: f32, depth: f32, link: Option<types::LinkTarget>) -> Block {
    let links = link.map_or_else(Vec::new, |target| {
      vec![crate::line::LineLink {
        target,
        x0: 0.0,
        x1: width,
      }]
    });
    return Block::ComposedLine {
      line: Line {
        boxes: vec![crate::line::PositionedBox {
          content: HBoxContent::Rule { width, height },
          x: 0.0,
          dy: 0.0,
          width,
        }],
        height,
        depth,
        is_last: true,
        links,
      },
      leading: 12.0,
    };
  }

  #[test]
  fn composed_line_places_at_baseline_with_leading() {
    // Arrange — margin_top=10, leading=12 の ComposedLine を 2 つ
    let geom = test_geometry();
    let blocks = vec![
      composed_line(20.0, 8.0, 2.0, None),
      composed_line(20.0, 8.0, 2.0, None),
    ];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — baseline は 10, 22（leading=12 ずつ）。行分割は通さない
    let baselines: Vec<f32> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => Some(*baseline_y),
        _ => None,
      })
      .collect();
    assert_eq!(baselines, vec![10.0, 22.0]);
  }

  #[test]
  fn composed_line_resolves_anchor_and_collects_link() {
    // Arrange — 見出しアンカー直後の ComposedLine（内部リンク付き）
    use types::{AnchorMark, LinkTarget};
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading {
        key: "heading:0".to_string(),
        label: None,
      }),
      composed_line(20.0, 8.0, 2.0, Some(LinkTarget::Internal("heading:5".to_string()))),
    ];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — アンカーは行上端（baseline 10 − height 8 = 2）に解決
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!((pages[0].anchors[0].y - 2.0).abs() < f32::EPSILON);
    // リンクは PlacedLink 化（top=2, height=height+depth=10, 行き先は内部キー）
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    assert!(matches!(&pages[0].links[0].target, LinkTarget::Internal(k) if k == "heading:5"));
    assert!((pages[0].links[0].y - 2.0).abs() < f32::EPSILON);
    assert!((pages[0].links[0].height - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn composed_lines_break_across_pages() {
    // Arrange — page_limit=50, margin_top=10, leading=12, depth=2:
    // baseline 10,22,34,46（46+2=48≤50）まで 1 ページ、5 本目で改ページ
    let geom = test_geometry();
    let blocks: Vec<Block> = (0..5).map(|_| composed_line(20.0, 8.0, 2.0, None)).collect();

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 ページに分かれ、2 ページ目の先頭 baseline は margin_top
    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Line { baseline_y, .. } = pages[1].blocks.first().expect("2 ページ目に行があるはず")
    else {
      panic!("Line を期待");
    };
    assert!((baseline_y - 10.0).abs() < f32::EPSILON);
  }

  #[test]
  fn column_width_helper_divides_text_width() {
    // Arrange / Act / Assert — (100 - 1*10)/2 = 45、単段は本文幅そのまま
    assert!((column_width(100.0, 2, 10.0) - 45.0).abs() < f32::EPSILON);
    assert!((column_width(100.0, 1, 18.0) - 100.0).abs() < f32::EPSILON);
  }

  #[test]
  fn two_column_flow_fills_left_then_right_then_next_page() {
    // Arrange — 2 段（段幅 45・段オフセット 55）。9 行を流すと、左段 4 行 → 右段 4 行 → 次ページ 1 行
    // になる（各段はカーソルが margin_top に戻り、右段は x に 55 が乗る）。
    let geom = two_column_geometry();

    // Act
    let pages = break_pages(vec![paragraph_of_lines(9)], 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 各行の (baseline_y, 先頭ボックス x) を採取して段送りを検証する
    let page_lines = |page: &Page| -> Vec<(f32, f32)> {
      return page
        .blocks
        .iter()
        .filter_map(|b| match b {
          PlacedBlock::Line { line, baseline_y } => Some((*baseline_y, line.boxes[0].x)),
          _ => None,
        })
        .collect();
    };
    assert_eq!(pages.len(), 2, "{pages:?}");
    let p0 = page_lines(&pages[0]);
    assert_eq!(p0.len(), 8, "1 ページ目は左段 4 行 + 右段 4 行: {p0:?}");
    // 左段: x≈0、baseline は margin_top から leading(12) ずつ
    assert_eq!(p0[0..4], [(10.0, 0.0), (22.0, 0.0), (34.0, 0.0), (46.0, 0.0)], "左段は x=0 で 10,22,34,46");
    // 右段: x≈55、baseline は margin_top にリセットされて再び 10,22,34,46
    assert_eq!(
      p0[4..8],
      [(10.0, 55.0), (22.0, 55.0), (34.0, 55.0), (46.0, 55.0)],
      "右段は x=55 で baseline がリセット"
    );
    let p1 = page_lines(&pages[1]);
    assert_eq!(p1, [(10.0, 0.0)], "9 行目は次ページの左段先頭");
  }

  #[test]
  fn two_column_paragraph_link_rect_uses_column_offset() {
    // Arrange（Hole A の番人）— 左段を埋めた後に右段へ入るリンク付き段落を置く。リンク矩形は
    // ボックスと一緒に段オフセット分シフトされていなければならない。
    use types::LinkTarget;
    let geom = two_column_geometry();
    let link_para = Block::Paragraph {
      items: vec![
        HItem::LinkStart(LinkTarget::External("https://example.com".to_string())),
        test_box(),
        test_box(),
        HItem::LinkEnd,
      ],
      leading: 12.0,
      indent: 0.0,
      right_indent: 0.0,
      align: types::Align::Left,
    };
    // paragraph_of_lines(5) で左段を埋め、カーソルを右段へ送ってからリンク段落を置く
    let blocks = vec![paragraph_of_lines(5), link_para];

    // Act
    let pages = break_pages(blocks, 100.0, &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — リンクは右段（x≈55）に確定し、2 ボックス分の幅 20 を持つ
    assert_eq!(pages.len(), 1, "全行が 1 ページの 2 段に収まる: {pages:?}");
    let link = pages[0]
      .links
      .iter()
      .find(|l| matches!(&l.target, LinkTarget::External(uri) if uri == "https://example.com"))
      .expect("External リンクがあるはず");
    assert!((link.x - 55.0).abs() < f32::EPSILON, "リンク x={} は段オフセット 55", link.x);
    assert!((link.width - 20.0).abs() < f32::EPSILON, "リンク幅={}", link.width);
  }

  #[test]
  fn breakable_table_spans_columns_uses_column_offset() {
    // Arrange（Hole B の番人）— 1 段に収まらない breakable 表が左段 → 右段にまたがるとき、
    // 2 つ目の断片の x に段オフセットが乗る。各行高 10・段高 40 で 4 行ずつ。
    let geom = two_column_geometry();
    let table = TableBox {
      columns: vec![TableColumn {
        align: ColumnAlign::Left,
        width: ColumnWidth::Auto,
      }],
      head: Vec::new(),
      rows: (0..6).map(|i| table_row(&format!("R{i}"))).collect(),
      breakable: true,
    };

    // Act
    let pages = break_pages(
      vec![Block::Table {
        table,
        align: types::Align::Left,
      }],
      100.0,
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert — 1 ページ内に 2 断片。1 つ目は左段（x≈0）、2 つ目は右段（x≈55）
    assert_eq!(pages.len(), 1, "{pages:?}");
    let xs: Vec<f32> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Table { x, .. } => Some(*x),
        _ => None,
      })
      .collect();
    assert_eq!(xs.len(), 2, "左段断片 + 右段断片の 2 つ: {xs:?}");
    assert!((xs[0] - 0.0).abs() < f32::EPSILON, "1 つ目（左段）x={}", xs[0]);
    assert!((xs[1] - 55.0).abs() < f32::EPSILON, "2 つ目（右段）x={}", xs[1]);
  }
}
