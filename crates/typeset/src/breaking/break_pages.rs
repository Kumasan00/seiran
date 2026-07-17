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

use model::{
  AnchorMark, Block, Length, Line, PENALTY_FORBID_BREAK, PENALTY_FORCE_BREAK, Page, PlacedAnchor, PlacedBlock,
  PlacedFootnote, PlacedLink, PlacedMathNumber, PlacedTableRow, TableBox, TextAlignment, column_width,
  resolve_column_widths, table_row_height,
};
use tracing::{debug, warn};

use super::break_lines::LineBreaker;

/// ページの物理ジオメトリと既定の行送りパラメータ
///
/// `config` に依存しないよう、呼び出し側（`build_pdf`）が
/// 設定から組み立てて渡す。
#[derive(Debug, Clone, Copy)]
pub struct PageGeometry {
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
  ///
  /// `true` のとき、満杯になった段（リージョン）の不足高さを段内の伸縮アキ（glue の stretch）へ
  /// 比例配分し、最終ベースラインを版面下端（`page_limit`）へ寄せる。最終ページ・強制改ページ直前の
  /// リージョン・伸縮アキが無いリージョンは揃えない（自然高のまま）。前付け（`front_geometry`）は常に
  /// `false`。既定 `false` で従来どおりの ragged bottom。
  pub flush_bottom: bool,
  /// 脚注: 本文と区切り罫線の間隔（`style.footnote.top_margin`）
  pub footnote_top_margin: Length,
  /// 脚注: 区切り罫線の長さ（`style.footnote.rule_length`）
  pub footnote_rule_length: Length,
  /// 脚注: 区切り罫線の太さ（0 のとき描画しない、`style.footnote.rule_thickness`）
  pub footnote_rule_thickness: Length,
  /// 脚注: 区切り罫線の色（RGB）。`None` は黒。呼び出し側が `config::Color::rgb()` で
  /// 変換済みの値を渡す（`RunningSlots.rule_color` と同じ規約）
  pub footnote_rule_color: Option<[u8; 3]>,
  /// 脚注: 区切り罫線〜最初の脚注、および脚注どうしの間隔（`style.footnote.rule_gap`）
  pub footnote_rule_gap: Length,
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
  /// 未解決のアンカー。次に配置される実ブロックの確定座標で解決する
  pending_anchors: Vec<AnchorMark>,
  /// カーソル位置（ページ上端からの距離、pt）。基本は「次のベースライン位置」
  y: Length,
  /// 直前のブロックが底辺基準（画像・表・罫線）で終わったか
  ///
  /// `true` のとき、次の段落の先頭行はベースラインをアセント分だけ下げる。
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
  ///
  /// 内容ブロックを配置するたびに [`PageComposer::take_pending_penalty`] で読み取ってリセットする。
  /// 既定 0（中立）。強制改ページ（−∞）は eager に処理するためここには積まない。本 issue（#166）では
  /// 発行者がいないため常に 0 で、[`PageComposer::consider_break`] は幾何判定のみに帰着する。
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
  ///
  /// 脚注 1 個がリージョンの脚注エリアに収まらないとき、[`place_paragraph`] が入るだけの行を置いて
  /// 残りをここへ積む。繰越は次リージョンの脚注エリアの**先頭**に置く（そのページの自前の脚注より前）。
  ///
  /// # 不変条件
  ///
  /// **[`place_lines`] は、繰越が残っている状態でリージョン境界をまたいで計画しない。**
  ///
  /// [`place_lines`] は段落全体（＝複数リージョン）の計画を 1 回で立てる純粋関数だが、次リージョンの
  /// 脚注エリアが繰越でどれだけ埋まるかは [`PageComposer::seed_carry`] を通すまで分からない。
  /// 予測させると計画と実配置がずれて本文が繰越脚注に重なるので、代わりに**境界で計画を打ち切る**:
  ///
  /// - 繰越がある状態で改リージョンに達したら [`place_lines`] はそこで計画を返す。
  ///   [`place_paragraph`] が [`PageComposer::advance_region`]（＝ seed）してから計画し直す。
  /// - 脚注を分割した行でも計画を打ち切る（そのリージョンは脚注エリアで満杯になっている）。
  /// - 文書末尾に残った分は [`PageComposer::finish`] が出し切る。
  ///
  /// 繰越が無い（＝脚注が分割されない）文書ではどちらの打ち切りも起きないので、[`place_lines`] は
  /// 従来どおり段落全体を 1 回で計画する。
  carry: Vec<PendingFootnote>,
}

/// 現在リージョンに集約された脚注 1 個（行分割済み、未確定座標）
///
/// [`place_paragraph`] が [`LinePlacement::own_splits`] に従って `lines` を切り詰めてから積むため、
/// [`PageComposer::end_region`] は「ここにある行をそのまま置く」だけでよい（＝分割の算術は
/// [`pack_footnotes`] だけが持つ）。切り落とされた残りは `continued: true` の [`PendingFootnote`] として
/// [`PageComposer::carry`] へ回る。
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
///
/// アキを消費した時点の各配置ベクタの長さを覚えておくことで、座標比較に頼らずに「このアキより後に
/// 置かれた要素」を index で厳密に判定する（行の箱がアキ帯へ食い込む等の座標曖昧性を回避する）。
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
    };
  }

  /// 現在リージョンの実効下限（pt）。脚注が占有する高さぶん `geom.page_limit` を縮める。
  ///
  /// 脚注は行の配置（[`place_paragraph`]）と同じタイミングで積まれるため、この値は本文の
  /// 改段・改ページ判定（[`PageComposer::consider_break`] 等）と行のベースライン判定
  /// （[`plan_paragraph_lines`]）の両方で `geom.page_limit` の代わりに読む。
  fn region_limit(&self, geom: &PageGeometry) -> Length { return geom.page_limit - self.region_footnote_height; }

  /// 現在の段の左端 x オフセット（本文左端基準、pt）。段 `k` は `k * (段幅 + 段間)` だけ右へ寄る
  fn column_offset(&self) -> Length {
    // 段インデックスは実用上 0〜1。f32 で精度を失う桁数にはならない
    #[allow(clippy::cast_precision_loss)]
    let col = self.col as f32;
    return (self.column_width + self.column_gap) * col;
  }

  /// ページ下限を超えたときの遷移。次の段があれば改段、なければ改ページし、繰越脚注を新リージョンへ詰める。
  ///
  /// 改段はページを送らず、段インデックスを進めてカーソルを上端へ戻すだけ
  /// （次段の先頭は段落先頭行と同じ扱いにするため `cursor_at_edge` も倒す）。最終段で
  /// 超えたときだけ [`PageComposer::start_new_page`] へフォールスルーして実際に改ページする。
  fn advance_region(&mut self, geom: &PageGeometry) {
    // 満杯になったリージョン（段）を先に確定する。下端揃え（#169）が有効なら不足高さを段内の
    // 伸縮アキへ配分してから次段 / 次ページへ移る。強制改ページ・最終ページはこの経路を通らない
    // （それぞれ `force_new_page`・`finish` が確定）ため揃えられない。
    self.end_region(geom, true);
    self.next_region(geom);
    self.seed_carry(geom);
  }

  /// 次のリージョン（段 / ページ）へ移るだけの遷移（リージョン確定・繰越の詰め込みは行わない）
  ///
  /// リージョンの確定（[`PageComposer::end_region`]）と繰越の詰め込み（[`PageComposer::seed_carry`]）は
  /// 呼び出し側の責務。段が残っていれば改段、最終段なら改ページする。
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
  ///
  /// [`PageComposer::start_new_page`] を直接呼ばずこちらを通すことで、強制改ページ経路でも
  /// リージョン入口の繰越詰め込みを飛ばさない（`carry` の不変条件）。
  fn force_new_page(&mut self, geom: &PageGeometry) {
    self.start_new_page(geom);
    self.seed_carry(geom);
  }

  /// 繰越脚注（`carry`）を新しいリージョンの脚注エリアの先頭へ 1 リージョンぶん詰める。
  ///
  /// 予算はリージョン全高（`page_limit - margin_top`）。入り切らなかった分は `carry` に残し、
  /// **このリージョンの本文はその下（`region_limit` まで）に流す** — 繰越が残っているからといって
  /// 本文を追い出さない。追い出すと、繰越が 1 リージョンに収まらないたびに本文 0 行のページが
  /// できてしまう（脚注エリアが全高に満たないケースでも本文の入る余地を捨てることになる）。
  ///
  /// 残った繰越は次のリージョン入口（[`PageComposer::advance_region`]）で再び詰める。
  /// [`pack_footnotes`] が先頭の脚注に最低 1 行を保証するので、リージョンを跨ぐたびに繰越は
  /// 必ず 1 行以上減り、有限回で尽きる。
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
    // 繰越は「そのページの自前の脚注より前」に置くので、常にエリア先頭（`base_reserved` = 0）から詰める。
    // `require_first_line = false` の詰め込みは最低 1 行を強制するので必ず成功する
    let packing = pack_footnotes(&demands, Length::ZERO, geom.page_limit - geom.margin_top, charges, false)
      .expect("繰越の詰め込みは先頭に最低 1 行を強制するので None にならない");
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
  ///
  /// `next_height` は次に置く内容ブロックの高さ、`penalty` は直前の境界の分割コスト
  /// （[`PageComposer::take_pending_penalty`] の戻り値）。判定は次の一本:
  ///
  /// - [`PENALTY_FORBID_BREAK`]（+∞）: 分割禁止。オーバーフローしても切らない（防御的不変条件）。
  /// - それ以外: 高さがページ下限を超えるなら [`PageComposer::advance_region`] で改段 / 改ページする（幾何判定）。
  ///
  /// 強制改ページ（[`PENALTY_FORCE_BREAK`]）はここを通さず [`break_pages`] の match アームで eager に処理する
  /// （直前のアキが旧ページ末尾に乗るのを避けるため）。keep-with-next（#168）の分割禁止は `pending_penalty`
  /// ではなく keep グループゲート（[`keep_group_orphaned`]）で処理するため、通常この経路には FORBID は流れない。
  /// 現状の発行者は有限 penalty がおらず `penalty == 0` に帰着することが多い。有限 penalty を使う
  /// widow/orphan（#167）はここに badness 判定を足す。
  fn consider_break(&mut self, next_height: Length, penalty: i32, geom: &PageGeometry) {
    if penalty == PENALTY_FORBID_BREAK {
      return;
    }
    if self.y + next_height > self.region_limit(geom) {
      self.advance_region(geom);
    }
  }

  /// 現在のリージョン（段 / ページ）の先頭にいて、これ以上前へは送れない（回避不能）かを返す。
  ///
  /// keep-with-next ゲート（[`keep_group_orphaned`]）が真でも、既にリージョン先頭なら改段 / 改ページ
  /// しても空間は増えないため送らない。`start_new_page` の空ページ抑止と併せ、無限ループを防ぐ。
  fn at_region_top(&self, geom: &PageGeometry) -> bool { return self.y <= geom.margin_top && !self.cursor_at_edge; }

  /// 現在ページを確定し、新しいページを開始する
  ///
  /// 未解決アンカー（`pending_anchors`）は引き継ぐ。次の実ブロックがこの新ページに
  /// 配置されたときに解決されるため、移動はしない。
  ///
  /// 現在ページにまだ本文ブロックが 1 つも置かれていない場合は何もしない（ページを
  /// 送らない）。これにより、先頭が見出しのときの白紙の先頭ページや、内容を挟まない
  /// 連続改ページ（Part の `page_break_after` の直後に Chapter の `page_break_before`
  /// が続く等）による中間の白紙ページが生じない。走り文はページ数確定後の別パスで載るため
  /// 判定に含めず、ゼロサイズのアンカー・リンクも本文ブロックではないため含めない。
  ///
  /// ただし確定済みの脚注（`page_footnotes`）があるページは、本文ブロックが無くても送る —
  /// 長い脚注の繰越（#227）だけでページ全体が埋まると本文 0 行のページが生じるためで、
  /// ここで送らないとその脚注が次ページへ silently に合流して重なる。
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
  ///
  /// `baseline_y` は行のベースライン。矩形は行の `height` / `depth` で縦範囲を取る。
  /// 退化した（幅 0 以下の）矩形はスキップする。
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

  /// 全ブロックの配置後に最終ページを確定して返す
  fn finish(mut self, geom: &PageGeometry) -> Vec<Page> {
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
    self.pages.push(Page {
      blocks: self.current,
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: self.page_footnotes,
      anchors: self.current_anchors,
      links: self.current_links,
    });
    return self.pages;
  }

  /// 現在リージョン（段）を確定し、下端揃え（#169）が有効なら不足高さを段内の伸縮アキへ配分する。
  ///
  /// `flush` は「このリージョンを揃える対象にするか」。満杯で改段・改ページする経路（[`advance_region`]）
  /// だけが `true` を渡す。強制改ページ直前・最終ページはこのメソッドを通さない（=揃えない）。配分は
  /// 配置順ベース: 各ブロック（およびリンク矩形・アンカー）を「それより前に記録された伸縮アキの
  /// stretch 合計 × ratio」だけ下方へずらす。末尾ブロックが不足高さ全量だけ下がり、リージョン下端が
  /// 版面下限（`page_limit`）に達する。リージョン末尾のアキ（末尾ブロックより後に記録された分＝改段で
  /// 捨てられるアキ）は配分の分母から除く（除かないと下端が不足高さ分だけ届かなくなる）。
  ///
  /// いずれの場合も次リージョンのためにリージョン追跡（`region_*`）を現在の末尾へリセットする。
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
///
/// 数式ブロックは本体ベースラインと各行番号を、表は全行の上端を一律にずらす。水平座標は変えない。
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
    PlacedBlock::Table { rows, .. } => {
      for row in rows {
        row.top_y += dy;
      }
    },
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
///   左揃え段落にのみ適用し、中央・右寄せ段落（[`model::Align::Center`] / [`model::Align::Right`]）は
///   伸縮しない
///
/// 段組み時は本文を左段 → 右段 → 次ページの順に流す。各ブロックは 1 段あたりの幅 `col_width` で
/// 行分割・揃え・列幅解決を行い、確定 x には現在段の左端オフセット（[`PageComposer::column_offset`]）を
/// 加える。段下限を超えると [`PageComposer::advance_region`] が改段または改ページする。単段
/// （`num_columns == 1`）では `col_width == text_width`・オフセット 0 となり、従来と同一の出力になる。
#[must_use]
pub fn break_pages(
  blocks: Vec<Block>,
  text_width: Length,
  geom: &PageGeometry,
  breaker: &dyn LineBreaker,
  alignment: TextAlignment,
) -> Vec<Page> {
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
    // 配置対象を取り出す（残したプレースホルダは以降参照しない）。
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
        let width = width.unwrap_or(Length::ZERO);
        let height = height.unwrap_or(Length::ZERO);
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
    i += 1;
  }

  let pages = composer.finish(geom);
  debug!(block_count, page_count = pages.len(), "ページ分割が完了しました");
  return pages;
}

