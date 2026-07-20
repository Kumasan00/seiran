//! PDF を生成するモジュール
//! このモジュールは、設定ファイルの `sources` に列挙されたテキストファイルから
//! PDF を生成するための主要な機能を提供します。

mod back_matter;
mod error;
mod front_matter;
mod outline;
mod page_values;
mod running;

#[cfg(test)]
mod dump;
#[cfg(test)]
mod golden;

use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
  time::Instant,
};

use back_matter::{assemble_back_matter, break_back_matter};
use citation::{References, read_references};
use error::BuildPdfError;
use font::{
  FontData, FontDataExt, FontMetrics, FontMetricsExt, FontRefs, FontRefsExt,
  shaper::{HarfRustShapers, HarfRustShapersExt, ShaperDatas, ShaperDatasExt, ShaperInstances, ShaperInstancesExt},
  validate_font,
};
use front_matter::{assemble_front_matter, break_front_matter};
use frontend::ParseSourceError;
use model::DocNode;
use outline::collect_outline_entries;
use page_values::BodyPageValues;
use pdf_gen::OutlineEntry;
use running::build_running_spec;
use tracing::{debug, debug_span, info, warn};
use typeset::LoweringContext;

/// ビルド成功時のサマリ（ユーザーチャンネルのレポータが表示する最小情報）
///
/// `tracing` の段ログとは別物で、コンパイラ型 CLI の「成果報告 1 行」（出力先・ページ数・所要
/// 時間）だけを運ぶ。表示は呼び出し側（`main::report_build`）が担う。
pub(super) struct BuildSummary {
  /// 出力した PDF のパス
  pub(super) output_path: PathBuf,
  /// 総ページ数
  pub(super) page_count: usize,
  /// ビルド全体の所要ミリ秒
  pub(super) total_elapsed_ms: u64,
}