/// widow/orphan 制御でまとめて送る最小行数。
///
/// 段落の先頭 / 末尾のリージョン（次段・次ページ）に、この行数を下回る孤立行を残さない。
/// 当面は固定値 2（先頭 1 行だけ / 末尾 1 行だけの孤立を防ぐ）。`style.toml` への公開は
/// スコープ外（#167）— 必要になったら別 issue で読み込み値に差し替える。
const MIN_LINES_AT_BREAK: usize = 2;

/// 段落 1 行の配置計画（純粋な幾何判定の結果）
///
/// [`plan_paragraph_lines`] が段落の各行について確定する。副作用（アンカー解決・リンク収集・
/// 段オフセット）は持たず、ベースライン位置と「この行から新しいリージョンが始まるか」だけを表す。
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
  ///
  /// 行数に満たない要素は分割されており、残りは繰越（[`PageComposer::carry`]）へ回る。
  /// [`place_paragraph`] が [`split_pending`] でそのとおりに切る。
  own_splits: Vec<usize>,
}

/// `demands` を全行そのまま積んだときの脚注エリアの高さ（pt、固定費込み）を返す（純粋関数）
///
/// `reserved` はこのリージョンで既に確保済みのエリア高さ。戻り値は「追加ぶん」ではなく確保後の
/// 総高さで、[`pack_footnotes`] が全行を置けたときの `reserved + height` と一致する。
fn footnote_area_full(demands: &[FootnoteDemand], reserved: Length, charges: FootnoteCharges) -> Length {
  let mut area = reserved;
  for demand in demands {
    area += charges.entry_overhead(area) + demand.full_height();
  }
  return area;
}

/// 行 1 個の脚注を、リージョンの脚注エリアへどう収めるかの判定結果
enum LineFootnoteFit {
  /// 全部そのまま入る。値はエリアの新しい高さ（pt）
  Full(Length),
  /// 入り切らないので分割する。エリアの新しい高さ（pt）と脚注ごとの配置行数
  Split(Length, Vec<usize>),
  /// この行はこのリージョンに置けない（脚注の先頭 1 行すら入らない、または行自体が入らない）
  Rejected,
}

/// 行 `i` をベースライン基準で置いたとき、その行の脚注をリージョンの脚注エリアへどう収めるかを
/// 決める（純粋関数）
///
/// `reserved` はこのリージョンで既に確保済みの脚注エリア高さ、`body_bottom` は行の下端
/// （`baseline + line.depth`）。判定は次の順:
///
/// 1. 脚注を全部置いても本文下端が実効下限（`page_limit - 予約`）に収まるなら [`LineFootnoteFit::Full`]。
/// 2. 収まらないなら、本文下端からページ下端までを予算にして [`pack_footnotes`] で分割を試みる
///    （＝入るだけ入れてエリアをページ下端まで満たす）。分割できたら [`LineFootnoteFit::Split`]。
///    このとき `reserved` は本文下端まで食い込むので、**次の行は既存の幾何判定だけで自動的に
///    改リージョンになる**（改リージョン規則を足さないのが要点）。
/// 3. 脚注の先頭 1 行すら置けない（または脚注が無くて行自体が入らない）なら
///    [`LineFootnoteFit::Rejected`] — 呼び出し側が行ごと次リージョンへ送る（従来の振る舞い）。
fn fit_line_footnotes(
  demands: &[FootnoteDemand],
  reserved: Length,
  body_bottom: Length,
  page_limit: Length,
  charges: FootnoteCharges,
) -> LineFootnoteFit {
  let full = footnote_area_full(demands, reserved, charges);
  if body_bottom <= page_limit - full {
    return LineFootnoteFit::Full(full);
  }
  if demands.is_empty() {
    // 脚注が無いのに収まらない = 行自体がリージョンに入らない（分割する余地は無い）
    return LineFootnoteFit::Rejected;
  }
  let budget = page_limit - body_bottom - reserved;
  return match pack_footnotes(demands, reserved, budget, charges, true) {
    Some(packing) => LineFootnoteFit::Split(reserved + packing.height, packing.splits),
    None => LineFootnoteFit::Rejected,
  };
}