/// 設定ファイルの `sources` から PDF を生成
///
/// 各 source ファイルを順次読み込み、`frontend::parse_source` でパース・評価して
/// ファイルごとに [`ParsedSource`] を作る（平坦化せずソース帰属を保持）。lowering は
/// 全ファイル + 書誌を 1 回の `lower_sources_with_headings` でまとめて処理し、1 つの文書として扱う。
///
/// # エラー戦略
///
/// - I/O 失敗（ファイル読み込み）は早期失敗
/// - パース・評価エラーは全ソースで集約して `MultipleSourceErrors` で報告
///
/// # Arguments
///
/// * `config_path` - 設定ファイルのパス
///
/// # Returns
///
/// 成功時は [`BuildSummary`]（出力先・ページ数・所要時間）。ユーザー向けの成果報告は呼び出し側の
/// `report_build` が担うため、ここでは最終サマリを `tracing` には出さない（二重表示を避ける）。
pub(super) fn build_pdf(config_path: &Path) -> miette::Result<BuildSummary> {
  let build_start = Instant::now();
  info!(config_path = %config_path.display(), "PDF のビルドを開始します");

  let config = config::read_config(config_path)?;
  let style = config::read_style(config.style_path.as_deref())?;
  config::validate_layout(&config, &style).map_err(|source| return BuildPdfError::Layout { source })?;
  let references = read_references(config.references_path.as_deref())?;

  let stage_start = Instant::now();
  let font_data = FontData::new(&config.font_configs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの読み込みが完了しました");

  // 描画前パイプライン（パース〜走り文配置）を 1 つの seam に束ねて確定レイアウトを得る。
  let laid_out = build_pages(&config, &style, &references, &font_data)?;

  // 描画は font_refs / metrics を再構築して使う。両者は `font_data` 上のゼロコピービュー
  // （FontRef のパース + head/hhea 参照）で、`build_pages` 内で使ったものは borrow が閉じていて
  // 持ち出せないため描画パス用にもう一度組み直す。フォントファイル自体の再読込は起きない。
  let font_refs = FontRefs::new(&config.font_configs, &font_data)?;
  let metrics = FontMetrics::new(&font_refs)?;

  let stage_start = Instant::now();
  let pdf_bytes =
    pdf_gen::create_pdf(&config, &font_data, &font_refs, &metrics, &laid_out.pages, &style, &laid_out.outline_entries)?;
  info!(page_count = laid_out.pages.len(), elapsed_ms = elapsed_ms(stage_start), "PDF の描画が完了しました");

  let stage_start = Instant::now();
  let output_path = config.output.pdf_path();
  fs::write(&output_path, pdf_bytes).map_err(|source| {
    return BuildPdfError::WritePdf {
      path: output_path.display().to_string(),
      source,
    };
  })?;
  info!(output_path = %output_path.display(), elapsed_ms = elapsed_ms(stage_start), "PDF の保存が完了しました");

  return Ok(BuildSummary {
    output_path,
    page_count: laid_out.pages.len(),
    total_elapsed_ms: elapsed_ms(build_start),
  });
}

/// [`build_pages`] の出力＝描画パスへ渡す確定レイアウト。
///
/// いずれもフォント非依存の所有データ（[`model::Page`] は計測済みグリフ列を持ち `FontRef` を
/// 借用しない、[`OutlineEntry`] はプレーンな見出し情報）なので、フォント関連の借用を伴わずに
/// `build_pages` の外へ持ち出せる。golden スナップショットテストは `pages` をダンプ対象にする。
pub(super) struct LaidOutDocument {
  /// 前付け + 本文を連結した確定ページ列（走り文配置済み）
  pub(super) pages: Vec<model::Page>,
  /// PDF しおり用の見出し情報（文書順）
  pub(super) outline_entries: Vec<OutlineEntry>,
}

/// 本文パス 1 回ぶんの出力（[`break_body_per_page_footnotes`] が反復する単位）。
struct BodyLayout {
  /// 確定した本文ページ列
  pages: Vec<model::Page>,
  /// 目次・しおり用の見出し情報（文書順）
  headings: Vec<typeset::HeadingRecord>,
}

/// 脚注のページ単位採番（`FootnoteNumbering::PerPage`）で本文パスを回す上限回数。
///
/// 1 回目は通し番号で組んで脚注のページ割り当てを知り、2 回目でページ単位番号を反映する。
/// 実質はここで収束するので、残りは番号の変化がページ割り当てを揺らすケース用の余裕。
const MAX_FOOTNOTE_NUMBERING_PASSES: u32 = 4;

/// 脚注のページ単位採番を不動点まで反復して本文ページを確定する。
///
/// ページ単位採番は「番号 → マーカーの桁数 → マーカー幅 → 行分割 → ページ分割 → 脚注のページ
/// 割り当て → 番号」と循環している。`break_pages` はフォント非依存の純粋パスで、ページ確定後に
/// マーカーのグリフを作り直すことはできない（アーキテクチャ上の不変条件）ため、番号を与えて
/// 組み直す反復で解く。
///
/// 各パスは「そのパスで表示した番号」で組まれたページ列を返す。そのページ列から番号を割り当て
/// 直しても同じ番号になれば、表示とページ割り当てが一致した＝不動点なのでそこで止める。
/// 脚注のない文書は 1 回目で（マップが空のまま）収束する。
///
/// 反復が成り立つのは、番号が**表示値しか変えない**から。どの脚注が存在するか・文書順は番号に
/// 依存しないので、出現 index は全パスで同じ脚注を指し続け、マップがパス間で整合する。
///
/// # Errors
///
/// `body_pass` が失敗した場合（lowering・画像サイズ確定のエラー）にそのまま伝播する。
fn break_body_per_page_footnotes(
  body_pass: &impl Fn(Option<&[u32]>) -> miette::Result<BodyLayout>,
) -> miette::Result<BodyLayout> {
  // 1 回目は空マップ＝全脚注が通し番号へフォールバックする（＝ページ割り当てを知るための下見）。
  let mut numbers: Vec<u32> = Vec::new();
  let mut pass: u32 = 1;
  loop {
    let layout = body_pass(Some(&numbers))?;
    let next = typeset::per_page_footnote_numbers(&layout.pages);
    if next == numbers {
      debug!(pass, "脚注のページ単位採番が収束しました");
      return Ok(layout);
    }
    if pass == MAX_FOOTNOTE_NUMBERING_PASSES {
      // 番号の桁数変化がページ割り当てを揺らし続けるケース（脚注が 9 → 10 の桁境界でページ境界に
      // 乗る等）。最後のパスの結果を採用するので、一部のページで番号が 1 から始まらない可能性が
      // ある。黙って出さずに報告する。
      warn!(
        passes = MAX_FOOTNOTE_NUMBERING_PASSES,
        "脚注のページ単位採番が収束しませんでした。最後の組版結果を採用します（一部のページで脚注番号が 1 から始まらない可能性があります）"
      );
      return Ok(layout);
    }
    numbers = next;
    pass += 1;
  }
}

/// 読込済みの設定・スタイル・文献とフォントデータから、描画直前の確定レイアウトを構築する。
///
/// `build_pdf` の描画前パイプライン（ソースのパース → 文献 CSL 整形 → Document IR → `LayoutNode` →
/// `build_blocks` → 画像サイズ確定 → `break_pages` → 走り文配置 → しおり収集）を 1 つの関数に束ねたもの。
/// フォントは `font_data` から内部で `FontRefs` / シェーパー / メトリクスを組み立てて使い、
/// 返り値はフォント非依存の所有データのみ（[`LaidOutDocument`]）。これにより本文組版のロジックを
/// PDF 描画・ファイル I/O から切り離し、確定ページ列を golden テストで直接検証できる。
///
/// # Errors
///
/// ソース読込・パース・文献整形・lowering・フォント検証・段組み幅の不正のいずれかで失敗した場合に
/// エラーを返す。
fn build_pages(
  config: &config::Config,
  style: &config::Style,
  references: &References,
  font_data: &FontData,
) -> miette::Result<LaidOutDocument> {
  // `\cite` のキー存在検証に使う有効な参照 ID 集合（CSL 整形そのものは後続の citation ステージで実施）
  let citation_keys: HashSet<String> = references.keys().cloned().collect();

  let stage_start = Instant::now();
  let mut parsed = parse_all_sources(&config.sources, &citation_keys)?;
  info!(
    source_count = config.sources.len(),
    node_count = parsed.iter().map(|p| return p.nodes.len()).sum::<usize>(),
    elapsed_ms = elapsed_ms(stage_start),
    "全ソースのパースが完了しました"
  );

  // `\cite` を CSL 整形し、引用された文献の書誌を最後の合成グループとして受け取る（parser の後・lowering の前）。
  let stage_start = Instant::now();
  let bibliography = citation::process_citations(parsed.iter_mut().map(|p| return &mut p.nodes), references, style)
    .map_err(|source| return BuildPdfError::Citation { source })?;
  info!(elapsed_ms = elapsed_ms(stage_start), "文献引用の CSL 整形が完了しました");

  // 各ソースファイルを 1 グループとし、書誌を末尾の合成グループとして連結する。グループの並び順が
  // SourceId のインデックスになり、書誌グループの SourceId は parsed.len()（範囲外）になる。
  let groups: Vec<&[DocNode]> = parsed
    .iter()
    .map(|p| return p.nodes.as_slice())
    .chain(std::iter::once(bibliography.as_slice()))
    .collect();

  let font_refs = FontRefs::new(&config.font_configs, font_data)?;

  let stage_start = Instant::now();
  validate_font::validate_fonts(&config.font_configs, &font_refs)?;
  info!(elapsed_ms = elapsed_ms(stage_start), "フォントの検証が完了しました");

  let shaper_datas = ShaperDatas::new(&font_refs);
  let shaper_instances = ShaperInstances::new(&config.font_configs, &font_refs);
  let harf_rust_shapers = HarfRustShapers::new(&config.font_configs, &font_refs, &shaper_datas, &shaper_instances)?;
  debug!("シェーパーの初期化が完了しました");

  let metrics = FontMetrics::new(&font_refs)?;

  // 本文幅は画像サイズ解決と行分割の双方で使うので先に算出する
  let text_width = config.pdf.width - config.pdf.margin.left - config.pdf.margin.right;
  let default_font_size = style.text.font_size;
  let line_height_factor = style.text.line_height_factor;

  // 本文の段組み（前付けは常に単段）。1 段あたりの幅を算出する。config × style の横断制約
  // （段幅が非正にならないこと）は `build_pdf` 冒頭の `config::validate_layout` で検証済み。
  let body_columns = style.columns.count as usize;
  let column_gap = style.columns.gap;
  let body_col_width = typeset::column_width(text_width, body_columns, column_gap);

  // ジオメトリは本文（N 段）・前付け（常に 1 段）・後付け＝索引（独自の段組み数）で分ける。
  let (body_geometry, front_geometry, back_geometry) =
    build_page_geometries(config, style, default_font_size, line_height_factor, body_columns, column_gap);

  // 本文の lowering → シェーピング → 画像確定 → ページ分割を 1 回通す。
  //
  // `footnote_numbers` は脚注の表示番号の上書きマップ（出現 index 引き）。通し採番では `None` を
  // 渡し、上書きマップを一切通さない現状どおりの経路になる。ページ単位採番のときだけ
  // [`break_body_per_page_footnotes`] がページ確定後の番号を与えて複数回呼ぶ。
  let run_body_pass = |footnote_numbers: Option<&[u32]>| -> miette::Result<BodyLayout> {
    let stage_start = Instant::now();
    let mut lowering_ctx =
      LoweringContext::new(style).with_image_defaults(config.image.max_dpi, config.image.downsample);
    if let Some(numbers) = footnote_numbers {
      lowering_ctx = lowering_ctx.with_footnote_numbers(numbers);
    }
    let (body_layout_nodes, headings) = typeset::lower_sources_with_headings(&lowering_ctx, &groups)
      .map_err(|error| return wrap_lowering_error(error, &parsed))?;
    info!(elapsed_ms = elapsed_ms(stage_start), "Document IR → LayoutNode への変換が完了しました");

    // build_blocks は本文・タイトルページで複数回呼ばれ、自段完了を同じ文面の DEBUG で出すため、
    // span の `region` で呼び出し区間を区別できるようにする（INFO 時は span 非活性でゼロコスト）。
    let stage_start = Instant::now();
    let body_blocks = {
      let _span = debug_span!("build_blocks", region = "body").entered();
      typeset::build_blocks(
        body_layout_nodes,
        &harf_rust_shapers,
        &metrics,
        default_font_size,
        line_height_factor,
        config.document.language.as_deref(),
        style.text.punctuation_spacing,
      )
    };
    info!(
      block_count = body_blocks.len(),
      elapsed_ms = elapsed_ms(stage_start),
      "本文ブロックの構築が完了しました"
    );

    let stage_start = Instant::now();
    // 本文画像は段幅に合わせて解決する（段抜き＝全幅フロートは将来検討）。
    let body_blocks = pdf_gen::resolve_images(body_blocks, body_col_width.to_pt())?;
    info!(elapsed_ms = elapsed_ms(stage_start), "画像サイズの確定が完了しました");

    // 本文をページ分割する。各見出しの本文内ページ index もここから採取する。本文は前付け
    // （タイトルページ・目次）と別系列で 1 から番号付けするため、得られる本文内ページ番号が
    // 最終値になる（前付けの長さに不依存 = R1。break_pages は純粋）。
    let stage_start = Instant::now();
    let pages = {
      let _span = debug_span!("break_pages", region = "body").entered();
      typeset::break_pages(body_blocks, text_width, &body_geometry, &typeset::KnuthPlassBreaker, style.text.alignment)
    };
    info!(
      body_page_count = pages.len(),
      elapsed_ms = elapsed_ms(stage_start),
      "本文のページ分割が完了しました"
    );
    return Ok(BodyLayout { pages, headings });
  };

  // 脚注の採番方式で本文パスの回し方が変わる（他は一切変わらない）。
  let BodyLayout {
    pages: mut body_pages,
    headings,
  } = match style.footnote.numbering {
    // 通し採番: 1 回だけ通す。番号はページに依存しないので反復する理由がない。
    config::FootnoteNumbering::Continuous => run_body_pass(None)?,
    config::FootnoteNumbering::PerPage => break_body_per_page_footnotes(&run_body_pass)?,
  };
  let body_page_count = body_pages.len();
  let body_page_values = BodyPageValues::from_body_pages(&body_pages, &style.page_numbering);
  info!(body_page_count, elapsed_ms = elapsed_ms(stage_start), "本文のページ分割が完了しました");

  // 前付けブロック（タイトルページ → 目次）を組み立てる。各リージョンは改ページ境界で始まる。
  // タイトルページのメタデータは config 形状から疎結合にするため本体で構築して渡す。
  let title_metadata = typeset::TitlePageMetadata {
    title: config.document.title.clone(),
    author: config.document.author.clone(),
    date: config.document.date.clone(),
  };
  let front_blocks = assemble_front_matter(
    &headings,
    &body_page_values,
    &title_metadata,
    style,
    &harf_rust_shapers,
    &metrics,
    text_width,
  );

  // 前付け（1 段）を本文（N 段）と別に分割し、ページ列として連結する。前付けと本文は段数が異なるため
  // 1 回の break_pages では兼ねられない。連結することで本文ページは後ろの index へ自動的にずれ、内部
  // リンク・しおりの参照ページもレンダリング時の列挙で正しく解決される。
  let stage_start = Instant::now();
  let front_pages = {
    let _span = debug_span!("break_pages", region = "front").entered();
    break_front_matter(front_blocks, text_width, &front_geometry, &typeset::KnuthPlassBreaker, style.text.alignment)
  };
  let front_matter_count = front_pages.len();

  // 後付け（索引）ブロックを組み立てる。本文の index_entries から全ページの索引語を集約し、
  // 出現ページへ内部リンクの到達先アンカーを事後追加する（`body_pages` の破壊的更新）。
  // `\index` が 1 個もなければ空ページ列になる。
  let back_blocks = assemble_back_matter(&mut body_pages, &body_page_values, style, &harf_rust_shapers, &metrics);
  let back_pages = {
    let _span = debug_span!("break_pages", region = "back").entered();
    break_back_matter(back_blocks, text_width, &back_geometry, &typeset::KnuthPlassBreaker, style.text.alignment)
  };
  let back_matter_count = back_pages.len();

  // 索引ページも本文からの通し番号（独立した番号体系を持たない）。前付けページ列が確定した時点
  // （`pages` への move の前）でラベルを解決する。
  let page_labels = body_page_values.with_back_matter(&back_pages).finalize(&front_pages);
  let mut pages = front_pages;
  pages.extend(body_pages);
  pages.extend(back_pages);
  debug_assert_eq!(page_labels.len(), pages.len(), "ラベル数は物理ページ総数と一致するはず");
  info!(
    page_count = pages.len(),
    front_matter_count,
    body_page_count,
    back_matter_count,
    elapsed_ms = elapsed_ms(stage_start),
    "ページ分割が完了しました"
  );

  // ページ数確定後にヘッダー・フッターを配置する（ページ番号トークンの解決にラベルが必要なため）
  let page_height = config.pdf.height;
  let running_spec = build_running_spec(style, &config.document, text_width, page_height, page_labels);
  typeset::build_running_content(&mut pages, &harf_rust_shapers, &metrics, &running_spec);

  // PDF しおり用の見出し情報を文書順に集める（CSL 整形で追加された References 見出しも含む）。
  // lowering が各見出しの直前に出すアンカーと文書順で 1 対 1 に対応する。
  let outline_entries = collect_outline_entries(&headings);

  return Ok(LaidOutDocument {
    pages,
    outline_entries,
  });
}

/// ステージ開始時刻からの経過ミリ秒を返す（INFO サマリの `elapsed_ms` 用）。
///
/// ビルド処理時間が `u64::MAX` ms（約 5 億年）を超えることはない前提。
#[allow(clippy::cast_possible_truncation)]
fn elapsed_ms(start: Instant) -> u64 { return start.elapsed().as_millis() as u64; }

/// 1 ソースファイルのパース結果と、そのファイル名・内容（診断用）。
///
/// `parse_all_sources` が平坦化せずファイルごとに 1 件生成する。`nodes` の並び順（`Vec<ParsedSource>`
/// のインデックス）が [`typeset::SourceId`] に一致し、lowering エラー発生時に `name` / `content` を
/// `NamedSource` へ逆引きしてファイル名・スニペット付きの診断を表示するのに使う。
struct ParsedSource {
  /// 表示用のソースパス文字列（`NamedSource` の名前になる）
  name: String,
  /// ソースファイルの元テキスト全体（`NamedSource` のスニペット元になる）
  content: String,
  /// パース・評価済みの Document IR ノード列
  nodes: Vec<DocNode>,
}

/// 全 source を順次読み込み、ファイルごとに [`ParsedSource`] を生成して返す。
///
/// 旧実装のように 1 つの `Vec<DocNode>` へ平坦化せず、どの `DocNode` がどのソース由来かを
/// 保持する（lowering エラーのソース帰属に必要）。I/O 失敗は早期にエラーを返し、
/// パース・評価エラーは全 source で集約して [`BuildPdfError::MultipleSourceErrors`] にまとめて返す。
// BuildPdfError は診断用の NamedSource を同梱するため大きい。ソース位置付き診断を優先する方針で、
// frontend::parse_source と同じく result_large_err を許可する（Err は稀な失敗時のみ構築される）。
#[allow(clippy::result_large_err)]
fn parse_all_sources(
  sources: &[std::path::PathBuf],
  citation_keys: &HashSet<String>,
) -> Result<Vec<ParsedSource>, BuildPdfError> {
  let mut parsed: Vec<ParsedSource> = Vec::new();
  let mut parse_errors: Vec<ParseSourceError> = Vec::new();

  for source_path in sources {
    let content = std::fs::read_to_string(source_path).map_err(|source| {
      return BuildPdfError::ReadTextFile {
        path: source_path.display().to_string(),
        source,
      };
    })?;
    let display_path = source_path.display().to_string();
    match frontend::parse_source(&content, &display_path, citation_keys) {
      Ok(nodes) => parsed.push(ParsedSource {
        name: display_path,
        content,
        nodes,
      }),
      Err(error) => parse_errors.push(error),
    }
  }

  if !parse_errors.is_empty() {
    return Err(BuildPdfError::MultipleSourceErrors {
      errors: parse_errors,
    });
  }
  return Ok(parsed);
}

/// `LoweringError` を、`source_id()` で特定できるソースファイルに `NamedSource` を紐付けて
/// [`BuildPdfError`] に変換する。
///
/// `source_id()` が `parsed` の範囲内を指す場合はそのファイル名・内容を `NamedSource` に載せた
/// [`BuildPdfError::Lowering`] にし、パースエラーと同じくファイル名・スニペット付きで診断できるようにする。
/// 範囲外（= 合成された書誌グループ、`SourceId` が `parsed.len()`）を指す場合は帰属元を特定できないため
/// [`BuildPdfError::LoweringInternal`] にフォールバックする。span は各ファイル内のオフセットのまま
/// 使える（各 `NamedSource` はその 1 ファイル分の `content` だけを持つため、グローバル変換は不要）。
fn wrap_lowering_error(error: typeset::LoweringError, parsed: &[ParsedSource]) -> BuildPdfError {
  let index = error.source_id().index();
  return match parsed.get(index) {
    Some(source) => BuildPdfError::Lowering {
      src: miette::NamedSource::new(&source.name, source.content.clone()),
      source: error,
    },
    None => BuildPdfError::LoweringInternal { source: error },
  };
}

/// 本文（N 段）・前付け（常に 1 段）・後付け（索引、独自の段組み数）の [`typeset::PageGeometry`] を
/// 組み立てる。
///
/// いずれも段数・段間以外を共有するため、本文側を組んでから前付け・後付けはそれぞれ差し替える。
fn build_page_geometries(
  config: &config::Config,
  style: &config::Style,
  default_font_size: model::Length,
  line_height_factor: f32,
  body_columns: usize,
  column_gap: model::Length,
) -> (typeset::PageGeometry, typeset::PageGeometry, typeset::PageGeometry) {
  let body_geometry = typeset::PageGeometry {
    margin_top: config.pdf.margin.top,
    page_limit: config.pdf.height - config.pdf.margin.bottom,
    default_font_size,
    line_height_factor,
    table_cell_padding: style.table.cell_padding,
    num_columns: body_columns,
    column_gap,
    flush_bottom: style.page.flush_bottom,
    footnote_top_margin: style.footnote.top_margin,
    footnote_rule_length: style.footnote.rule_length,
    footnote_rule_thickness: style.footnote.rule_thickness,
    footnote_rule_color: style.footnote.rule_color.map(model::Color::rgb),
    footnote_rule_gap: style.footnote.rule_gap,
  };
  // 前付け（タイトルページ・目次）は下端揃えの対象外。struct-update で本文値を継ぐため明示的に落とす。
  let front_geometry = typeset::PageGeometry {
    num_columns: 1,
    column_gap: model::Length::ZERO,
    flush_bottom: false,
    ..body_geometry
  };
  // 後付け（索引）は本文とは独立の段組み数を持つ（style.index.column_count）。段間は本文と共通。
  // 前付けと同様、下端揃えの対象外。
  let back_geometry = typeset::PageGeometry {
    num_columns: usize::from(style.index.column_count),
    flush_bottom: false,
    ..body_geometry
  };
  return (body_geometry, front_geometry, back_geometry);
}

#[cfg(test)]
mod tests {
  use std::cell::RefCell;

  use super::{
    BodyLayout, BuildPdfError, MAX_FOOTNOTE_NUMBERING_PASSES, ParsedSource, break_body_per_page_footnotes,
    wrap_lowering_error,
  };

  /// 指定した出現 index の脚注だけを持つ 1 ページを作るテストヘルパ
  fn page_with_footnotes(indices: &[u32]) -> model::Page {
    return model::Page {
      blocks: Vec::new(),
      header: Vec::new(),
      footer: Vec::new(),
      footnotes: indices
        .iter()
        .map(|index| {
          return model::PlacedFootnote {
            number: index + 1,
            index: *index,
            continued: false,
            blocks: Vec::new(),
          };
        })
        .collect(),
      anchors: Vec::new(),
      links: Vec::new(),
      index_entries: Vec::new(),
    };
  }

  #[test]
  fn per_page_footnote_passes_stop_at_fixed_point() {
    // Arrange — ページ割り当てが番号に依らず安定している本文パスを模す（実文書の通常ケース）。
    // 1 ページ目に 2 個・2 ページ目に 1 個で固定。
    let calls = RefCell::new(0_u32);
    let body_pass = |_numbers: Option<&[u32]>| {
      *calls.borrow_mut() += 1;
      return Ok(BodyLayout {
        pages: vec![page_with_footnotes(&[0, 1]), page_with_footnotes(&[2])],
        headings: Vec::new(),
      });
    };

    // Act
    let layout = break_body_per_page_footnotes(&body_pass).expect("失敗しない");

    // Assert — 1 回目（通し番号）でページ割り当てを知り、2 回目でページ単位番号を反映して組み直す。
    // 2 回目の結果から番号を割り当て直しても同じマップになる＝不動点なのでそこで止まる。
    assert_eq!(*calls.borrow(), 2, "実質 2 回で収束するはず");
    assert_eq!(layout.pages.len(), 2);
  }

  #[test]
  fn per_page_footnote_passes_give_up_at_max_and_keep_last_layout() {
    // Arrange — 番号を与えるたびにページ割り当てが変わり続けて収束しない本文パスを模す
    // （脚注が桁境界でページ境界に乗り続けるケースの極端版）。呼ばれるたびに脚注の配置を
    // 1 ページ目・2 ページ目で交互に入れ替える。
    let calls = RefCell::new(0_u32);
    let body_pass = |_numbers: Option<&[u32]>| {
      let call = {
        let mut c = calls.borrow_mut();
        *c += 1;
        *c
      };
      let pages = if call % 2 == 0 {
        vec![page_with_footnotes(&[0]), page_with_footnotes(&[1])]
      } else {
        vec![page_with_footnotes(&[0, 1]), page_with_footnotes(&[])]
      };
      return Ok(BodyLayout {
        pages,
        headings: Vec::new(),
      });
    };

    // Act
    let layout = break_body_per_page_footnotes(&body_pass).expect("収束しなくてもエラーにはしない");

    // Assert — 上限回数で打ち切り（無限ループしない）、最後のパスの結果を返す。
    // 上限回（4）は偶数なので、最後は各ページ 1 個ずつの側（奇数回の 2 個・0 個ではない）。
    assert_eq!(*calls.borrow(), MAX_FOOTNOTE_NUMBERING_PASSES, "上限回数で打ち切るはず");
    assert_eq!(layout.pages[0].footnotes.len(), 1, "最後のパスのレイアウトを返すはず");
    assert_eq!(layout.pages[1].footnotes.len(), 1, "最後のパスのレイアウトを返すはず");
  }

  /// index=1 のグループに未定義ラベルの `\ref` を含む 2 グループを作り、`source_id()==1` の
  /// `LoweringError` を生成するテストヘルパ
  fn lowering_error_with_source_id_1(style: &config::Style) -> typeset::LoweringError {
    use model::{DocNode, InlineNode};
    let ctx = typeset::LoweringContext::new(style);
    let g0 = vec![DocNode::Paragraph(vec![InlineNode::Text(
      "plain".to_string(),
    )])];
    let g1 = vec![DocNode::Paragraph(vec![InlineNode::Ref {
      label: "missing".to_string(),
      span: model::Span::DUMMY,
    }])];
    let error = typeset::lower_sources_with_headings(&ctx, &[g0.as_slice(), g1.as_slice()])
      .expect_err("未定義ラベルはエラーになるはず");
    assert_eq!(error.source_id().index(), 1, "グループ 1 の \\ref が帰属源のはず");
    return error;
  }

  #[test]
  fn lowering_error_attributes_named_source_by_source_id() {
    // Arrange — source_id()==1 の LoweringError を生成する（範囲内・範囲外の両方に流し込む）
    let style = config::Style::default();

    // Act / Assert 1 — parsed が 2 要素なら index=1 の 2 番目のファイルに NamedSource が紐づく
    let parsed = vec![
      ParsedSource {
        name: "a.sei".to_string(),
        content: "A".to_string(),
        nodes: Vec::new(),
      },
      ParsedSource {
        name: "b.sei".to_string(),
        content: "B content".to_string(),
        nodes: Vec::new(),
      },
    ];
    match wrap_lowering_error(lowering_error_with_source_id_1(&style), &parsed) {
      BuildPdfError::Lowering { src, .. } => {
        assert_eq!(src.name(), "b.sei", "source_id=1 は 2 番目のファイルに帰属するはず");
      },
      other => panic!("Lowering が期待されます: {other:?}"),
    }

    // Act / Assert 2 — parsed が 1 要素だけなら index=1 は範囲外で LoweringInternal にフォールバックする
    let parsed_short = vec![ParsedSource {
      name: "only.sei".to_string(),
      content: "X".to_string(),
      nodes: Vec::new(),
    }];
    assert!(
      matches!(
        wrap_lowering_error(lowering_error_with_source_id_1(&style), &parsed_short),
        BuildPdfError::LoweringInternal { .. }
      ),
      "範囲外の SourceId は LoweringInternal になるはず"
    );
  }
}