/// 強制改リージョン点（`forced`）を尊重しつつ、貪欲にベースラインを送って各行を配置する（純粋関数）
///
/// `forced[i]` が `true` の行は幾何的に収まっても新リージョンの先頭に置く。それ以外は
/// [`fit_line_footnotes`] が「この行と、その行の脚注が現在のリージョンに収まるか」を判定する
/// （＝脚注予約込みの `baseline + depth > page_limit - reserved` という従来どおりの貪欲判定）。
/// ベースライン漸化式は [`place_paragraph`] の配置ループと同一で、`demands` が全て空
/// （脚注なし）のときは移行前と同じ結果になる。
///
/// `demands[i]` は行 `i` に付いた脚注の需要（分割可能な行ごとの高さ）。`initial_reserved` は
/// 呼び出し時点で既にこのリージョンに確定している脚注予約高さ（`composer.region_footnote_height`）。
/// 新リージョンが始まった行では予約を 0 にリセットして計算し直す（旧リージョンの予約は
/// `end_region` が既に確定させている）。
///
/// # 戻り値と打ち切り
///
/// `(計画, 打ち切ったか)`。次リージョンの脚注エリアが繰越でどれだけ埋まるかはここでは分からないので、
/// 次の 2 つの境界で計画を打ち切る（`carry` の不変条件）。呼び出し元（[`plan_paragraph_lines`] →
/// [`place_paragraph`]）が繰越を詰めてから残りを計画し直す:
///
/// - **脚注を分割した行**: その行は計画に含める（分割でそのリージョンは満杯になっている）。
/// - **繰越が残っている（`carry_pending`）状態での改リージョン**: その行は計画に含めない
///   （新リージョンの予約が決まっていないので、ベースラインを決められない）。
///
/// `carry_pending` が偽で脚注も分割されない文書ではどちらも起きず、段落全体を 1 回で計画する
/// （移行前と同一の結果）。
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
    let (reserved_after, own_splits) = match fit {
      LineFootnoteFit::Full(area) => (area, demands[i].iter().map(FootnoteDemand::line_count).collect()),
      LineFootnoteFit::Split(area, splits) => (area, splits),
      // 空のリージョンでも収まらない病的ケース（脚注の先頭 1 行がページ全高を超える等）。
      // 次リージョンへ送っても改善しないので、オーバーフローを許容してそのまま置く
      LineFootnoteFit::Rejected => {
        if !demands[i].is_empty() {
          warn!("脚注の高さがページ全体を超えるため、オーバーフローしたまま配置します");
        }
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
    });
    prev_depth = Some(line.depth);
    if split_here {
      return (plan, true);
    }
  }
  return (plan, false);
}

/// 脚注エリアの高さ課金パラメータ（`style.footnote` 由来、`geom` から切り出した純粋な値）
///
/// [`PageComposer::end_region`] の積み方 —「リージョン最初の脚注の直前にだけ `top_margin` + 区切り罫線、
/// 各脚注の直前に `rule_gap`」— をそのまま表す。予約高さの見積り（[`pack_footnotes`] / [`place_lines`]）が
/// 確定配置と同じ課金を通るようにするために、3 つを 1 つの値にまとめて持ち回る。
#[derive(Debug, Clone, Copy)]
struct FootnoteCharges {
  /// 本文と区切り罫線の間隔（`style.footnote.top_margin`）
  top_margin: Length,
  /// 区切り罫線の太さ（0 のとき描画しない、`style.footnote.rule_thickness`）
  rule_thickness: Length,
  /// 区切り罫線〜最初の脚注、および脚注どうしの間隔（`style.footnote.rule_gap`）
  rule_gap: Length,
}

impl FootnoteCharges {
  /// ページジオメトリから課金パラメータを取り出す
  fn of(geom: &PageGeometry) -> Self {
    return FootnoteCharges {
      top_margin: geom.footnote_top_margin,
      rule_thickness: geom.footnote_rule_thickness,
      rule_gap: geom.footnote_rule_gap,
    };
  }

  /// 脚注エリアを `base_reserved` まで確保済みのリージョンへ、脚注 1 個を新たに置くときの固定費（pt）
  ///
  /// `base_reserved == 0`（＝このリージョンにまだ脚注が無い）ならその脚注が区切り罫線を連れてくるので
  /// `top_margin + rule_thickness + rule_gap`、2 個目以降は `rule_gap` だけ。
  fn entry_overhead(self, base_reserved: Length) -> Length {
    if base_reserved == Length::ZERO {
      return self.top_margin + self.rule_thickness + self.rule_gap;
    }
    return self.rule_gap;
  }
}

/// 脚注 1 個の分割可能な需要（純粋データ）
///
/// `prefix[k]` は先頭 `k` 行だけを積んだときの本体高さで、`prefix[0]` は 0、末尾が全体の高さ。
/// 「予算に何行入るか」を単調増加列の探索に落とすために持つ。
struct FootnoteDemand {
  /// 先頭 `k` 行の積み上げ高さ（長さ = 行数 + 1）
  prefix: Vec<Length>,
}

impl FootnoteDemand {
  /// 行分割済みの脚注本体から需要を組み立てる
  ///
  /// 漸化式（先頭行 = `height`、以降 `leading.max(前行の depth + height)`、末尾に最終行の `depth`）は
  /// [`PageComposer::end_region`] の確定配置と同一でなければならない（ずれた分だけ本文と脚注が重なる）。
  fn new(lines: &[Line], leading: Length) -> Self {
    let mut prefix = Vec::with_capacity(lines.len() + 1);
    prefix.push(Length::ZERO);
    let mut baseline = Length::ZERO;
    let mut prev_depth = Length::ZERO;
    for (i, line) in lines.iter().enumerate() {
      if i == 0 {
        baseline = line.height;
      } else {
        baseline += leading.max(prev_depth + line.height);
      }
      prev_depth = line.depth;
      prefix.push(baseline + prev_depth);
    }
    return FootnoteDemand { prefix };
  }

  /// 本体の行数
  fn line_count(&self) -> usize { return self.prefix.len() - 1; }

  /// 全行を置くのに要する本体高さ（pt）
  fn full_height(&self) -> Length { return *self.prefix.last().expect("prefix は必ず prefix[0] を持つ"); }

  /// 先頭 1 行だけを置くのに要する本体高さ（pt）。行が無ければ 0
  fn first_line_height(&self) -> Length { return self.prefix.get(1).copied().unwrap_or(Length::ZERO); }

  /// 高さ `allowance` に収まる最大の行数を返す（`prefix` の単調増加性を使う）
  fn fit_lines(&self, allowance: Length) -> usize {
    let mut fit = 0;
    for (k, height) in self.prefix.iter().enumerate().skip(1) {
      if *height > allowance {
        break;
      }
      fit = k;
    }
    return fit;
  }
}

/// 脚注エリアへの詰め込み結果
struct FootnotePacking {
  /// 脚注ごとの、このリージョンへ置く行数（入力 `demands` と同順・同長）。
  /// 行数未満なら残りは繰り越す
  splits: Vec<usize>,
  /// この詰め込みで脚注エリアに追加される高さ（pt、固定費込み）
  height: Length,
}

/// 脚注エリアの「予算に対して何行入るか」を決める唯一の純粋関数（#227）
///
/// `demands` を先頭から順に、`budget`（`base_reserved` から**さらに**割ける高さ）へ詰められるだけ詰める。
/// 分割の算術をここ 1 箇所に閉じることで、見積り（[`place_lines`] の計画）と確定配置
/// （[`PageComposer::end_region`]）が食い違わないようにする。
///
/// `require_first_line` は呼び出し元の 2 つのモードを分ける:
///
/// - `true`（本文行の自前の脚注）: マーカーのある行と脚注の先頭は同じページに置く規則があるため、
///   **全脚注に最低 1 行**を割り当てられなければ `None` を返す（＝呼び出し側は本文行ごと次リージョンへ
///   送る、従来どおりの振る舞い）。同じ行に複数の脚注があるときは、後続の脚注のぶん
///   （`rule_gap` + 先頭 1 行）を予約してから手前の脚注へ最大量を割り当てる。
/// - `false`（前リージョンからの繰越）: 最低保証は要らない（続きなので次リージョンへ送ってよい）。
///   ただし先頭の脚注だけは必ず 1 行進めて、[`PageComposer::seed_carry`] のループが停止することを保証する。
///   1 行がリージョン全高にすら収まらない病的ケースはオーバーフローを許容して警告する。
///
/// 順序は保存する: ある脚注が分割された時点で以降の脚注はこのリージョンに置かない
/// （置くと繰越と入れ替わって出現順が壊れる）。`require_first_line` のときは各脚注の最低 1 行を
/// 予約済みなので、分割された脚注の後ろにも先頭行が残せる。
fn pack_footnotes(
  demands: &[FootnoteDemand],
  base_reserved: Length,
  budget: Length,
  charges: FootnoteCharges,
  require_first_line: bool,
) -> Option<FootnotePacking> {
  let mut splits = Vec::with_capacity(demands.len());
  let mut height = Length::ZERO;
  for (j, demand) in demands.iter().enumerate() {
    let overhead = charges.entry_overhead(base_reserved + height);
    // 後続の脚注に最低 1 行ずつ残す（`require_first_line` のときのみ）。この脚注を置いた後なので
    // 後続の固定費は `rule_gap` だけになる
    let rest_min: Length = if require_first_line {
      demands[j + 1..].iter().map(|rest| return charges.rule_gap + rest.first_line_height()).sum()
    } else {
      Length::ZERO
    };
    let mut placed = demand.fit_lines(budget - height - overhead - rest_min);
    if placed == 0 && demand.line_count() > 0 {
      if require_first_line {
        // この行の脚注は先頭 1 行すら置けない。呼び出し側が行ごと次リージョンへ送る
        return None;
      }
      if j > 0 {
        // 繰越: 手前の脚注で予算が尽きた。以降はこのリージョンに置かない（出現順を保つ）
        splits.extend(std::iter::repeat_n(0, demands.len() - j));
        return Some(FootnotePacking { splits, height });
      }
      // 繰越の先頭が 1 行も入らない病的ケース。次リージョンへ送っても改善しないので、
      // オーバーフローを許容して 1 行進める（[`PageComposer::seed_carry`] のループの停止条件）
      warn!("脚注 1 行の高さがページ全体を超えるため、オーバーフローしたまま配置します");
      placed = 1;
    }
    height += overhead + demand.prefix[placed];
    splits.push(placed);
    // 分割された脚注より後ろをこのリージョンに置くと繰越と出現順が入れ替わる。`require_first_line` の
    // ときは後続の最低 1 行を予約済みなので、そのまま詰め続けてよい
    if placed < demand.line_count() && !require_first_line {
      splits.extend(std::iter::repeat_n(0, demands.len() - j - 1));
      return Some(FootnotePacking { splits, height });
    }
  }
  return Some(FootnotePacking { splits, height });
}

/// 脚注を「先頭 `placed` 行」と「残り」に分ける
///
/// 残りは `continued: true`（前ページからの続き）になる。マーカーは行分割時点で先頭行の箱に
/// 入っているため、行単位で切れば繰越側にマーカーは現れない（追加処理は不要）。
/// `placed == 0` なら全部が残り、`placed >= 行数` なら残りは無い。
fn split_pending(mut footnote: PendingFootnote, placed: usize) -> (Option<PendingFootnote>, Option<PendingFootnote>) {
  if placed >= footnote.lines.len() {
    return (Some(footnote), None);
  }
  let rest = footnote.lines.split_off(placed);
  let tail = PendingFootnote {
    number: footnote.number,
    index: footnote.index,
    continued: true,
    lines: rest,
    leading: footnote.leading,
  };
  if placed == 0 {
    return (None, Some(tail));
  }
  return (Some(footnote), Some(tail));
}

/// 配置計画から widow/orphan 違反を 1 つ検出し、追加すべき強制改リージョン点を返す（純粋関数）
///
/// 判定はどちらも段落の境界行（先頭 = index 0・末尾 = index n-1）だけを見る。中間リージョンが
/// [`MIN_LINES_AT_BREAK`] 未満でも（伝統的な widow/orphan ではないので）対象にしない。
///
/// - **orphan**: 段落先頭リージョンの行数が最小行数未満 → 段落全体を次リージョンへ送る（`Some(0)`）。
///   先頭行が既にリージョン先頭（回避不能）なら補正しない。
/// - **widow**: 段落末尾リージョンの行数が最小行数未満 → 末尾 `min_lines` 行をまとめて送る
///   （`Some(n - min_lines)`）。前側に最小行数を確保できない短い段落（`n < 2 * min_lines`）は
///   段落全体を送る（`Some(0)`）。それも回避不能なら補正しない。
///
/// `is_paragraph_start` / `is_paragraph_end` は `plan` が段落の端を含むかを表す。脚注の分割（#227）で
/// 計画は段落の途中で打ち切られ得るため、chunk の端を段落の端と誤認して孤立補正を掛けないよう
/// 該当側の判定を落とす（そもそも分割点は「脚注がページを埋めた」ことによる強制点なので、
/// 孤立回避のために動かせない）。
///
/// 返した点が既に強制済みなら呼び出し側（[`plan_paragraph_lines`]）は停止する（前進のみ・有限停止）。
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
///
/// まず [`place_lines`] で従来どおりの貪欲配置を求め、[`pick_correction`] が返す境界孤立の補正点を
/// 強制改リージョン点として 1 つずつ足しては再フローする。補正は必ず前方（小さい index）へしか
/// break を動かさず、返る補正点が既に強制済みになったら停止する（回避不能ケースは孤立を許容）。
/// 孤立が生じない段落では `forced` が全 `false` のまま確定し、移行前と同一の計画になる。
///
/// `demands` / `initial_reserved` は [`place_lines`] にそのまま引き継ぐ（脚注構成は
/// 補正の再フローで変わらないため、リトライのたびに再計算しない）。
///
/// `lines` は段落の**残り**（chunk）で、`is_paragraph_start` はそれが段落の先頭から始まるかを表す。
/// `carry_pending` と戻り値の `bool`（打ち切ったか）は [`place_lines`] に素通し / から素通しする。
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
///
/// Glue（アキ）・Penalty（分割コスト）・Anchor（ゼロサイズマーカー）は内容ではない。
/// keep-with-next のグループ判定・孤立判定で「見出し」「本文」に当たる実ブロックを選ぶのに使う。
fn is_content_block(block: &Block) -> bool {
  return matches!(
    block,
    Block::Paragraph { .. }
      | Block::Table { .. }
      | Block::Image { .. }
      | Block::Rule { .. }
      | Block::ComposedLine { .. }
      | Block::MathBlock { .. }
  );
}

/// `blocks[start]` が keep-with-next グループの先頭なら、グループ末尾（最後の内容ブロック）の
/// index を返す（純粋・幾何非依存）。
///
/// グループは「内容ブロック →（アキ・アンカーを跨いで）[`PENALTY_FORBID_BREAK`] →次の内容ブロック」の
/// 連鎖。連続する見出しは各々が FORBID を出すので 1 つの極大グループにまとまる。末尾の内容ブロック（本文）は
/// keep 対象の相手であって、それ自身は次へ FORBID を持たない。強制改ページ（[`PENALTY_FORCE_BREAK`]）が
/// 挟まれば連鎖を断つ。連結が 1 つも無ければ（先頭が見出しでなければ）`None`。
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
///
/// カーソル `y`（`cursor_at_edge = cae`）に置いたとき、そのブロックの配置関数が改段 / 改ページ
/// する（= 見出しを孤立させる）かと、改段しない場合の配置後カーソルを返す。各 `place_*` の
/// リージョン判定を鏡写しにする（`place_math_block` / `place_table` は「新リージョンなら収まるとき
/// だけ先に改」、`consider_break`（画像・罫線）はオーバーフローで無条件に改）。
fn atomic_place_sim(block: &Block, y: Length, cae: bool, geom: &PageGeometry) -> (bool, Length) {
  match block {
    Block::Image { height, .. } => {
      let h = height.unwrap_or(Length::ZERO);
      return (y + h > geom.page_limit, y + h);
    },
    Block::Rule { height, .. } => return (y + *height > geom.page_limit, y + *height),
    Block::MathBlock { body, .. } => {
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
///
/// 先頭チャンク: 見出し段落は全行、末尾が段落なら widow/orphan（[`MIN_LINES_AT_BREAK`]）で丸ごと次へ
/// 送られない最小行数 `min(MIN_LINES_AT_BREAK, 行数)`。ベースライン漸化式は [`place_lines`] /
/// [`place_paragraph`] と同一（段落先頭行は `cursor_at_edge` ならアセント分下げ、2 行目以降は
/// `max(leading, 前行の深さ + 高さ)`）。末尾が図表等なら [`atomic_place_sim`] の改リージョン判定を使う。
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
        let effective = if *align == model::Align::Left {
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
  items: &[model::HItem],
  leading: Length,
  column_width: Length,
  indent: Length,
  right_indent: Length,
  align: model::Align,
) {
  let available = (column_width - indent - right_indent).max(Length::ZERO);
  // 両端揃えは左揃え段落にのみ適用する。中央・右寄せ段落は行を自然幅のまま組み、
  // 確定後に揃えオフセットでシフトする（伸縮すると余り幅が消えて揃え自体が無意味になる）
  let effective_alignment = if align == model::Align::Left {
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
    // ベースライン送りと改リージョン点を先に確定する（widow/orphan 制御込みの純粋計算）。
    // 配置ループは計画に従うだけにして、計画と配置で幾何がずれないようにする。
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
      let baseline = placement.baseline;
      last_baseline = baseline;
      // 着地段（advance_region 後）の左端オフセットを行ごとに加算する。リンク矩形の収集より前に行う。
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
      // 段落先頭行の確定位置（改段・改ページ後）で未解決アンカーを解決する。行の上端（段の左端）を指す
      if is_paragraph_start && i == 0 {
        composer.resolve_pending_anchors(col_off, baseline - line.height);
      }
      composer.collect_line_links(&line, baseline);
      // この行の脚注を、計画が決めた行数だけ現在リージョンへ登録する。残りは繰越へ回す
      // （実際のページ下部配置は `end_region` が行う）。
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
///
/// 段落先頭行と同じ規則で配置する: 直前が底辺基準ブロックならアセント分下げ、段下限を
/// 超えるなら改段または改ページし、確定位置で未解決アンカーを解決してリンク矩形を収集する。配置後は
/// カーソルを `baseline + leading` まで進める。行分割（`break_lines`）は通さない。
/// 段組みでは着地段の左端オフセットを行のボックス・リンクに加算する（行は段左端基準で組まれているため）。
fn place_single_line(composer: &mut PageComposer, geom: &PageGeometry, mut line: Line, leading: Length) {
  let mut baseline = composer.y;
  if composer.cursor_at_edge {
    baseline += line.height;
  }
  if baseline + line.depth > composer.region_limit(geom) {
    composer.advance_region(geom);
    baseline = geom.margin_top;
  }
  // 着地段（advance_region 後）の左端オフセットを行に加算する。リンク矩形の収集より前に行う。
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
/// 本体 Atom（`body`）は `block` 段で局所座標まで組み上がっているため、ここでは段幅の中で
/// 揃え（`align`、既定は中央）オフセットを 1 回算出し、各行番号を段端（`numbers_on_right` で
/// 左右）へ寄せて確定座標を与えるだけ。当面は改ページ（段）単位の不可分ブロックとして扱う
/// （収まらない場合に新しい段／ページなら収まるときだけ先に改段・改ページする）。段組みでは
/// fit チェック後の段オフセットを本体・行番号の x に加算する。
fn place_math_block(
  composer: &mut PageComposer,
  geom: &PageGeometry,
  body: model::HBox,
  numbers: Vec<model::MathRowNumber>,
  numbers_on_right: bool,
  align: model::Align,
  column_width: Length,
) {
  let total_height = body.height + body.depth;
  let limit = composer.region_limit(geom);
  if composer.y + total_height > limit && geom.margin_top + total_height <= limit {
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
  column_width: Length,
  align: model::Align,
) {
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
    if composer.y + *height > composer.region_limit(geom) {
      flush(composer, &mut placed_rows);
      composer.advance_region(geom);
    }
    placed_rows.push(PlacedTableRow {
      row: row.clone(),
      top_y: composer.y,
      height: *height,
    });
    composer.y += *height;
  }
  for (row, height) in table.rows.iter().zip(&row_heights) {
    if composer.y + *height > composer.region_limit(geom) {
      flush(composer, &mut placed_rows);
      composer.advance_region(geom);
      // 改段・改ページ後の先頭にヘッダ行を再描画する
      for (head_row, head_height) in table.head.iter().zip(&head_heights) {
        placed_rows.push(PlacedTableRow {
          row: head_row.clone(),
          top_y: composer.y,
          height: *head_height,
        });
        composer.y += *head_height;
      }
    }
    placed_rows.push(PlacedTableRow {
      row: row.clone(),
      top_y: composer.y,
      height: *height,
    });
    composer.y += *height;
  }
  flush(composer, &mut placed_rows);
}

#[cfg(test)]
mod tests {
  use model::{
    Block, ColumnAlign, ColumnWidth, GlyphRun, HBox, HBoxContent, HItem, Length, Line, PENALTY_FORBID_BREAK, Page,
    PlacedBlock, TableBox, TableCellBox, TableColumn, TableRowBox, TextAlignment,
  };

  use super::{
    super::break_lines::GreedyBreaker, FootnoteCharges, FootnoteDemand, LinePlacement, PageGeometry, break_pages,
    is_content_block, keep_group_end, pack_footnotes, plan_paragraph_lines,
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
      margin_top: Length::pt(10.0),
      page_limit: Length::pt(50.0),
      default_font_size: Length::pt(10.0),
      line_height_factor: 1.0,
      table_cell_padding: Length::pt(2.0),
      num_columns: 1,
      column_gap: Length::pt(0.0),
      flush_bottom: false,
      // 罫線・top_margin は 0（無効）にし、rule_gap だけ旧 FOOTNOTE_GAP と同じ 4pt にすることで、
      // 既存の脚注テストの数値（qualitative な比較込み）を変えずに保つ。
      footnote_top_margin: Length::ZERO,
      footnote_rule_length: Length::ZERO,
      footnote_rule_thickness: Length::ZERO,
      footnote_rule_color: None,
      footnote_rule_gap: Length::pt(4.0),
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
      align: model::Align::Left,
    };
  }

  /// 幅 0 の脚注マーカー（`HItem::Footnote`）を作るテストヘルパ
  fn footnote_item(number: u32, body: Vec<HItem>, leading: Length) -> HItem {
    return HItem::Footnote {
      number,
      // テストヘルパは通し採番相当（index と表示番号が 1 対 1）で十分
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
      align: model::Align::Left,
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

  #[test]
  fn footnote_places_at_page_bottom_without_overlap() {
    // Arrange — 1 行の段落に脚注 1 個。ページには十分な余白がある
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_item(
      1,
      vec![test_box()],
      pt(12.0),
    )])];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 本文行は baseline=10（先頭行）、脚注はページ下部（本文の下、page_limit=50 以内）
    assert_eq!(pages.len(), 1);
    let body_baseline = page_baselines(&pages[0])[0];
    assert!(close(body_baseline, 10.0), "body baseline={}", body_baseline.to_pt());
    let footnote_baseline = footnote_baselines(&pages[0], 1)[0];
    // 本文の下端（baseline + depth = 12）より脚注が下にあり、page_limit（50）を超えない
    assert!(footnote_baseline.to_pt() > body_baseline.to_pt() + 2.0, "脚注が本文より下にあるはず");
    assert!(footnote_baseline.to_pt() <= geom.page_limit.to_pt(), "脚注が page_limit を超えないはず");
  }

  #[test]
  fn line_with_overflowing_footnote_moves_to_next_page_with_it() {
    // Arrange — 3 行の 1 行段落（baseline 10,22,34 で y=46 まで進む）の後、4 個目の 1 行段落に
    // 大きな脚注（2 行本体）を付ける。脚注なしなら baseline=46 で収まるが、脚注込みだと溢れる
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページ目は先頭 3 行だけ・脚注なし。行と脚注は 2 ページ目へ一緒に送られ、重ならない
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
    // Arrange — 1 行の段落に脚注 2 個
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![
      footnote_item(1, vec![test_box()], pt(12.0)),
      footnote_item(2, vec![test_box()], pt(12.0)),
    ])];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 出現順（1 → 2）に、脚注 1 の下に脚注 2 が積まれる
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].footnotes.len(), 2);
    assert_eq!(pages[0].footnotes[0].number, 1);
    assert_eq!(pages[0].footnotes[1].number, 2);
    let first = footnote_baselines(&pages[0], 1)[0];
    let second = footnote_baselines(&pages[0], 2)[0];
    assert!(second.to_pt() > first.to_pt(), "脚注 2 は脚注 1 より下");
  }

  /// `lines` 行の本体を持つ脚注マーカーを作る（各行は [`test_box`] 1 個 = 高さ 8・深さ 2）
  ///
  /// `test_geometry` の脚注行送り 12 では、`n` 行の本体の積み上げ高さは `12n - 2`。
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
    // Arrange — 本文 1 行（baseline 10・下端 12）+ 4 行の脚注。脚注エリアに使えるのは 50-12=38 で、
    // rule_gap 4 を引くと 34 = 3 行ぶん（積み上げ高さ 12n-2）。4 行目が繰り越される
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![]),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — マーカー行と先頭 3 行が同じページに残り、4 行目だけが次ページへ繰り越される
    assert_eq!(footnote_layout(&pages), vec![(1, vec![(1, false, 3)]), (1, vec![(1, true, 1)])], "{pages:?}");
    // 1 ページ目: 本文の下（下端 12）から page_limit=50 までを脚注が埋める
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0]));
    assert_eq!(footnote_baselines(&pages[0], 1), pts(&[24.0, 36.0, 48.0]));
    // 2 ページ目: 繰越は脚注エリアの先頭。本文（下端 12）と重ならない
    let carried = footnote_baselines(&pages[1], 1)[0];
    assert!(carried.to_pt() > 12.0, "繰越が本文と重ならないはず: {}", carried.to_pt());
    assert!(carried.to_pt() + 2.0 <= geom.page_limit.to_pt() + 1e-3, "繰越が page_limit を超えないはず");
  }

  #[test]
  fn carried_footnote_stacks_before_own_footnote_of_the_page() {
    // Arrange — 1 ページ目の脚注 1 が繰り越され、2 ページ目には自前の脚注 2 がある
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 4)]),
      single_line_paragraph(vec![footnote_of_lines(2, 1)]),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 ページ目は「繰越 → そのページの脚注」の順に積まれる
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
    // Arrange — 10 行の脚注。1 リージョンの脚注エリアには 3 行しか入らないので繰越が連鎖する
    let geom = test_geometry();
    let blocks = vec![
      single_line_paragraph(vec![footnote_of_lines(1, 10)]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
      single_line_paragraph(vec![]),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 3 + 3 + 3 + 1 行に分かれて 4 ページの連鎖になる。各ページには本文も 1 行ずつ流れる
    // （繰越が残っていても、そのページに入る本文まで追い出さないことの番人）
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
    // 本体は 10 行すべて保存され、先頭断片だけがマーカーを持つ（＝繰越にマーカーは出ない）
    let total: usize =
      footnote_layout(&pages).iter().flat_map(|(_, f)| return f.clone()).map(|(_, _, n)| return n).sum();
    assert_eq!(total, 10, "脚注の行が欠落しないはず");
  }

  #[test]
  fn footnote_split_on_last_line_is_drained_at_document_end() {
    // Arrange — 文書の最後の行で脚注が分割される（後続ブロックが無い）。`finish` が繰越を
    // 出し切らないと、脚注の残りが黙って落ちる
    let geom = test_geometry();
    let blocks = vec![single_line_paragraph(vec![footnote_of_lines(1, 4)])];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 繰越ぶんだけのページが最後に足される（置く本文がもう無いので本文 0 行は正しい）
    assert_eq!(footnote_layout(&pages), vec![(1, vec![(1, false, 3)]), (0, vec![(1, true, 1)])], "{pages:?}");
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
    // Arrange — 4 行の脚注（積み上げ高さ 10 / 22 / 34 / 46）
    let demands = vec![demand_of_lines(4)];

    // Act — 予算 38 = rule_gap 4 + 34（3 行ぶん）ちょうど
    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), true).expect("先頭 1 行は入る");

    // Assert — ちょうど 3 行入り、4 行目は繰越
    assert_eq!(packing.splits, vec![3]);
    assert!(close(packing.height, 38.0), "{}", packing.height.to_pt());
  }

  #[test]
  fn pack_footnotes_rejects_when_first_line_does_not_fit() {
    // Arrange — 先頭 1 行に必要なのは rule_gap 4 + 10 = 14
    let demands = vec![demand_of_lines(2)];

    // Act — 予算 13 では 1 行も置けない
    let packing = pack_footnotes(&demands, Length::ZERO, pt(13.0), gap_only_charges(), true);

    // Assert — 呼び出し側が「本文行ごと次リージョンへ送る」ためのシグナル
    assert!(packing.is_none());
  }

  #[test]
  fn pack_footnotes_reserves_a_first_line_for_later_footnotes_on_the_same_line() {
    // Arrange — 同じ行に長い脚注と短い脚注。マーカー行と脚注の先頭は同じページに置く規則があるので、
    // 手前の脚注が予算を食い尽くしてはいけない
    let demands = vec![demand_of_lines(4), demand_of_lines(1)];

    // Act — 予算 38。予約が無ければ手前が 3 行取って後続が 0 行（= 拒否）になる
    let packing = pack_footnotes(&demands, Length::ZERO, pt(38.0), gap_only_charges(), true)
      .expect("予約すれば両方に先頭行が置ける");

    // Assert — 手前は後続の 1 行ぶん（rule_gap 4 + 10）を残して 1 行だけ置き、残りを繰越にする
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
    // Arrange — 1 行の段落に脚注 2 個、区切り罫線を有効にする
    let geom = geometry_with_footnote_rule();
    let blocks = vec![single_line_paragraph(vec![
      footnote_item(1, vec![test_box()], pt(12.0)),
      footnote_item(2, vec![test_box()], pt(12.0)),
    ])];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 罫線は 1 本だけ（脚注 2 個目には付かない）、脚注 1 の blocks 先頭に置かれる
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
  fn footnote_rule_reservation_does_not_overlap_when_footnote_starts_new_region() {
    // Arrange — line_with_overflowing_footnote_moves_to_next_page_with_it と同型だが、区切り罫線
    // ぶんの追加予約（top_margin + rule_thickness）が新リージョンの先頭行でも正しく効くかを見る
    // （advisor 指摘の回帰テスト: 見積りと配置が非対称だと脚注エリアが page_limit を超えて重なる）。
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 行と脚注は 2 ページ目へ一緒に送られ、罫線は新リージョンに 1 本だけ、
    // かつ脚注の最終行が page_limit をはみ出さない（見積りと配置が一致している証拠）
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
    // Arrange — 3 行の段落。ベースラインは margin_top から leading ずつ進む
    let geom = test_geometry();

    // Act
    let pages =
      break_pages(vec![paragraph_of_lines(3)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページに 3 行、baseline は 10, 22, 34
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
    // Arrange — page_limit=50, leading=12: 幾何だけなら 4 行目まで（10,22,34,46）が入り 5 行目が
    // 改ページだが、それは末尾 1 行の孤立（widow）。widow 制御で末尾 2 行がまとめて 2 ページ目へ送られる
    // （3 行 / 2 行の分割）。どちらでも「2 ページに分かれ、2 ページ目先頭 baseline = margin_top」は成り立つ。
    let geom = test_geometry();

    // Act
    let pages =
      break_pages(vec![paragraph_of_lines(5)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 ページに分かれ、2 ページ目の先頭 baseline は margin_top
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
    // Arrange — 先行 3 行段落で 10,22,34 を埋め、カーソルは 46 へ。続く 3 行段落は幾何だけなら
    // 先頭行が 46 に入り 2 行目が改ページ → 先頭 1 行の孤立（orphan）。orphan 制御で段落全体が次ページへ。
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(3), paragraph_of_lines(3)];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページ目は先行段落の 3 行だけ、2 ページ目に後続段落の 3 行が揃う
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0, 34.0]), "先頭行を孤立させず先行段落のみ");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0, 34.0]), "後続段落は丸ごと 2 ページ目へ");
  }

  #[test]
  fn widow_last_line_kept_with_previous() {
    // Arrange — 5 行段落。幾何だけなら 4 行目まで入り 5 行目が改ページ = 末尾 1 行の孤立（widow）。
    let geom = test_geometry();

    // Act
    let pages =
      break_pages(vec![paragraph_of_lines(5)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — widow 制御で 3 行 / 2 行に分かれ、末尾 2 行が 2 ページ目に揃う
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0, 34.0]), "1 ページ目は 3 行（4 行目を繰り下げ）");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0]), "末尾 2 行が 2 ページ目に揃う");
  }

  #[test]
  fn short_paragraph_moves_whole_rather_than_split() {
    // Arrange — 先行 2 行段落でカーソルを 34 へ。続く 3 行段落は幾何だけなら 2 行入り 3 行目が改ページ。
    // n=3 は内部に許容 break が無い（分割すると必ず孤立）ので、段落全体が次ページへ送られる。
    let geom = test_geometry();
    let blocks = vec![paragraph_of_lines(2), paragraph_of_lines(3)];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 22.0]), "先行段落の 2 行だけ");
    assert_eq!(page_baselines(&pages[1]), pts(&[10.0, 22.0, 34.0]), "3 行段落は分割せず丸ごと次ページ");
  }

  #[test]
  fn oversized_paragraph_builds_without_hang() {
    // Arrange（回避不能ケースの番人）— 1 ページに 4 行しか入らないのに 20 行の段落。孤立を完全には
    // 避けられないが、無限ループ・行の欠落なくビルドが完了し、全 20 行が保存されることを確認する。
    let geom = test_geometry();

    // Act
    let pages =
      break_pages(vec![paragraph_of_lines(20)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 行総数は 20 のまま、複数ページに分かれる
    let total: usize = pages.iter().map(|p| return page_baselines(p).len()).sum();
    assert_eq!(total, 20, "行が欠落しない: {pages:?}");
    assert!(pages.len() >= 5, "4 行/ページなので 5 ページ以上に分かれる: {}", pages.len());
  }

  #[test]
  fn plan_leaves_fitting_paragraph_untouched() {
    // Arrange — 3 行がすべて収まる段落。widow/orphan 補正は起きず、貪欲配置と同一になる。
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

    // Assert — 10,22,34 で改リージョンなし。脚注が無いので計画は打ち切られない
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
      ]
    );
  }

  #[test]
  fn plan_defers_orphan_first_line() {
    // Arrange — カーソル 46 で 3 行。幾何だけなら先頭行が 46 に入り 2 行目で改ページ（orphan）。
    let lines = vec![test_line(), test_line(), test_line()];

    // Act — y0=46
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

    // Assert — 先頭行から新リージョン開始、全行が新リージョンに 10,22,34 で並ぶ
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: true,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
      ]
    );
  }

  #[test]
  fn plan_pulls_widow_last_line_back() {
    // Arrange — カーソル 10 で 5 行。幾何だけなら 4 行入り 5 行目が widow。
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

    // Assert — 末尾 2 行（index 3,4）が新リージョンへまとまる（3 行 / 2 行）
    assert!(!truncated, "繰越も分割も無いので計画は打ち切られない");
    assert_eq!(
      plan,
      vec![
        LinePlacement {
          baseline: pt(10.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(34.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(10.0),
          starts_region: true,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
        LinePlacement {
          baseline: pt(22.0),
          starts_region: false,
          reserved_after: Length::ZERO,
          own_splits: Vec::new()
        },
      ]
    );
  }

  #[test]
  fn vspace_shifts_following_baseline() {
    // 固定アキ（glue）は次のブロックのベースラインを下へずらす
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(5.0)),
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    let baselines: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Line { baseline_y, .. } => return Some(*baseline_y),
        _ => return None,
      })
      .collect();
    // 1 つ目: 10。段落後カーソル 10+12=22、VSpace で 27
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

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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
        width: Some(pt(20.0)),
        height: Some(pt(15.0)),
        target_dpi: None,
        align: model::Align::Left,
      },
      paragraph_of_lines(1),
    ];

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // 画像 top=10, bottom=25。段落先頭行 baseline = 25 + height(8) = 33
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
    // 現ページに収まらない画像は改ページしてから配置する
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      Block::Image {
        path: "x.png".to_string(),
        width: Some(pt(20.0)),
        height: Some(pt(30.0)),
        target_dpi: None,
        align: model::Align::Left,
      },
    ];

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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
            font_type: model::FontType::Serif,
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

    let pages = break_pages(vec![], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    assert_eq!(pages.len(), 3);
  }

  #[test]
  fn leading_page_break_does_not_create_blank_page() {
    // 先頭が改ページ（Part 見出しの page_break_before 相当）でも、白紙の先頭ページを作らない
    let geom = test_geometry();
    let blocks = vec![Block::force_break(), paragraph_of_lines(1)];

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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

    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

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
      rows: (0..5).map(|i| return table_row(&format!("R{i}"))).collect(),
      breakable: true,
    };

    // Act
    let pages = break_pages(
      vec![Block::Table {
        table,
        align: model::Align::Left,
      }],
      Length::pt(100.0),
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
    use model::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading {
        key: "heading:0".to_string(),
        label: None,
      }),
      paragraph_of_lines(1),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — baseline=10, line.height=8 → アンカー y = 2、x = 0
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!(close(pages[0].anchors[0].y, 2.0));
    assert!(close(pages[0].anchors[0].x, 0.0));
    assert!(matches!(pages[0].anchors[0].mark, AnchorMark::Heading { label: None, .. }));
  }

  #[test]
  fn pending_anchor_resolves_on_page_after_break() {
    // Arrange — ページ 1 を埋めた後の Anchor は、改ページした次段落とともにページ 2 に解決される
    use model::AnchorMark;
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(4),
      Block::Anchor(AnchorMark::Label("tab:x".to_string())),
      paragraph_of_lines(1),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — アンカーはページ 1 ではなくページ 2（改ページ後）に解決される
    assert_eq!(pages.len(), 2, "{pages:?}");
    assert!(pages[0].anchors.is_empty(), "ページ 1 にアンカーは無い: {:?}", pages[0].anchors);
    assert_eq!(pages[1].anchors.len(), 1, "{:?}", pages[1].anchors);
    assert!(close(pages[1].anchors[0].y, 2.0));
  }

  #[test]
  fn paragraph_link_becomes_placed_link() {
    // Arrange — リンクマーカーで囲んだ段落から PlacedLink が確定する
    use model::LinkTarget;
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
      align: model::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — baseline=10, height=8, depth=2 → top=2, height=10, x=0, width=20
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
    // Arrange — indent=20, text_width=60 → 利用可能幅 40。box(10) を glue(5) で連結し折り返す
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
      align: model::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(60.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 利用可能幅 40 で折り返し（複数行）、全行の先頭ボックス x が indent(20) 以上
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
      // どのボックスも本文幅 60 を超えない（はみ出さない）
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
    // Arrange — indent=10, right_indent=10, text_width=60 → 利用可能幅 40。
    // box(10) を glue(5) で 6 連結し折り返す（右インデントぶん早く折り返す）
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
      align: model::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(60.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 利用可能幅は 60-10-10=40。全行が右端 text_width - right_indent = 50 を超えない
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
    // Arrange — indent=15 のリンク付き段落。リンク矩形も indent ぶん右へシフトされる
    use model::LinkTarget;
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
      align: model::Align::Left,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — x0=0 → +15、幅 20 は不変
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    let link = &pages[0].links[0];
    assert!(close(link.x, 15.0), "link.x={}", link.x.to_pt());
    assert!(close(link.width, 20.0), "link.width={}", link.width.to_pt());
  }

  #[test]
  fn centered_paragraph_shifts_line_to_horizontal_center() {
    // Arrange — box(10) 単一行の段落を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = (100 - 10) / 2 = 45。box.x は 45
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
    // Arrange — box(10) 単一行を align=Right、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Paragraph {
      items: vec![test_box()],
      leading: Length::pt(12.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: model::Align::Right,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = 100 - 10 = 90。box.x は 90（右端に揃う）
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
  fn stretchable_paragraph(align: model::Align) -> Block {
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
    // Arrange — text_width=27 で折り返す左揃え段落を justify 設定で配置する
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(model::Align::Left)];

    // Act
    let pages = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert — 1 行目の余り 2 が glue に配分され、2 つ目の box の右端が版面右端（27）に一致する
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
    // Arrange — 同じ段落を align=Center にすると justify 設定でも伸縮しない
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(model::Align::Center)];

    // Act
    let pages = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert — 1 行目は自然幅 25 のまま中央へシフト（オフセット = (27 − 25) / 2 = 1）。
    // box 間隔は自然 glue 幅 5 を保つ
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
    // Arrange — align=Right も伸縮せず、自然幅のまま右端へ寄る
    let geom = test_geometry();
    let blocks = vec![stretchable_paragraph(model::Align::Right)];

    // Act
    let pages = break_pages(blocks, Length::pt(27.0), &geom, &GreedyBreaker, TextAlignment::Justify);

    // Assert — 1 行目のオフセット = 27 − 25 = 2
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
    // Arrange — 幅 50 の単一 box を align=Center、text_width=30 で配置（行幅 > 利用可能幅）
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
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(30.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — シフト量は 0 にクランプされ、box.x は左端 0 のまま（左へはみ出さない）
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
    // Arrange — box(10) glue(5) box(10) glue(5) box(10) を text_width=35 で折り返す。
    // 1 行目は box glue box（幅 25）、2 行目は box（幅 10）になり、行幅が異なる。
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
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(35.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 行に折り返し、各行が自身の行幅で独立に中央寄せされる。
    // 1 行目: 幅 25 → オフセット (35-25)/2 = 5、2 行目: 幅 10 → オフセット (35-10)/2 = 12.5
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
    // Arrange — box 2 つ（幅 20）を囲むリンクを align=Center、text_width=100 で配置する。
    // リンク矩形も中央オフセット分シフトされ、確定 PlacedLink に追従する。
    use model::LinkTarget;
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
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 行幅 20 → 中央オフセット (100-20)/2 = 40。link.x=40、幅 20 は不変
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
    // Arrange — 幅 20 の画像を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: "x.png".to_string(),
      width: Some(pt(20.0)),
      height: Some(pt(15.0)),
      target_dpi: None,
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = (100 - 20) / 2 = 40
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!(close(*x, 40.0), "image.x={}", x.to_pt());
  }

  #[test]
  fn right_aligned_image_shifts_x_to_right_edge() {
    // Arrange — 幅 20 の画像を align=Right、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Image {
      path: "x.png".to_string(),
      width: Some(pt(20.0)),
      height: Some(pt(15.0)),
      target_dpi: None,
      align: model::Align::Right,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = 100 - 20 = 80（右端に揃う）
    let PlacedBlock::Image { x, .. } = first_image(&pages[0]) else {
      unreachable!()
    };
    assert!(close(*x, 80.0), "image.x={}", x.to_pt());
  }

  #[test]
  fn centered_rule_shifts_x_to_horizontal_center() {
    // Arrange — 幅 30 の罫線を align=Center、text_width=100 で配置する
    let geom = test_geometry();
    let blocks = vec![Block::Rule {
      width: Length::pt(30.0),
      height: Length::pt(2.0),
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — オフセット = (100 - 30) / 2 = 35
    let PlacedBlock::Rule { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Rule { .. })).expect("罫線があるはず")
    else {
      unreachable!()
    };
    assert!(close(*x, 35.0), "rule.x={}", x.to_pt());
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
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 表全体幅 24 → オフセット (100 - 24) / 2 = 38
    let PlacedBlock::Table { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!(close(*x, 38.0), "table.x={}", x.to_pt());
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
      align: model::Align::Center,
    }];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 表全体幅 = 本文幅 100 → オフセット 0
    let PlacedBlock::Table { x, .. } =
      pages[0].blocks.iter().find(|b| matches!(b, PlacedBlock::Table { .. })).expect("表があるはず")
    else {
      unreachable!()
    };
    assert!(close(*x, 0.0), "table.x={}", x.to_pt());
  }

  #[test]
  fn no_line_baseline_exceeds_page_limit() {
    // 不変条件: どのページの行も baseline + depth がページ下限を超えない
    let geom = test_geometry();

    let pages =
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
  fn composed_line(width: f32, height: f32, depth: f32, link: Option<model::LinkTarget>) -> Block {
    let width = pt(width);
    let height = pt(height);
    let depth = pt(depth);
    let links = link.map_or_else(Vec::new, |target| {
      return vec![model::LineLink {
        target,
        x0: Length::ZERO,
        x1: width,
      }];
    });
    return Block::ComposedLine {
      line: Line {
        boxes: vec![model::PositionedBox {
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
      },
      leading: Length::pt(12.0),
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — baseline は 10, 22（leading=12 ずつ）。行分割は通さない
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
    // Arrange — 見出しアンカー直後の ComposedLine（内部リンク付き）
    use model::{AnchorMark, LinkTarget};
    let geom = test_geometry();
    let blocks = vec![
      Block::Anchor(AnchorMark::Heading {
        key: "heading:0".to_string(),
        label: None,
      }),
      composed_line(20.0, 8.0, 2.0, Some(LinkTarget::Internal("heading:5".to_string()))),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — アンカーは行上端（baseline 10 − height 8 = 2）に解決
    assert_eq!(pages[0].anchors.len(), 1, "{:?}", pages[0].anchors);
    assert!(close(pages[0].anchors[0].y, 2.0));
    // リンクは PlacedLink 化（top=2, height=height+depth=10, 行き先は内部キー）
    assert_eq!(pages[0].links.len(), 1, "{:?}", pages[0].links);
    assert!(matches!(&pages[0].links[0].target, LinkTarget::Internal(k) if k == "heading:5"));
    assert!(close(pages[0].links[0].y, 2.0));
    assert!(close(pages[0].links[0].height, 10.0));
  }

  #[test]
  fn composed_lines_break_across_pages() {
    // Arrange — page_limit=50, margin_top=10, leading=12, depth=2:
    // baseline 10,22,34,46（46+2=48≤50）まで 1 ページ、5 本目で改ページ
    let geom = test_geometry();
    let blocks: Vec<Block> = (0..5).map(|_| return composed_line(20.0, 8.0, 2.0, None)).collect();

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 2 ページに分かれ、2 ページ目の先頭 baseline は margin_top
    assert_eq!(pages.len(), 2, "{pages:?}");
    let PlacedBlock::Line { baseline_y, .. } = pages[1].blocks.first().expect("2 ページ目に行があるはず")
    else {
      panic!("Line を期待");
    };
    assert!(close(*baseline_y, 10.0));
  }

  #[test]
  fn two_column_flow_fills_left_then_right_then_next_page() {
    // Arrange — 2 段（段幅 45・段オフセット 55）。9 行を流す。幾何だけなら左段 4 行 → 右段 4 行 →
    // 次ページ 1 行だが、その 9 行目は末尾 1 行の孤立（widow）になる。widow 制御が末尾 2 行
    // （8・9 行目）をまとめて次ページへ送るので、右段は 3 行に減り、次ページ左段に 2 行が並ぶ。
    let geom = two_column_geometry();

    // Act
    let pages =
      break_pages(vec![paragraph_of_lines(9)], Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 各行の (baseline_y, 先頭ボックス x) を採取して段送りを検証する
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
    // 左段: x≈0、baseline は margin_top から leading(12) ずつ
    assert_eq!(p0[0..4], [(10.0, 0.0), (22.0, 0.0), (34.0, 0.0), (46.0, 0.0)], "左段は x=0 で 10,22,34,46");
    // 右段: x≈55、baseline は margin_top にリセット。widow 制御で 3 行のみ（46 は空く）
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
    // Arrange（Hole A の番人）— 左段を埋めた後に右段へ入るリンク付き段落を置く。リンク矩形は
    // ボックスと一緒に段オフセット分シフトされていなければならない。
    use model::LinkTarget;
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
      align: model::Align::Left,
    };
    // paragraph_of_lines(5) で左段を埋め、カーソルを右段へ送ってからリンク段落を置く
    let blocks = vec![paragraph_of_lines(5), link_para];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — リンクは右段（x≈55）に確定し、2 ボックス分の幅 20 を持つ
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
    // Arrange（Hole B の番人）— 1 段に収まらない breakable 表が左段 → 右段にまたがるとき、
    // 2 つ目の断片の x に段オフセットが乗る。各行高 10・段高 40 で 4 行ずつ。
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
    let pages = break_pages(
      vec![Block::Table {
        table,
        align: model::Align::Left,
      }],
      Length::pt(100.0),
      &geom,
      &GreedyBreaker,
      TextAlignment::RaggedRight,
    );

    // Assert — 1 ページ内に 2 断片。1 つ目は左段（x≈0）、2 つ目は右段（x≈55）
    assert_eq!(pages.len(), 1, "{pages:?}");
    let xs: Vec<Length> = pages[0]
      .blocks
      .iter()
      .filter_map(|b| match b {
        PlacedBlock::Table { x, .. } => return Some(*x),
        _ => return None,
      })
      .collect();
    assert_eq!(xs.len(), 2, "左段断片 + 右段断片の 2 つ: {xs:?}");
    assert!(close(xs[0], 0.0), "1 つ目（左段）x={}", xs[0].to_pt());
    assert!(close(xs[1], 55.0), "2 つ目（右段）x={}", xs[1].to_pt());
  }

  // ---- keep-with-next（見出し直後の改ページ禁止・#168）----

  #[test]
  fn is_content_block_classifies_variants() {
    // Arrange / Act / Assert — 実ブロックだけ true、アキ・分割コスト・アンカーは false
    assert!(is_content_block(&paragraph_of_lines(1)));
    assert!(is_content_block(&Block::Rule {
      width: Length::pt(1.0),
      height: Length::pt(1.0),
      align: model::Align::Left,
    }));
    assert!(!is_content_block(&Block::fixed_space(pt(5.0))));
    assert!(!is_content_block(&forbid_break()));
    assert!(!is_content_block(&Block::force_break()));
  }

  #[test]
  fn keep_group_end_links_heading_to_following_block() {
    // Arrange — 見出し段落 → アキ → FORBID → 本文段落
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(2),
    ];

    // Act / Assert — 先頭（見出し）から末尾内容ブロック（index 3）までが 1 グループ
    assert_eq!(keep_group_end(&blocks, 0), Some(3));
  }

  #[test]
  fn keep_group_end_none_without_forbid() {
    // Arrange — FORBID の無い通常の段落並び
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      paragraph_of_lines(2),
    ];

    // Act / Assert — 連結が無いのでグループにならない
    assert_eq!(keep_group_end(&blocks, 0), None);
  }

  #[test]
  fn keep_group_end_chains_consecutive_headings() {
    // Arrange — 見出し → 見出し → 本文（各見出しが FORBID を出す）
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      forbid_break(),
      paragraph_of_lines(2),
    ];

    // Act / Assert — 連続見出しは 1 つの極大グループにまとまる（index 6 まで）
    assert_eq!(keep_group_end(&blocks, 0), Some(6));
  }

  #[test]
  fn keep_group_end_severed_by_forced_break() {
    // Arrange — 見出し直後に強制改ページ（page_break_after 相当）。FORBID は無い
    let blocks = vec![
      paragraph_of_lines(1),
      Block::fixed_space(pt(3.0)),
      Block::force_break(),
      paragraph_of_lines(2),
    ];

    // Act / Assert — 強制改ページは keep 連鎖を作らない
    assert_eq!(keep_group_end(&blocks, 0), None);
  }

  #[test]
  fn heading_kept_with_body_moves_to_next_page() {
    // Arrange — filler 3 行でカーソルを 46 まで進め、見出し（1 行）+ FORBID + 本文（2 行）。
    // 見出しは幾何的にはページ末尾（baseline 46）に入るが、本文先頭が入らない → 孤立。
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3), // filler
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 見出しごと 2 ページ目へ送られる（1 ページ目 = filler 3 行、2 ページ目 = 見出し + 本文 2 行）
    assert_eq!(line_counts(&pages), vec![3, 3], "{pages:?}");
  }

  #[test]
  fn heading_without_keep_marker_is_orphaned() {
    // Arrange — 同じ配置だが FORBID 無し（keep-with-next の対照）。
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1),
      Block::fixed_space(pt(0.0)),
      paragraph_of_lines(2),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 見出しは 1 ページ目末尾に孤立（1 ページ目 4 行 = filler + 見出し、2 ページ目 = 本文）
    assert_eq!(line_counts(&pages), vec![4, 2], "{pages:?}");
  }

  #[test]
  fn consecutive_headings_move_together() {
    // Arrange — filler で末尾近くまで進め、見出し + 見出し + 本文。
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 連続見出しが分断されず一体で 2 ページ目へ（1 ページ目 3 行、2 ページ目 = 見出し 2 + 本文 2 = 4 行）
    assert_eq!(line_counts(&pages), vec![3, 4], "{pages:?}");
  }

  #[test]
  fn heading_with_fitting_body_stays_in_place() {
    // Arrange — 見出しと本文先頭が同ページに収まる位置。keep-with-next は余計な移動をしない。
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(1), // filler → y=22
      paragraph_of_lines(1), // 見出し
      Block::fixed_space(pt(0.0)),
      forbid_break(),
      paragraph_of_lines(2), // 本文
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 全 4 行が 1 ページに収まり、改ページは起きない
    assert_eq!(line_counts(&pages), vec![4], "{pages:?}");
  }

  #[test]
  fn doc_final_heading_without_body_does_not_hang() {
    // Arrange — 文書末尾が見出し（後続の内容ブロック無し）。FORBID は宙に浮く。
    let geom = test_geometry();
    let blocks = vec![
      paragraph_of_lines(3),
      paragraph_of_lines(1),
      Block::fixed_space(pt(0.0)),
      forbid_break(),
    ];

    // Act — グループ相手が無いのでゲートは無効。ハングせず配置できる
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページに filler 3 行 + 見出し 1 行
    assert_eq!(line_counts(&pages), vec![4], "{pages:?}");
  }

  #[test]
  fn unavoidable_keep_at_region_top_does_not_add_blank_page() {
    // Arrange — 見出しがリージョン先頭にあり、本文先頭がフレッシュなリージョンでも収まらない。
    // 送っても空間は増えない（回避不能）ので、余計な空ページを差し込まない。
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 見出しは 1 ページ目先頭（回避不能で孤立を許容）、本文は 2 ページ目。空ページは生じない
    assert_eq!(line_counts(&pages), vec![1, 2], "{pages:?}");
  }

  // ---- 下端揃え（flush bottom・#169）--------------------------------------------------------------

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
      align: model::Align::Left,
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
    // Arrange — 罫線 3 本を伸縮アキ 2 個で挟み、4 本目でページを溢れさせる。1 ページ目は満杯（不足 2pt）
    // なので下端揃えの対象になる。伸縮アキは等量なので不足高さは 1pt ずつ配分される。
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページ目は各アキが 1pt ずつ伸び、末尾罫線の底辺が版面下限 50 に達する
    assert_eq!(pages.len(), 2);
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 25.0, 40.0]));
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "{:?}", pages[0]);
    // 最終ページは自然高のまま（揃えない）
    assert_eq!(rule_ys(&pages[1]), pts(&[10.0]));
  }

  #[test]
  fn flush_bottom_disabled_keeps_ragged_bottom() {
    // Arrange — 下端揃え無効。同じ入力でも従来どおり自然高で終わる
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — アキは自然値のまま、末尾は 48（<50）
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 24.0, 38.0]));
    assert!(approx(max_block_bottom(&pages[0]), 48.0));
  }

  #[test]
  fn flush_bottom_shifts_paragraph_lines() {
    // Arrange — 段落（Line）3 個を伸縮アキで挟み、大きな罫線で溢れさせる。行の底辺（baseline+depth）で
    // リージョン下端を測り、行を丸ごと下方シフトすることを確認する。
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 不足 6pt を等量アキへ 3:3 で配分（0.75 × 4 = 3pt ずつ累積）。末尾行の底辺が 50 に達する
    assert_eq!(page_baselines(&pages[0]), pts(&[10.0, 29.0, 48.0]));
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "{:?}", pages[0]);
  }

  #[test]
  fn flush_bottom_aligns_last_baseline_across_pages() {
    // Arrange — 罫線 7 本を伸縮アキで連ね、2 ページを満杯にして 3 ページ目に 1 本残す
    let geom = flush_geometry();
    let mut blocks = Vec::new();
    for i in 0..7 {
      if i > 0 {
        blocks.push(Block::stretchable_space(pt(4.0), pt(4.0)));
      }
      blocks.push(rule(10.0));
    }

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 満杯の 1・2 ページ目は下端が版面下限で揃い、最終ページだけ自然高
    assert_eq!(pages.len(), 3);
    assert!(approx(max_block_bottom(&pages[0]), 50.0), "page0 {:?}", pages[0]);
    assert!(approx(max_block_bottom(&pages[1]), 50.0), "page1 {:?}", pages[1]);
    assert!(max_block_bottom(&pages[2]) < 50.0 - 1e-3, "last page ragged {:?}", pages[2]);
  }

  #[test]
  fn flush_bottom_skips_page_before_forced_break() {
    // Arrange — 強制改ページ直前のページは揃えない（自然高のまま）
    let geom = flush_geometry();
    let blocks = vec![
      rule(10.0),
      Block::stretchable_space(pt(4.0), pt(4.0)),
      rule(10.0),
      Block::force_break(), // このページは強制改ページで終わる → 揃えない
      rule(10.0),
    ];

    // Act
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 1 ページ目は自然高（10, 24）のまま
    assert_eq!(pages.len(), 2);
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 24.0]));
    assert_eq!(rule_ys(&pages[1]), pts(&[10.0]));
  }

  #[test]
  fn flush_bottom_skips_page_without_stretch() {
    // Arrange — 固定アキ（stretch=0）だけのページは配分先が無いので揃えない
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
    let pages = break_pages(blocks, Length::pt(100.0), &geom, &GreedyBreaker, TextAlignment::RaggedRight);

    // Assert — 伸縮アキが無いので自然高（10, 24, 38）のまま
    assert_eq!(rule_ys(&pages[0]), pts(&[10.0, 24.0, 38.0]));
  }
}
