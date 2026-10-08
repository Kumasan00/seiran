//! 確定レイアウトの golden スナップショット回帰テスト
//!
//! fixture を組版した決定的テキストを `tests/golden/` と比較する。再生成は
//! `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で行う。検証手段の選択（layout dump golden と
//! PDF バイト比較の使い分け）は `.claude/skills/verify-typesetting/SKILL.md` が規定し、
//! 本モジュール内部のテスト分類はこの doc が正典。
//!
//! 入力は例外なく [`crate::compiler::test_support::TestProject`] が組み立て、production と同じ
//! `input::load` → frontend → semantics → typeset を通る。
//!
//! # テストの分類
//!
//! golden ファイル（`tests/golden/<name>.txt`）と実際に比較するのは主入口
//! [`layout_dumps_match_golden`]（[`GOLDEN_INPUTS`] 全 fixture の回帰）だけである。これは公開 facade
//! `compile()` → `compiler::dump::dump_publication`（`publication::Publication` の決定的テキスト
//! ダンプ）を通す。ダンプは確定座標のテキスト表現であり krilla の描画は含まないが、`Publication` の
//! メタデータ・リンク・しおりまで含むため `dump_pages`（`typeset::dump` が所有）よりカバー範囲が広い。
//!
//! 残りのテストは golden ファイルを一切読み書きせず、`Publication` へ変換すると失われる情報
//! （anchor・索引語のページ帰属・脚注 fragment の繰越と番号・`PlacedBlock` の幾何）を見るため
//! `TestProject::layout` を使うか、`Publication` のグリフ列を直接見る。
//!
//! - **2 つの `typeset::Page` ダンプをテスト内で直接比較**（`assert_eq!` / `assert_ne!`）:
//!   [`index_marks_are_invisible_to_layout`]・style 差分 2 種
//!   [`layout_dump_changes_with_line_height`] / [`layout_dump_changes_with_punctuation_spacing`]・
//!   [`blank_code_line_keeps_a_full_line_height`]・
//!   [`text_alignment_leaves_kind_specific_alignment_untouched`]（本文の揃えを変えた組版と既定の組版の水平位置）・
//!   寄せ環境 3 種 [`flush_environment_matches_text_alignment_of_the_same_direction`] /
//!   [`nested_flush_environment_uses_innermost_direction_and_reverts_after_it`] /
//!   [`empty_flush_environment_adds_no_vertical_space`]（寄せ環境で包んだ組版と、包まない・`[text].alignment` を変えた組版）
//! - **`Page` / `PlacedBlock` へ直接アサート**（ダンプ関数は通らない）:
//!   [`keep_with_next_prevents_heading_orphan_end_to_end`]・
//!   [`index_group_heading_never_ends_a_column`]・
//!   脚注ページ単位採番 2 種 [`per_page_footnote_numbering_restarts_on_each_page`] /
//!   [`continuous_footnote_numbering_runs_through_pages`]（共通ヘルパ [`footnote_numbers_per_page`]
//!   経由）・[`index_entries_follow_the_page_the_content_lands_on`]・
//!   [`footnote_links_follow_the_page_the_line_lands_on`]・
//!   [`long_footnote_splits_across_pages_without_overlapping_body`]・
//!   [`figure_images_resolve_to_expected_display_sizes`]・
//!   [`figure_image_without_size_fits_two_column_width_not_text_width`]・
//!   [`front_matter_adds_no_blank_pages`]（前付けの構成ごとの総ページ数）
//! - **`Publication` のグリフ列へ直接アサート**（`compile` を通す。グリフ範囲はダンプに出ない）:
//!   クラスタ範囲がテキストを過不足なく覆うことの検査 3 種 [`glyph_ranges_tile_text_with_combining_marks`] /
//!   [`glyph_ranges_tile_text_in_right_to_left_runs`] / [`glyph_ranges_tile_text_in_japanese_clusters`]
//!   （共通ヘルパ [`assert_ranges_tile_text`] 経由）
//! - **区切り括弧の伸縮**（フォントサイズを変えず MATH の size variant / glyph assembly で縦にだけ伸ばし、インクの縦中央を
//!   数式軸へ）: [`delimiters_center_on_the_math_axis_at_the_body_font_size`] /
//!   [`delimiters_cover_the_grid_and_stay_inside_the_block`] / [`tall_delimiters_grow_only_vertically`] /
//!   [`assembled_delimiter_is_one_character_of_text`] /
//!   [`assembled_delimiter_parts_stack_bottom_to_top_with_the_width_on_the_last_part`]（共通ヘルパ [`measure_delimited_block`]・[`stix_run_ink`] 経由）
//! - **数式のスクリプト段**: [`script_levels_use_math_scale_down_and_ssty_glyphs`]（縮小率は MATH、字形は `ssty`）
//! - **数式のスクリプト配置**（MATH 定数とインクからのシフト量・上下付きの列・スクリプト後のアキ）:
//!   [`superscript_on_a_short_base_sits_at_the_font_shift`] / [`subscript_on_a_short_base_sits_at_the_font_shift`] /
//!   [`tall_base_pushes_the_superscript_up`] / [`stacked_scripts_differ_by_the_italic_correction_in_either_order`] /
//!   [`space_after_script_follows_the_scripts`] / [`script_in_a_non_math_font_is_placed_by_its_ink`]
//! - **数式のスクリプトの横位置**（イタリック補正・math kern）:
//!   送り幅への補正 [`slanted_glyph_before_an_operator_gets_its_italic_correction`] /
//!   [`italic_correction_is_added_only_before_an_upright_glyph`] / [`glyph_without_italic_correction_keeps_its_advance`] /
//!   [`operator_keeps_its_advance_despite_its_italic_correction`] /
//!   [`script_content_gets_italic_correction_through_the_atom_path`]・
//!   スクリプトの位置 [`subscript_on_a_slanted_base_sits_at_its_advance`] /
//!   [`large_operator_pulls_its_subscript_back_by_the_italic_correction`] /
//!   [`scripts_on_an_upright_base_share_a_column`] / [`scripts_on_an_empty_base_share_a_column`] /
//!   [`scripts_on_a_non_math_base_share_a_column`] / [`base_ending_with_scripts_takes_no_italic_correction`] /
//!   [`cursor_after_scripts_follows_the_farther_script`]・
//!   math kern [`subscript_cuts_in_under_a_base_with_a_bottom_right_kern`] /
//!   [`superscript_moves_by_the_top_right_kern_of_the_base`] / [`subscript_kern_uses_the_top_left_table_of_the_script_glyph`]
//! - **分数**（MATH の `Fraction*` 定数とインクからのシフト量・数式軸上の横罫・段の遷移・左右のアキ）:
//!   [`inline_fraction_stacks_script_size_parts_around_a_rule_on_the_math_axis`] /
//!   [`display_fraction_uses_display_style_constants_and_text_size_parts`] /
//!   [`fraction_in_a_display_superscript_uses_text_constants_at_script_size`] /
//!   [`nested_fraction_draws_a_thinner_inner_rule`] / [`adjacent_fraction_rules_do_not_touch`] /
//!   [`superscript_in_a_denominator_uses_the_cramped_shift`] / [`fraction_rule_is_painted_as_a_filled_rect`] /
//!   [`empty_fraction_compiles_with_a_zero_width_rule`]（共通ヘルパ [`first_line_parts`]・[`display_parts`]・[`collect_rules`] 経由）
//! - **根号**（根号記号の伸縮・横線・ギャップの調整・指数の kern と高さ）:
//!   [`radical_vinculum_continues_the_top_of_the_surd_over_the_radicand`] /
//!   [`tall_radicand_stretches_the_surd_to_cover_it`] / [`display_radical_uses_the_display_gap`] /
//!   [`narrow_degree_sits_right_above_the_surd`] / [`wide_degree_pushes_the_surd_right_by_the_kerns`] /
//!   [`degree_bottom_rises_by_the_percent_of_the_radical_height`] / [`empty_radical_compiles_with_a_zero_width_vinculum`]
//! - **大型演算子**（display 段で `DisplayOperatorMinHeight` 以上の字形へ伸ばして数式軸に合わせる・limits を取る演算子の
//!   範囲を上下に積む・text 段は不変）:
//!   [`display_large_operator_grows_past_the_text_glyph_and_centers_on_the_math_axis`] /
//!   [`inline_large_operator_keeps_the_text_glyph_on_the_baseline`] /
//!   [`display_integral_pulls_its_subscript_back_by_the_display_glyph_correction`] /
//!   [`display_sum_stacks_its_limits_centered_above_and_below`] /
//!   [`wide_lower_limit_widens_the_operator_and_pushes_the_next_atom`] /
//!   [`empty_limits_take_no_width_beyond_the_operator`] / [`inline_sum_keeps_its_limits_at_the_shoulder`]
//! - **テストヘルパが入力読込を迂回していないことの検査**:
//!   [`layout_helper_reports_cross_input_layout_validation`]
//!
//! # カバレッジの注意
//!
//! 前付け（タイトルページ / 目次）・running content（ヘッダ / フッタ）・段組みは既定 config では
//! 無効で、fixture 名ごとの差分（`test_support` の `golden_fixture`）が有効化する
//! （例: `toc` / `title_page` / `footnote_columns`）。これらの経路を触ったら、該当 fixture が
//! その機能を実際に通していることを確認する。
//!
//! # 新機能に golden テストを足す
//!
//! 1. `tests/text/<name>.sei` に機能を exercise する入力を追加する
//! 2. [`GOLDEN_INPUTS`] に名前を登録する。機能が既定で無効なら `test_support` の
//!    fixture 差分（style / config）に有効化を追記する（config 差分は生 TOML の 1 系統だけ）
//! 3. `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で golden を生成し、内容を確認してコミットする
//!
//! 外部ファイルに依存する入力は対象外（前例: `figure.sei` は画像実体にレイアウトが依存するため除外）。

use std::{
  fs, iter,
  ops::Range,
  path::{Path, PathBuf},
};

use harfrust::Font;
use read_fonts::{
  FontRef, TableProvider,
  tables::math::{MathConstant, MathKernCorner},
  types::GlyphId,
};

use crate::{
  compiler::{
    dump,
    test_support::{self, FIGURE_IMAGE_ASSETS, TestProject},
  },
  length::Length,
  project::FontType,
  publication::{GlyphRun, PaintOp},
  typeset::{AnchorId, HBoxContent, LinkTarget, Page, PlacedBlock, dump_pages},
};

/// golden 比較対象の入力名。
const GOLDEN_INPUTS: &[&str] = &[
  "align",
  "cases",
  "cite",
  "code",
  "color",
  "equation",
  "flush",
  "footnote",
  "footnote_columns",
  "footnote_per_page",
  "footnote_split",
  "gather",
  "hyperref",
  "hyphenation",
  "index",
  "index_groups",
  "index_ranges",
  "index_right",
  "index_split",
  "itemize",
  "justify",
  "math_break",
  "math_frac",
  "math_limits",
  "math_script",
  "math_spacing",
  "matrix",
  "multiline",
  "pagebreak",
  "quote",
  "ref",
  "split",
  "table",
  "table_break",
  "text",
  "text_center",
  "text_right",
  "theorem",
  "title_page",
  "toc",
  "toc_center",
  "yakumono",
];

/// golden ファイルを置くディレクトリ（`crates/seiran-compiler/tests/golden`）を返す。
fn golden_dir() -> PathBuf { return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"); }

/// `compile()` を入口として fixture を組版し、`Publication` のダンプを返す。
fn dump_publication_of(name: &str) -> String {
  let compilation = TestProject::builder()
    .golden_fixture(name)
    .build()
    .compile()
    .unwrap_or_else(|failure| panic!("fixture {name} の compile は成功するはず: {:?}", failure.into_report()));
  return dump::dump_publication(&compilation.publication);
}

/// fixture を組版し、確定ページ列（`typeset::Page`）のダンプを返す。
fn dump_pages_of(name: &str) -> String {
  return dump_pages(&TestProject::builder().golden_fixture(name).build().laid_out().pages);
}

#[test]
fn layout_dumps_match_golden() {
  let update = std::env::var_os("UPDATE_GOLDEN").is_some();
  if update {
    fs::create_dir_all(golden_dir()).expect("golden ディレクトリの作成");
  }

  let mut mismatches = Vec::new();
  for name in GOLDEN_INPUTS {
    let dump = dump_publication_of(name);
    let golden_path = golden_dir().join(format!("{name}.txt"));
    if update {
      fs::write(&golden_path, &dump).expect("golden の書き出し");
    } else {
      let expected = fs::read_to_string(&golden_path).unwrap_or_else(|error| {
        panic!("golden が未生成です: {} ({error})。UPDATE_GOLDEN=1 で生成してください", golden_path.display())
      });
      if dump != expected {
        mismatches.push(*name);
      }
    }
  }

  assert!(
    mismatches.is_empty(),
    "レイアウトダンプが golden と一致しません: {mismatches:?}（意図した変更なら UPDATE_GOLDEN=1 で再生成し git diff で確認）"
  );
}

/// 組版中間表現を取り出すテストヘルパが `input::load` の横断検証を迂回していないことの検査。
///
/// 余白の合計が用紙幅を超える config × style は `typeset::PreparedGeometry::prepare`（config と style の
/// 両方を要求する横断検証）でしか検出できない。`TestProject::layout` が `input::load` を迂回すると、
/// この診断が出なくなって失敗する。
#[test]
fn layout_helper_reports_cross_input_layout_validation() {
  // 左右余白の合計（600mm）が fixture の用紙幅（595mm）を超える
  let project = TestProject::builder()
    .golden_fixture("text")
    .style_toml(|table| {
      test_support::set(table, "page", "margin_left", "300mm");
      test_support::set(table, "page", "margin_right", "300mm");
    })
    .build();

  let Err(failure) = project.layout() else {
    panic!("横断検証に失敗するはず");
  };

  // 段名 wrapper ではなく leaf の診断がそのまま出る
  let codes: Vec<String> = failure
    .diagnostics()
    .map(|diagnostic| return diagnostic.code().expect("leaf 診断は code を持つはず").to_string())
    .collect();
  assert_eq!(codes, vec!["typeset::geometry::horizontal_margins".to_string()]);
}

/// 索引マーカーを除けば本文レイアウトが変わらないことを確認する。
#[test]
fn index_marks_are_invisible_to_layout() {
  let with_index = dump_pages_of("index");
  let without_index = dump_pages_of("index_baseline");
  let body_page_count = without_index.lines().filter(|line| return line.starts_with("=== page ")).count();
  let mut page_index: isize = -1;
  let stripped_lines: Vec<&str> = with_index
    .lines()
    .filter(|line| {
      if line.starts_with("=== page ") {
        page_index += 1;
      }
      return usize::try_from(page_index).is_ok_and(|index| return index < body_page_count)
        && !line.starts_with("index ")
        && !line.starts_with("anchor id=\"index-page:");
    })
    .collect();
  let stripped = stripped_lines.iter().fold(String::new(), |mut acc, line| {
    acc.push_str(line);
    acc.push('\n');
    return acc;
  });

  assert_eq!(stripped, without_index, "\\index の有無で本文のレイアウトが変わってはならない");
}

/// ページ末尾のブロックが見出し行かを返す。
fn page_ends_with_heading(page: &Page) -> bool {
  let Some(PlacedBlock::Line { line, baseline_y }) = page.blocks.last() else {
    return false;
  };
  let top = *baseline_y - line.height;
  return page.anchors.iter().any(|anchor| {
    return matches!(anchor.id, AnchorId::Heading(_)) && (anchor.y - top).abs() < Length::pt(0.5);
  });
}

#[test]
fn keep_with_next_prevents_heading_orphan_end_to_end() {
  // 版面を小さくして見出しがページ境界に当たりやすくする（見出し + 本文数行は空ページに
  // 収まる大きさ）。keepwithnext.sei は見出し直前を filler で埋め、見出しがページ末尾に来る配置。
  let project = TestProject::builder()
    .sources(&["tests/text/keepwithnext.sei"])
    .config_toml(|table| test_support::set(table, "pdf", "height", "45mm"))
    .style_toml(|table| {
      test_support::set(table, "page", "margin_top", "10mm");
      test_support::set(table, "page", "margin_bottom", "10mm");
    })
    .build();

  let laid_out = project.laid_out();

  assert!(laid_out.pages.len() >= 2, "複数ページに分かれるはず: {} ページ", laid_out.pages.len());
  for (index, page) in laid_out.pages.iter().enumerate() {
    assert!(!page_ends_with_heading(page), "page {index} が見出しで終わっている（孤立）: {:#?}", page.blocks);
  }
}

/// 索引ページ（エントリのページ番号が内部リンクを張っているページ）かを返す。
///
/// 本文ページと区別するための判定。区分見出しの検査を索引ページに限ることで、たまたま同じ文字列の
/// 本文行を見出しと取り違えない。
fn is_index_page(page: &Page) -> bool {
  return page
    .links
    .iter()
    .any(|link| return matches!(link.target, LinkTarget::Internal(AnchorId::IndexPage(_))));
}

/// 行が索引の区分見出し（単独のラベル文字列だけの行）ならそのラベルを返す。
///
/// 区分ラベルの固定表は `typeset::pagination::index::grouping` が持つ。ここでは `index_groups.sei` が実際に
/// 生む見出しだけを見れば足りるので、判定は「1 グリフ列だけの行で、その文字列がラベルと一致する」形にする。
fn index_group_heading_label(block: &PlacedBlock) -> Option<String> {
  const LABELS: &[&str] = &[
    "A", "B", "C", "Z", "あ", "か", "さ", "た", "な", "は", "ま", "や", "ら", "わ", "Others",
  ];
  let PlacedBlock::Line { line, .. } = block else {
    return None;
  };
  let [single] = line.boxes.as_slice() else {
    return None;
  };
  let HBoxContent::Glyphs(run) = &single.hbox.content else {
    return None;
  };
  return LABELS.contains(&run.text.as_str()).then(|| return run.text.clone());
}

#[test]
fn index_group_heading_never_ends_a_column() {
  // 索引が複数の段・ページへ分かれる小さな版面にする（段組みは style.index.column_count = 2）。
  // 用紙高さだけを縮め、幅は既定のまま（幅を詰めると本文が 1 行 1 文字になり、見出しと同じ文字列の
  // 本文行が生まれてしまう）。
  let project = TestProject::builder()
    .sources(&["tests/text/index_groups.sei"])
    .config_toml(|table| test_support::set(table, "pdf", "height", "60mm"))
    .style_toml(|table| {
      test_support::set(table, "page", "margin_top", "10mm");
      test_support::set(table, "page", "margin_bottom", "10mm");
      test_support::set(table, "index", "group_headings", true);
    })
    .build();

  let laid_out = project.laid_out();

  // 見出し行の直後には必ず同じ段の中に次の行が来る（段が変わると baseline_y が上へ戻る）
  let mut heading_count = 0usize;
  for (page_index, page) in laid_out.pages.iter().enumerate().filter(|(_, page)| return is_index_page(page)) {
    for (block_index, block) in page.blocks.iter().enumerate() {
      let Some(label) = index_group_heading_label(block) else {
        continue;
      };
      let PlacedBlock::Line { baseline_y, .. } = block else {
        unreachable!("index_group_heading_label が Some を返すのは PlacedBlock::Line のときだけ");
      };
      heading_count += 1;
      let next = page.blocks.get(block_index + 1);
      let follows_in_same_column = matches!(
        next,
        Some(PlacedBlock::Line {
          baseline_y: next_baseline,
          ..
        }) if *next_baseline > *baseline_y
      );
      assert!(
        follows_in_same_column,
        "page {page_index} の区分見出し {label} が段末・ページ末に孤立している: {next:#?}"
      );
    }
  }
  assert!(heading_count >= 5, "区分見出しが十分に出ているはず: {heading_count} 個");
  assert!(laid_out.pages.len() >= 2, "索引が複数ページへ分かれるはず: {} ページ", laid_out.pages.len());
}

/// `figure.sei` の画像の確定描画寸法を文書順に集める。
fn figure_image_sizes() -> Vec<(Length, Length)> {
  let project = TestProject::builder().sources(&["tests/text/figure.sei"]).assets(FIGURE_IMAGE_ASSETS).build();
  let layout = project.layout().expect("figure.sei は組版できるはず");
  return layout
    .pages
    .iter()
    .flat_map(|page| return page.blocks.iter())
    .filter_map(|block| match block {
      PlacedBlock::Image { width, height, .. } => return Some((*width, *height)),
      _ => return None,
    })
    .collect();
}

#[test]
fn figure_images_resolve_to_expected_display_sizes() {
  // `figure.sei` は画像のサイズ指定 4 パターン（両指定 / width のみ / height のみ /
  // 両省略）と SVG を通す。`figure.sei` は画像実体への依存で golden の対象外なので、
  // 寸法の確定はこのテストが固定する（本文幅 = 段幅は 425mm）
  let expected = [
    (Length::mm(80.0), Length::mm(60.0)),     // testimage1 1252x830・両指定
    (Length::mm(80.0), Length::mm(120.0)),    // testimage2 1000x1500・width のみ → 高さは縦横比から
    (Length::mm(90.0), Length::mm(60.0)),     // testimage4 1200x800・height のみ → 幅は縦横比から
    (Length::mm(80.0), Length::mm(60.0)),     // testimage3 2348x3128・両指定
    (Length::mm(425.0), Length::mm(566.667)), // testimage5 756x1008・両省略 → 段幅いっぱい
    (Length::mm(80.0), Length::mm(48.0)),     // testimage6 SVG 200x120・width のみ
  ];

  let sizes = figure_image_sizes();

  assert_eq!(sizes.len(), expected.len(), "画像は 6 枚あるはず: {sizes:?}");
  for (index, ((width, height), (expected_width, expected_height))) in sizes.iter().zip(expected).enumerate() {
    assert!(
      (width.to_mm() - expected_width.to_mm()).abs() < 0.01,
      "画像 {index} の幅: actual={}mm expected={}mm",
      width.to_mm(),
      expected_width.to_mm()
    );
    assert!(
      (height.to_mm() - expected_height.to_mm()).abs() < 0.01,
      "画像 {index} の高さ: actual={}mm expected={}mm",
      height.to_mm(),
      expected_height.to_mm()
    );
  }
}

#[test]
fn figure_image_without_size_fits_two_column_width_not_text_width() {
  // 2 段組みでは本文の 1 段あたりの幅（`body_column_width`）が単段の `text_width`
  // （425mm）より狭い。サイズ両省略の画像（testimage5）は段幅いっぱいにフィットするので、
  // 本文パスの呼び出し元が誤って `text_width` を渡していれば幅は 425mm のままになり検出できる
  let project = TestProject::builder()
    .sources(&["tests/text/figure.sei"])
    .assets(FIGURE_IMAGE_ASSETS)
    .style_toml(|table| test_support::set(table, "columns", "count", 2))
    .build();
  let layout = project.layout().expect("figure.sei は 2 段組みでも組版できるはず");
  let sizes: Vec<(Length, Length)> = layout
    .pages
    .iter()
    .flat_map(|page| return page.blocks.iter())
    .filter_map(|block| match block {
      PlacedBlock::Image { width, height, .. } => return Some((*width, *height)),
      _ => return None,
    })
    .collect();

  // 6 枚中サイズ両省略は testimage5（5 番目、index 4）
  assert_eq!(sizes.len(), 6, "画像は 6 枚あるはず: {sizes:?}");
  let (width, height) = sizes[4];

  // 段間 18pt（既定）を引いた 2 段組みの段幅（実測して固定した値）
  let expected_width = Length::mm(209.325);
  let expected_height = Length::mm(279.1);
  assert!(
    (width.to_mm() - expected_width.to_mm()).abs() < 0.01,
    "2 段組みの段幅いっぱいにフィットするはず: actual={}mm expected={}mm",
    width.to_mm(),
    expected_width.to_mm()
  );
  assert!(
    (height.to_mm() - expected_height.to_mm()).abs() < 0.01,
    "縦横比から決まる高さ: actual={}mm expected={}mm",
    height.to_mm(),
    expected_height.to_mm()
  );
}

/// 脚注本体の先頭行の先頭ボックス（脚注エリア側の上付きマーカー）に描かれた番号を読む。
///
/// マーカーは `InlineNode::Raise` なので `HBoxContent::Atom` の子にグリフ列を持つ。既定の
/// `marker_format`（`{number}`）では番号の数字だけが並ぶ。
fn footnote_marker_number(blocks: &[PlacedBlock]) -> u32 {
  let marker = blocks
    .iter()
    .find_map(|block| match block {
      PlacedBlock::Line { line, .. } => return line.boxes.first(),
      _ => return None,
    })
    .expect("脚注本体は先頭行にマーカーを持つはず");
  let HBoxContent::Atom(children) = &marker.hbox.content else {
    panic!("脚注マーカーは上付きの閉じた箱のはず: {marker:?}");
  };
  let text: String = children
    .iter()
    .filter_map(|child| match &child.hbox.content {
      HBoxContent::Glyphs(run) => return Some(run.text.as_str()),
      HBoxContent::Atom(_) | HBoxContent::Rule => return None,
    })
    .collect();
  return text.parse().unwrap_or_else(|_| panic!("マーカーは番号の数字だけのはず: {text:?}"));
}

/// `footnote_per_page.sei` を指定の採番方式（style.toml の `[footnote].numbering` の値）で組版し、
/// ページごとに、そのページで始まる脚注のマーカー番号列を返すテストヘルパ
fn footnote_numbers_per_page(numbering: &'static str) -> Vec<Vec<u32>> {
  // 採番方式は fixture 差分の既定値を上書きする（`golden_fixture` の後に適用される）
  let laid_out = TestProject::builder()
    .golden_fixture("footnote_per_page")
    .style_toml(move |table| test_support::set(table, "footnote", "numbering", numbering))
    .build()
    .laid_out();
  return laid_out
    .pages
    .iter()
    .map(|page| {
      return page
        .footnotes
        .iter()
        .filter(|footnote| return !footnote.continued)
        .map(|footnote| return footnote_marker_number(&footnote.blocks))
        .collect();
    })
    .collect();
}

#[test]
fn per_page_footnote_numbering_restarts_on_each_page() {
  let per_page = footnote_numbers_per_page("per_page");

  // 脚注を持つページが 2 つ以上あり（空振りでないこと）、どのページも 1 から始まる連番。
  // 入力は 1 ページ目に 10 個置くので、2 ページ目は通し番号なら 11 以降＝マーカーが 2 桁になる。
  // ページ単位採番では 1 桁に縮み、その幅の変化が行分割へ跳ね返る循環を踏んだうえで収束している。
  let pages_with_footnotes: Vec<&Vec<u32>> = per_page.iter().filter(|numbers| return !numbers.is_empty()).collect();
  assert!(pages_with_footnotes.len() >= 2, "脚注が 2 ページ以上に分かれるはず: {per_page:?}");
  for numbers in pages_with_footnotes {
    let expected: Vec<u32> = (1..=u32::try_from(numbers.len()).expect("脚注数は u32 に収まる")).collect();
    assert_eq!(*numbers, expected, "各ページの脚注番号は 1 からの連番のはず: {per_page:?}");
  }
}

/// `\index` の出現ページが「マーカーを含む内容が実際に置かれたページ」になることを、
/// 脚注のページ繰越と表のページ跨ぎの両方で end-to-end に確かめる。
///
/// 帰属を決める `crate::typeset::breaking` 側の単体テストと違い、こちらはソース（`.sei`）から
/// 確定ページまでを通すので、frontend の許可・lowering・collector の配線が繋がっていないと落ちる。
#[test]
fn index_entries_follow_the_page_the_content_lands_on() {
  let laid_out = TestProject::builder().golden_fixture("index_split").build().laid_out();

  // 索引語ごとに、載っているページ index の集合を作る
  let pages_of = |word: &str| -> Vec<usize> {
    return laid_out
      .pages
      .iter()
      .enumerate()
      .filter(|(_, page)| return page.index_entries.iter().any(|entry| return entry.word == word))
      .map(|(index, _)| return index)
      .collect();
  };
  // 脚注が繰越されている（空振り検知）
  let carried = laid_out
    .pages
    .iter()
    .position(|page| return page.footnotes.iter().any(|f| return f.continued))
    .unwrap_or_else(|| panic!("脚注が分割されて繰越が生じるはず: {:?}", laid_out.pages.len()));
  assert!(carried > 0, "繰越は 2 ページ目以降に現れるはず");
  assert_eq!(pages_of("脚注冒頭"), vec![carried - 1], "脚注本体の先頭行はマーカーのあるページに残る");
  assert_eq!(pages_of("脚注繰越"), vec![carried], "繰越された行の索引語は繰越先ページへ帰属する");

  // 表は 2 ページ以上に跨り、各行の索引語は自分の行が落ちたページにだけ現れる
  let row_pages: Vec<Vec<usize>> = (1..=16).map(|i| return pages_of(&format!("表{i}"))).collect();
  for (i, pages) in row_pages.iter().enumerate() {
    assert_eq!(pages.len(), 1, "表 {} 行目の索引語はちょうど 1 ページに載るはず: {row_pages:?}", i + 1);
  }
  let first = row_pages[0][0];
  let last = row_pages[15][0];
  assert!(last > first, "表がページを跨いでいるはず（空振り検知）: {row_pages:?}");
  assert!(row_pages.windows(2).all(|w| return w[0][0] <= w[1][0]), "行の順序どおりに並ぶはず: {row_pages:?}");
}

/// 脚注本体のリンクが、その行が落ちたページのクリック矩形になることを end-to-end で確かめる
///
/// `footnote_split.sei` の長い脚注は前半に `\href`、繰越される後半に `\ref` を持つ。帰属を決める
/// `crate::typeset::breaking` 側の単体テストと違い、こちらはソース（`.sei`）から確定ページまでを
/// 通すので、frontend・lowering・collector の配線が繋がっていないと落ちる。ページ index は
/// 版面の都合で動きうるので、繰越が起きたページを基準に相対で見る。
#[test]
fn footnote_links_follow_the_page_the_line_lands_on() {
  let laid_out = TestProject::builder().golden_fixture("footnote_split").build().laid_out();

  // 脚注が繰越されている（空振り検知）
  let carried = laid_out
    .pages
    .iter()
    .position(|page| return page.footnotes.iter().any(|f| return f.continued))
    .unwrap_or_else(|| panic!("脚注が分割されて繰越が生じるはず: {} ページ", laid_out.pages.len()));
  assert!(carried > 0, "繰越は 2 ページ目以降に現れるはず");

  // 折り返しで矩形が 2 つに割れることがあるので、個数ではなく「あるか」で見る。
  // 本文中の脚注マーカーも内部リンクを作るので、`\ref` の到達先 namespace（`Label`）で絞る。
  let pages_with = |predicate: &dyn Fn(&LinkTarget) -> bool| -> Vec<usize> {
    return laid_out
      .pages
      .iter()
      .enumerate()
      .filter(|(_, page)| return page.links.iter().any(|link| return predicate(&link.target)))
      .map(|(index, _)| return index)
      .collect();
  };
  let external =
    pages_with(&|target| return matches!(target, LinkTarget::External(uri) if uri == "https://example.com"));
  let reference = pages_with(&|target| return matches!(target, LinkTarget::Internal(AnchorId::Label(_))));
  assert_eq!(external, vec![carried - 1], "脚注本体の前半のリンクはマーカーのあるページに残る");
  assert_eq!(reference, vec![carried], "繰越された行のリンクは繰越先ページのクリック矩形になる");
}

#[test]
fn long_footnote_splits_across_pages_without_overlapping_body() {
  let laid_out = TestProject::builder().golden_fixture("footnote_split").build().laid_out();

  // 最初の脚注（index 0）の続きが次ページへ繰り越される
  let fragments: Vec<Vec<(u32, bool)>> = laid_out
    .pages
    .iter()
    .map(|page| return page.footnotes.iter().map(|f| return (f.index, f.continued)).collect())
    .collect();
  let carried = fragments
    .iter()
    .position(|page| return page.iter().any(|(_, continued)| return *continued))
    .unwrap_or_else(|| panic!("脚注が分割されて繰越が生じるはず（空振り検知）: {fragments:?}"));
  assert!(carried > 0, "繰越は 2 ページ目以降に現れるはず: {fragments:?}");
  // 繰越はそのページの脚注領域の先頭（自前の脚注より前）に置かれる
  assert_eq!(fragments[carried].first(), Some(&(0, true)), "繰越が脚注領域の先頭のはず: {fragments:?}");
  assert!(
    fragments[carried].iter().any(|(index, continued)| return *index == 1 && !continued),
    "繰越先ページの自前の脚注が繰越の後ろに積まれるはず: {fragments:?}"
  );
  // 本文と脚注が重ならない（繰越ページも含めて）
  for (index, page) in laid_out.pages.iter().enumerate() {
    let Some(body_bottom) = page.blocks.iter().filter_map(block_bottom).reduce(Length::max) else {
      continue;
    };
    let Some(footnote_top) =
      page.footnotes.iter().flat_map(|f| return &f.blocks).filter_map(block_top).reduce(Length::min)
    else {
      continue;
    };
    assert!(
      footnote_top >= body_bottom,
      "page {index}: 本文の下端 {} と脚注の上端 {} が重なっている",
      body_bottom.to_pt(),
      footnote_top.to_pt()
    );
  }
}

/// 配置済みブロックの上端（脚注領域の重なり判定に使う。行・罫線のみを見る）
fn block_top(block: &PlacedBlock) -> Option<Length> {
  return match block {
    PlacedBlock::Line { line, baseline_y } => Some(*baseline_y - line.height),
    PlacedBlock::Rule { y, .. } => Some(*y),
    _ => None,
  };
}

/// 配置済みブロックの下端（本文の重なり判定に使う。行のみを見る）
fn block_bottom(block: &PlacedBlock) -> Option<Length> {
  return match block {
    PlacedBlock::Line { line, baseline_y } => Some(*baseline_y + line.depth),
    _ => None,
  };
}

#[test]
fn continuous_footnote_numbering_runs_through_pages() {
  let continuous = footnote_numbers_per_page("continuous");

  let flattened: Vec<u32> = continuous.iter().flatten().copied().collect();
  let expected: Vec<u32> = (1..=u32::try_from(flattened.len()).expect("脚注数は u32 に収まる")).collect();
  assert_eq!(flattened, expected, "通し採番はページをまたいで連番のはず: {continuous:?}");
}

#[test]
fn layout_dump_changes_with_line_height() {
  // 行送り（line_height_factor）だけを変えた 2 スタイル。行送りは 2 行目以降の
  // ベースライン送りに効くため、複数行が縦に並ぶ入力（itemize）を対象にする。
  let taller = TestProject::builder()
    .golden_fixture("itemize")
    .style_toml(|table| {
      let base = table["text"]["line_height_factor"]
        .as_float()
        .expect("fixture style.toml は [text].line_height_factor を持つはず");
      test_support::set(table, "text", "line_height_factor", base + 0.5);
    })
    .build();

  let base_dump = dump_pages_of("itemize");
  let taller_dump = dump_pages(&taller.laid_out().pages);

  assert_ne!(base_dump, taller_dump);
}

#[test]
fn layout_dump_changes_with_punctuation_spacing() {
  // 和文約物アキ調整（JIS X 4051）の on/off だけを変えた 2 スタイル。
  // 約物が密な入力（yakumono）で連続約物の詰め・約物の収縮点化が座標差として現れる。
  let disabled = TestProject::builder()
    .golden_fixture("yakumono")
    .style_toml(|table| test_support::set(table, "text", "punctuation_spacing", false))
    .build();

  let enabled_dump = dump_pages_of("yakumono");
  let disabled_dump = dump_pages(&disabled.laid_out().pages);

  assert_ne!(enabled_dump, disabled_dump);
}

/// fixture を組版した確定ページ列を返す。`alignment` が `Some` なら `[text].alignment` をその値にする。
fn pages_with_text_alignment(name: &str, alignment: Option<&'static str>) -> Vec<Page> {
  let mut builder = TestProject::builder().golden_fixture(name);
  if let Some(alignment) = alignment {
    builder = builder.style_toml(move |table| test_support::set(table, "text", "alignment", alignment));
  }
  return builder.build().laid_out().pages;
}

#[test]
fn text_alignment_leaves_kind_specific_alignment_untouched() {
  // 本文の行数が揃えで変わり縦位置はずれうるので、水平位置だけを比べる
  let footnote_dx = |pages: &[Page]| {
    return pages
      .iter()
      .flat_map(|page| return page.footnotes.iter())
      .flat_map(|footnote| return footnote.blocks.iter())
      .flat_map(|block| match block {
        PlacedBlock::Line { line, .. } => return line.boxes.iter().map(|placed| return placed.dx).collect::<Vec<_>>(),
        _ => return Vec::new(),
      })
      .collect::<Vec<Length>>();
  };
  let math_x = |pages: &[Page]| {
    return pages
      .iter()
      .flat_map(|page| return page.blocks.iter())
      .filter_map(|block| match block {
        PlacedBlock::MathBlock { x, .. } => return Some(*x),
        _ => return None,
      })
      .collect::<Vec<Length>>();
  };

  for alignment in ["center", "right"] {
    let footnote = footnote_dx(&pages_with_text_alignment("footnote", Some(alignment)));
    let equation = math_x(&pages_with_text_alignment("equation", Some(alignment)));
    let title = pages_with_text_alignment("title_page", Some(alignment));
    assert!(!footnote.is_empty() && !equation.is_empty(), "比較対象の脚注・数式ブロックがあるはず");

    assert_eq!(footnote, footnote_dx(&pages_with_text_alignment("footnote", None)), "脚注本体（{alignment}）");
    assert_eq!(equation, math_x(&pages_with_text_alignment("equation", None)), "数式ブロック（{alignment}）");
    assert_eq!(
      format!("{:?}", title[0].blocks),
      format!("{:?}", pages_with_text_alignment("title_page", None)[0].blocks),
      "タイトルページ（{alignment}）"
    );
  }
}

/// 本文 `text` を組んだページのダンプを返す。`alignment` が `Some` なら `[text].alignment` をその値にする。
///
/// fixture の版面は行長が約 1200pt あり段落が 1 行に収まる（最終行は両端揃えでも伸びない）ので、左右の余白を
/// 広げて段落を複数行に折り返させる。
fn dump_source(text: &str, alignment: Option<&'static str>) -> String {
  let mut builder = TestProject::builder().source_text(text).style_toml(|table| {
    test_support::set(table, "page", "margin_left", "250mm");
    test_support::set(table, "page", "margin_right", "250mm");
  });
  if let Some(alignment) = alignment {
    builder = builder.style_toml(move |table| test_support::set(table, "text", "alignment", alignment));
  }
  return dump_pages(&builder.build().laid_out().pages);
}

/// 寄せ環境の等価テストの本文。`[text].alignment` に従う要素（複数行の段落・`\\`・揃え未指定の見出し・入れ子の
/// リスト・引用・定理・証明の QED）と、種類で揃えが決まる要素（脚注本体・数式ブロック・コード・表）を並べる。
const FLUSH_BODY: &str = "\\subsection{見出し}

寄せ環境の本体は、style.toml の text.alignment を環境の向きに置き換えた文書と同じに組まれる。複数行に折り返す
だけの長さを持たせ、各行が独立に寄ることと、両端揃えの伸縮が起きないことを確かめる\\footnote{脚注本体は左。}。\\\\
強制改行の後の行も独立に寄る。

\\begin{itemize}
\\item{外側の項目。
\\begin{enumerate}
\\item{入れ子の項目。}
\\end{enumerate}
}
\\end{itemize}

\\begin{quote}
引用の本体。
\\end{quote}

\\begin{theorem}
定理の本体。
\\end{theorem}

\\begin{proof}
証明の本体。
\\end{proof}

\\begin{equation}
a + b = c
\\end{equation}

\\begin{code}
let x = 1;
\\end{code}

\\begin{table}[columns=left right]
\\row{左 & 右}
\\end{table}
";

#[test]
fn flush_environment_matches_text_alignment_of_the_same_direction() {
  let justified = dump_source(FLUSH_BODY, None);

  for (name, alignment) in [
    ("flushleft", "left"),
    ("center", "center"),
    ("flushright", "right"),
  ] {
    let wrapped = dump_source(&format!("\\begin{{{name}}}\n{FLUSH_BODY}\\end{{{name}}}\n"), None);
    let restyled = dump_source(FLUSH_BODY, Some(alignment));

    assert_eq!(wrapped, restyled, "{name} の本体は [text].alignment = \"{alignment}\" の文書と同じに組まれるはず");
    assert_ne!(wrapped, justified, "{name} は既定（両端揃え）の組版を変えるはず");
  }
}

#[test]
fn nested_flush_environment_uses_innermost_direction_and_reverts_after_it() {
  let nested =
    "\\begin{flushright}\n右の段落\n\n\\begin{center}\n中央の段落\n\\end{center}\n\n右へ戻る段落\n\\end{flushright}\n";
  let flat = "\\begin{flushright}\n右の段落\n\\end{flushright}\n\n\\begin{center}\n中央の段落\n\\end{center}\n\n\
              \\begin{flushright}\n右へ戻る段落\n\\end{flushright}\n";
  let plain = "右の段落\n\n中央の段落\n\n右へ戻る段落\n";

  let nested_dump = dump_source(nested, None);

  assert_eq!(nested_dump, dump_source(flat, None), "内側の環境が閉じた後は外側の向きに戻るはず");
  assert_ne!(nested_dump, dump_source(plain, None), "寄せ環境は組版を変えるはず");
}

#[test]
fn empty_flush_environment_adds_no_vertical_space() {
  let with_empty = dump_source("前の段落\n\n\\begin{center}\\end{center}\n\n後の段落\n", None);

  assert_eq!(with_empty, dump_source("前の段落\n\n後の段落\n", None));
}

/// コードブロックの空行が 1 行ぶんの高さを保つことを確認する。
///
/// 空行は内容が空の Atom 1 つになるので、素朴に組むと行の高さ・深さが 0 になり、行送りが
/// `leading.max(前の行の深さ + この行の高さ)` の leading まで縮む（他の行より詰まる）。
/// `typeset::boxing` の strut がこれを防いでいる。
#[test]
fn blank_code_line_keeps_a_full_line_height() {
  // code.sei は空行を含むコードブロックを 2 つ持つ
  let dump = dump_pages_of("code");

  assert!(!dump.contains("height=0.00"), "高さ 0 の行は出ないはず（空行も 1 行ぶんの extent を持つ）");
}

/// 前付けの構成（タイトルページ・目次）を切り替えて組んだ総ページ数を返すテストヘルパ。
///
/// `blank_metadata` が真なら config の `[document]` から title / author / date を取り除く
/// （タイトルページに載せる中身が無い状態）。
fn page_count_with_front_matter(title_page: bool, toc: bool, blank_metadata: bool) -> usize {
  let laid_out = TestProject::builder()
    .sources(&["tests/text/toc.sei"])
    .config_toml(move |table| {
      if blank_metadata && let Some(document) = table.get_mut("document").and_then(toml::Value::as_table_mut) {
        document.remove("title");
        document.remove("author");
        document.remove("date");
      }
    })
    .style_toml(move |table| {
      test_support::set(table, "title_page", "enabled", title_page);
      test_support::set(table, "toc", "enabled", toc);
    })
    .build()
    .laid_out();
  return laid_out.pages.len();
}

#[test]
fn front_matter_adds_no_blank_pages() {
  let body_only = page_count_with_front_matter(false, false, false);

  let title_only = page_count_with_front_matter(true, false, false);
  let toc_only = page_count_with_front_matter(false, true, false);
  let both = page_count_with_front_matter(true, true, false);
  let empty_title_only = page_count_with_front_matter(true, false, true);
  let empty_title_and_toc = page_count_with_front_matter(true, true, true);

  assert_eq!(title_only, body_only + 1, "タイトルページだけなら 1 ページ増える");
  assert!(toc_only > body_only, "目次だけなら 1 ページ以上増える: {toc_only} vs {body_only}");
  assert_eq!(both, toc_only + 1, "両方ならタイトルページ 1 + 目次のページ数");
  assert_eq!(empty_title_only, body_only, "中身の無いタイトルページは白紙ページを作らない");
  assert_eq!(empty_title_and_toc, toc_only, "中身の無いタイトルページは目次の前に白紙ページを作らない");
}

/// 本文 `text` 1 本を serif の書字方向 `direction` で compile し、全ページの `DrawGlyphRun` のグリフ列を返す。
fn glyph_runs_of(text: &str, direction: &'static str) -> Vec<GlyphRun> {
  let compilation = TestProject::builder()
    .source_text(text)
    .config_toml(move |table| {
      let serif = table
        .get_mut("font_configs")
        .and_then(|font_configs| return font_configs.as_table_mut())
        .and_then(|font_configs| return font_configs.get_mut("serif"))
        .and_then(|serif| return serif.as_table_mut())
        .expect("fixture config.toml は [font_configs.serif] を持つはず");
      serif.insert("direction".to_string(), toml::Value::from(direction));
    })
    .build()
    .compile()
    .unwrap_or_else(|failure| panic!("本文 {text:?} の compile は成功するはず: {:?}", failure.into_report()));
  return compilation
    .publication
    .pages()
    .iter()
    .flat_map(|page| return page.ops())
    .filter_map(|op| {
      let PaintOp::DrawGlyphRun { run, .. } = op else {
        return None;
      };
      return Some(run.clone());
    })
    .collect();
}

/// `run` のグリフ範囲がクラスタ単位で元テキストを過不足なく覆うことを検査する。
///
/// 各範囲は空でなく文字境界に乗り、同じ範囲（同じクラスタのグリフ）を除いて重ならず、和がテキスト全体になる。
fn assert_ranges_tile_text(run: &GlyphRun) {
  let mut ranges: Vec<Range<usize>> = run.glyphs.iter().map(|glyph| return glyph.range.clone()).collect();
  for range in &ranges {
    assert!(
      range.start < range.end && range.end <= run.text.len(),
      "範囲 {range:?} はテキスト {:?} の中の空でない範囲のはず",
      run.text
    );
    assert!(
      run.text.is_char_boundary(range.start) && run.text.is_char_boundary(range.end),
      "範囲 {range:?} の両端はテキスト {:?} の文字境界のはず",
      run.text
    );
  }
  ranges.sort_by_key(|range| return (range.start, range.end));
  ranges.dedup();
  let mut covered = 0;
  for range in &ranges {
    assert_eq!(range.start, covered, "クラスタ範囲は隙間も重なりもなく並ぶはず: {ranges:?} / {:?}", run.text);
    covered = range.end;
  }
  assert_eq!(covered, run.text.len(), "クラスタ範囲はテキスト全体を覆うはず: {ranges:?} / {:?}", run.text);
}

#[test]
fn glyph_ranges_tile_text_with_combining_marks() {
  let runs = glyph_runs_of("x a\u{308}\u{301}b y e\u{301}\u{200D}f", "left-to-right");

  for run in &runs {
    assert_ranges_tile_text(run);
  }
  assert!(
    runs
      .iter()
      .any(|run| return run.glyphs.windows(2).any(|pair| return pair[0].range == pair[1].range)),
    "結合文字のクラスタは複数グリフで組まれるはず（テストが複数グリフのクラスタを通っていない）"
  );
}

#[test]
fn glyph_ranges_tile_text_in_right_to_left_runs() {
  // 行幅を超える長さにし、切らずに 1 箱で積む経路でも compile が通ることを見る
  let runs = glyph_runs_of(&"abc def a\u{308}\u{301}b ".repeat(60), "right-to-left");

  for run in &runs {
    assert_ranges_tile_text(run);
  }
  assert!(
    runs
      .iter()
      .any(|run| return run.glyphs.windows(2).any(|pair| return pair[0].range.start > pair[1].range.start)),
    "右から左の run はクラスタ降順で組まれるはず（テストが RTL を通っていない）"
  );
}

#[test]
fn glyph_ranges_tile_text_in_japanese_clusters() {
  let runs = glyph_runs_of("あ」\u{FE00}い。か\u{3099}き「漢\u{E0100}字」", "left-to-right");

  for run in &runs {
    assert_ranges_tile_text(run);
  }
}

/// `vendor/fonts/STIXTwoMath-Regular.ttf`（golden の数式フォント）の MATH 定数の生の値と upem。
///
/// 組版側と独立に MATH を読む。
fn stix_math_constant(constant: MathConstant) -> (i32, u16) {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
  let bytes = fs::read(&path).expect("vendor の STIX Two Math を読めるはず（tools/fetch-test-assets.sh）");
  let font = FontRef::new(&bytes).expect("STIX Two Math を解析できるはず");
  let upem = font.head().expect("head を読めるはず").units_per_em();
  let value = font
    .math()
    .and_then(|math| return math.math_constants())
    .expect("MathConstants を読めるはず")
    .constant(constant);
  return (value, upem);
}

/// 長さの MATH 定数の、`font_size` での長さ（組版側と同じ換算 `font_size × 値 / upem`）。
fn stix_math_length(constant: MathConstant, font_size: Length) -> Length {
  let (value, upem) = stix_math_constant(constant);
  return font_size.scale(f64::from(value) / f64::from(upem));
}

/// 縮小率の MATH 定数（百分率）を `base` に掛けたフォントサイズ（組版側と同じ換算）。
fn stix_scaled_size(constant: MathConstant, base: Length) -> Length {
  let (percent, _) = stix_math_constant(constant);
  return base.scale(f64::from(percent) / 100.0);
}

/// 箱の中を先頭から辿り、最初のグリフ列を返す。
fn first_glyph_run(content: &HBoxContent) -> Option<&GlyphRun> {
  return match content {
    HBoxContent::Glyphs(run) => Some(run),
    HBoxContent::Atom(children) => children.iter().find_map(|child| return first_glyph_run(&child.hbox.content)),
    HBoxContent::Rule => None,
  };
}

/// 括弧の種類すべて（matrix の `delimiter` の 5 種と、`None` は cases）
const DELIMITERS: [Option<&str>; 6] = [
  Some("paren"),
  Some("bracket"),
  Some("brace"),
  Some("bar"),
  Some("dbar"),
  None,
];

/// `rows` 行の表示数式の本文。`delimiter` は matrix の `delimiter` の値で、`None` は cases（左の波括弧だけ）。
fn delimited_source(delimiter: Option<&str>, rows: usize) -> String {
  let body = iter::repeat_n("a & b", rows).collect::<Vec<_>>().join(" \\\\\n");
  return match delimiter {
    Some(delimiter) => format!("\\begin{{matrix}}[delimiter={delimiter}]\n{body}\n\\end{{matrix}}\n"),
    None => format!("\\begin{{cases}}\n{body}\n\\end{{cases}}\n"),
  };
}

/// 区切り括弧で包んだ表示数式の、本体グリッドの位置と寸法
struct DelimitedGrid {
  /// ベースラインからの縦位置（正で上）
  dy: Length,
  /// 高さ
  height: Length,
  /// 深さ
  depth: Length,
  /// セルのフォントサイズ（数式本体のフォントサイズ）
  font_size: Length,
}

/// 区切り括弧 1 つの位置・幅とグリフ列
struct PlacedDelimiter {
  /// ベースラインからの縦位置（正で上）
  dy: Length,
  /// 送り幅
  width: Length,
  /// グリフ列
  run: GlyphRun,
}

/// 区切り括弧で包んだ表示数式 1 つの測定値
struct DelimitedBlock {
  /// 包み直した全体の高さ
  height: Length,
  /// 包み直した全体の深さ
  depth: Length,
  /// 本体グリッド
  grid: DelimitedGrid,
  /// 区切り括弧（左から順）
  delimiters: Vec<PlacedDelimiter>,
}

/// 本文 `source` を組版し、最初の表示数式ブロックを測る。
fn measure_delimited_block(source: &str) -> DelimitedBlock {
  let laid_out = TestProject::builder().source_text(source).build().laid_out();
  let body = laid_out
    .pages
    .iter()
    .flat_map(|page| return page.blocks.iter())
    .find_map(|block| match block {
      PlacedBlock::MathBlock { body, .. } => return Some(body),
      _ => return None,
    })
    .expect("表示数式ブロックが 1 つあるはず");
  let HBoxContent::Atom(children) = &body.content else {
    panic!("区切り括弧で包んだ本体は Atom のはず");
  };
  // 子は [左括弧, グリッド, 右括弧（あれば）] の順
  let grid = &children[1];
  let delimiters = children
    .iter()
    .enumerate()
    .filter(|&(index, _)| return index != 1)
    .map(|(_, child)| {
      let HBoxContent::Glyphs(run) = &child.hbox.content else {
        panic!("区切り括弧はグリフ列 1 本のはず");
      };
      return PlacedDelimiter {
        dy: child.dy,
        width: child.hbox.width,
        run: run.clone(),
      };
    })
    .collect();
  return DelimitedBlock {
    height: body.height,
    depth: body.depth,
    grid: DelimitedGrid {
      dy: grid.dy,
      height: grid.hbox.height,
      depth: grid.hbox.depth,
      font_size: first_glyph_run(&grid.hbox.content).expect("グリッドにはセルのグリフがあるはず").font_size,
    },
    delimiters,
  };
}

/// STIX Two Math のグリフのインクの上端（ベースライン基準・上が正）と高さを、フォント単位で組版側と独立に読む。
fn stix_glyph_ink(gid: u32) -> (f64, f64) {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
  let bytes = fs::read(&path).expect("vendor の STIX Two Math を読めるはず（tools/fetch-test-assets.sh）");
  let font = Font::new(bytes, 0).expect("STIX Two Math は sfnt として読めるはず");
  let extents = font.glyph_metrics().extents(GlyphId::new(gid)).expect("STIX の括弧のグリフはインクを読めるはず");
  return (f64::from(extents.y_bearing), f64::from(extents.height));
}

/// グリフ列のインクの上端と下端（グリフ列のベースライン基準・上が正）を、組版側と独立に STIX Two Math から測る。
fn stix_run_ink(run: &GlyphRun) -> (Length, Length) {
  let (_, upem) = stix_math_constant(MathConstant::AxisHeight);
  let to_length = |units: f64| return run.font_size.scale(units / f64::from(upem));
  return run
    .glyphs
    .iter()
    .map(|glyph| {
      let (y_bearing, height) = stix_glyph_ink(glyph.gid);
      let top = y_bearing + f64::from(glyph.y_offset);
      return (to_length(top), to_length(top - height));
    })
    .reduce(|(top, bottom), (other_top, other_bottom)| return (top.max(other_top), bottom.min(other_bottom)))
    .expect("区切り括弧は 1 つ以上のグリフを持つはず");
}

/// `vendor/fonts/STIXTwoMath-Regular.ttf` を組版側と独立に開く。
fn stix_math_font() -> Font {
  let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../vendor/fonts/STIXTwoMath-Regular.ttf");
  let bytes = fs::read(&path).expect("vendor の STIX Two Math を読めるはず（tools/fetch-test-assets.sh）");
  return Font::new(bytes, 0).expect("STIX Two Math は sfnt として読めるはず");
}

/// STIX Two Math のグリフ `gid` の送り幅（フォント単位）。
fn stix_advance(gid: u32) -> i32 {
  #[expect(clippy::cast_possible_truncation, reason = "STIX Two Math の送り幅は整数のフォント単位")]
  let advance = stix_math_font().glyph_metrics().h_advance(GlyphId::new(gid)) as i32;
  return advance;
}

/// STIX Two Math のグリフ `gid` のイタリック補正（フォント単位。登録が無ければ 0）。
fn stix_italics_correction(gid: u32) -> i32 {
  let font = stix_math_font();
  let info = font
    .tables()
    .math()
    .and_then(|math| return math.math_glyph_info())
    .expect("MathGlyphInfo を読めるはず");
  return info
    .math_italics_correction_info()
    .expect("STIX Two Math は MathItalicsCorrectionInfo を持つはず")
    .expect("MathItalicsCorrectionInfo を読めるはず")
    .correction(GlyphId::new(gid))
    .unwrap_or(0);
}

/// STIX Two Math のフォント単位 `units` の、`font_size` での長さ（組版側と同じ換算）。
fn stix_units(units: i32, font_size: Length) -> Length {
  let (_, upem) = stix_math_constant(MathConstant::AxisHeight);
  return font_size.scale(f64::from(units) / f64::from(upem));
}

/// STIX Two Math のグリフ `gid` の `corner` の math kern の、フォント単位の高さ `height` での値（表が無ければ 0）。
fn stix_math_kern(gid: u32, corner: MathKernCorner, height: i32) -> i32 {
  let font = stix_math_font();
  let info = font
    .tables()
    .math()
    .and_then(|math| return math.math_glyph_info())
    .expect("MathGlyphInfo を読めるはず");
  let Some(kern_info) = info.math_kern_info() else {
    return 0;
  };
  return kern_info
    .expect("MathKernInfo を読めるはず")
    .kern(GlyphId::new(gid), corner)
    .and_then(|kern| return kern.kerning(height))
    .unwrap_or(0);
}

/// 長さ `length` の、`font_size` でのフォント単位（組版側と同じ四捨五入）。
fn stix_height_units(length: Length, font_size: Length) -> i32 {
  let (_, upem) = stix_math_constant(MathConstant::AxisHeight);
  #[expect(clippy::cast_possible_truncation, reason = "数式 1 つの高さのフォント単位で i32 に収まる")]
  let units = (length.ratio(font_size) * f64::from(upem)).round() as i32;
  return units;
}

/// 1 グリフのグリフ列の、そのグリフの (gid, 送り幅)。
fn sole_glyph(line_run: &LineRun) -> (u32, i32) {
  let [glyph] = line_run.run.glyphs.as_slice() else {
    panic!("{:?} は 1 グリフのはず", line_run.run.text);
  };
  return (glyph.gid, glyph.x_advance);
}

/// 数学用イタリックの 𝑓（U+1D453）
const MATH_F: &str = "\u{1D453}";

/// 数学用イタリックの 𝑦（U+1D466）
const MATH_Y: &str = "\u{1D466}";

/// 数学用イタリックの 𝑎（U+1D44E）
const MATH_A: &str = "\u{1D44E}";

/// 数学用イタリックの 𝑏（U+1D44F）
const MATH_B: &str = "\u{1D44F}";

/// 数学用イタリックの 𝑑（U+1D451）
const MATH_D: &str = "\u{1D451}";

/// 分数の左右それぞれのアキ（組版側の `FRACTION_PADDING` と同じ 0.75pt）
const FRACTION_PADDING: Length = Length::from_sp(49_152);

#[test]
fn delimiters_center_on_the_math_axis_at_the_body_font_size() {
  for delimiter in DELIMITERS {
    for rows in [1, 2, 6] {
      let source = delimited_source(delimiter, rows);
      let block = measure_delimited_block(&source);

      let axis = stix_math_length(MathConstant::AxisHeight, block.grid.font_size);
      assert_eq!(
        block.grid.dy + (block.grid.height - block.grid.depth) / 2.0,
        axis,
        "グリッドの縦中央が数式軸に載るはず: {source}"
      );
      assert_eq!(block.delimiters.len(), if delimiter.is_some() { 2 } else { 1 }, "{source}");
      for placed in &block.delimiters {
        assert_eq!(placed.run.font_size, block.grid.font_size, "括弧はフォントサイズを変えずに伸ばすはず: {source}");
        let (top, bottom) = stix_run_ink(&placed.run);
        let center = placed.dy + (top + bottom) / 2.0;
        assert!(
          (center - axis).abs() <= Length::from_sp(1),
          "括弧のインクの縦中央 {center:?} が数式軸 {axis:?} に載るはず: {source}"
        );
      }
    }
  }
}

#[test]
fn delimiters_cover_the_grid_and_stay_inside_the_block() {
  for delimiter in DELIMITERS {
    for rows in [1, 2, 3, 6, 30] {
      let source = delimited_source(delimiter, rows);
      let block = measure_delimited_block(&source);

      let grid_total = block.grid.height + block.grid.depth;
      for placed in &block.delimiters {
        let (top, bottom) = stix_run_ink(&placed.run);
        // size variant は advanceMeasurement で選ぶので、インクの高さはそれより 0.01em 未満だけ短いことがある
        assert!(
          top - bottom + block.grid.font_size.scale(0.01) >= grid_total,
          "括弧のインクの高さ {:?} がグリッドの高さ + 深さ {grid_total:?} を覆うはず: {source}",
          top - bottom
        );
        assert!(
          block.height + Length::from_sp(1) >= placed.dy + top,
          "包み直した箱の高さは括弧のインクの上端を含むはず: {source}"
        );
        assert!(
          block.depth + Length::from_sp(1) >= -(placed.dy + bottom),
          "包み直した箱の深さは括弧のインクの下端を含むはず: {source}"
        );
      }
    }
  }
}

#[test]
fn tall_delimiters_grow_only_vertically() {
  for delimiter in DELIMITERS {
    let short = measure_delimited_block(&delimited_source(delimiter, 6));
    let tall = measure_delimited_block(&delimited_source(delimiter, 12));

    for (short, tall) in short.delimiters.iter().zip(&tall.delimiters) {
      assert!(
        short.run.glyphs.len() > 1 && tall.run.glyphs.len() > 1,
        "6 行・12 行は STIX の最大の size variant を超え、glyph assembly で組むはず: {delimiter:?}"
      );
      assert_eq!(short.width, tall.width, "括弧の幅は行数で変わらないはず: {delimiter:?}");
      let (short_top, short_bottom) = stix_run_ink(&short.run);
      let (tall_top, tall_bottom) = stix_run_ink(&tall.run);
      assert!(tall_top - tall_bottom > short_top - short_bottom, "括弧は縦には伸びるはず: {delimiter:?}");
    }
  }
}

#[test]
fn assembled_delimiter_is_one_character_of_text() {
  for delimiter in DELIMITERS {
    let block = measure_delimited_block(&delimited_source(delimiter, 6));

    for placed in &block.delimiters {
      let whole = 0..placed.run.text.len();
      assert_eq!(placed.run.text.chars().count(), 1, "括弧のグリフ列のテキストは括弧 1 字のはず");
      assert!(
        placed.run.glyphs.iter().all(|glyph| return glyph.range == whole),
        "glyph assembly の全パーツは括弧 1 字のクラスタに属し、PDF のテキストとしては 1 字になるはず: {:?}",
        placed.run.text
      );
    }
  }
}

#[test]
fn assembled_delimiter_parts_stack_bottom_to_top_with_the_width_on_the_last_part() {
  for delimiter in DELIMITERS {
    let block = measure_delimited_block(&delimited_source(delimiter, 6));

    for placed in &block.delimiters {
      let glyphs = &placed.run.glyphs;
      assert!(glyphs.len() > 1, "6 行は glyph assembly で組むはず: {delimiter:?}");
      // 各パーツのインクの下端と上端（フォント単位）
      let inks: Vec<(f64, f64)> = glyphs
        .iter()
        .map(|glyph| {
          let (y_bearing, height) = stix_glyph_ink(glyph.gid);
          let top = y_bearing + f64::from(glyph.y_offset);
          return (top - height, top);
        })
        .collect();
      for (index, pair) in inks.windows(2).enumerate() {
        let ((bottom, top), (next_bottom, _)) = (pair[0], pair[1]);
        assert!(next_bottom > bottom, "パーツは下から上へ積むはず（{index} 番目の次）: {delimiter:?}");
        assert!(next_bottom < top, "隣り合うパーツのインクは重なるはず（{index} 番目の次）: {delimiter:?}");
      }
      let (last, rest) = glyphs.split_last().expect("1 つ以上のグリフがあるはず");
      assert!(
        rest.iter().all(|glyph| return glyph.x_advance == 0),
        "最後以外のパーツは送り幅を持たないはず: {delimiter:?}"
      );
      assert!(last.x_advance > 0, "括弧の幅は最後のパーツが持つはず: {delimiter:?}");
    }
  }
}

/// 行の中のグリフ列 1 本と、その位置（入れ子の Atom を辿って足した値）
struct LineRun {
  /// 行頭からの水平位置
  dx: Length,
  /// ベースラインからの縦位置（正で上）
  dy: Length,
  /// 送り幅
  width: Length,
  /// グリフ列
  run: GlyphRun,
}

/// 箱の中身を辿り、グリフ列を (`dx`, `dy`) だけずらした位置で `out` へ出現順に積む。
fn collect_line_runs(content: &HBoxContent, width: Length, dx: Length, dy: Length, out: &mut Vec<LineRun>) {
  match content {
    HBoxContent::Glyphs(run) => out.push(LineRun {
      dx,
      dy,
      width,
      run: run.clone(),
    }),
    HBoxContent::Atom(children) => {
      for child in children {
        collect_line_runs(&child.hbox.content, child.hbox.width, dx + child.dx, dy + child.dy, out);
      }
    },
    HBoxContent::Rule => {},
  }
}

/// 行・数式ブロックの中の罫 1 本と、その位置（入れ子の Atom を辿って足した値）
struct PlacedRule {
  /// 左端の水平位置
  dx: Length,
  /// 下端の縦位置（ベースライン基準・上が正）
  bottom: Length,
  /// 上端の縦位置（ベースライン基準・上が正）
  top: Length,
  /// 幅
  width: Length,
}

/// Atom の子を辿り、罫を (`dx`, `dy`) だけずらした位置で `out` へ出現順に積む。
fn collect_rules(content: &HBoxContent, dx: Length, dy: Length, out: &mut Vec<PlacedRule>) {
  let HBoxContent::Atom(children) = content else {
    return;
  };
  for child in children {
    let (x, y) = (dx + child.dx, dy + child.dy);
    if matches!(child.hbox.content, HBoxContent::Rule) {
      out.push(PlacedRule {
        dx: x,
        bottom: y - child.hbox.depth,
        top: y + child.hbox.height,
        width: child.hbox.width,
      });
    }
    collect_rules(&child.hbox.content, x, y, out);
  }
}

/// 本文 `source` を組版し、最初の行のグリフ列と罫を出現順に返す。
fn first_line_parts(source: &str) -> (Vec<LineRun>, Vec<PlacedRule>) {
  let laid_out = TestProject::builder().source_text(source).build().laid_out();
  let line = laid_out
    .pages
    .iter()
    .flat_map(|page| return page.blocks.iter())
    .find_map(|block| match block {
      PlacedBlock::Line { line, .. } => return Some(line),
      _ => return None,
    })
    .expect("本文の行が 1 つはあるはず");
  let mut runs = Vec::new();
  let mut rules = Vec::new();
  for placed in &line.boxes {
    collect_line_runs(&placed.hbox.content, placed.hbox.width, placed.dx, placed.dy, &mut runs);
    collect_rules(&placed.hbox.content, placed.dx, placed.dy, &mut rules);
  }
  return (runs, rules);
}

/// 本文 `source` を組版し、最初の行のグリフ列を出現順に返す。
fn first_line_runs(source: &str) -> Vec<LineRun> { return first_line_parts(source).0; }

/// 本文 `source` を組版し、最初の表示数式ブロックの本体のグリフ列と罫を、本体のベースライン基準で出現順に返す。
fn display_parts(source: &str) -> (Vec<LineRun>, Vec<PlacedRule>) {
  let laid_out = TestProject::builder().source_text(source).build().laid_out();
  let body = laid_out
    .pages
    .iter()
    .flat_map(|page| return page.blocks.iter())
    .find_map(|block| match block {
      PlacedBlock::MathBlock { body, .. } => return Some(body),
      _ => return None,
    })
    .expect("表示数式ブロックが 1 つあるはず");
  let mut runs = Vec::new();
  let mut rules = Vec::new();
  collect_line_runs(&body.content, body.width, Length::ZERO, Length::ZERO, &mut runs);
  collect_rules(&body.content, Length::ZERO, Length::ZERO, &mut rules);
  return (runs, rules);
}

/// グリフ列のインクの高さと深さ（どちらも 0 以上。グリフ列のベースライン基準）。
fn stix_ink_extent(line_run: &LineRun) -> (Length, Length) {
  let (top, bottom) = stix_run_ink(&line_run.run);
  return (top.max(Length::ZERO), (-bottom).max(Length::ZERO));
}

/// 分数の段のフォントサイズ `size` で、インクの深さ `depth` の分子を上げる量（MathML Core §3.3.2.1）。
fn expected_numerator_shift(size: Length, display: bool, depth: Length) -> Length {
  let (shift_up, gap_min) = if display {
    (MathConstant::FractionNumeratorDisplayStyleShiftUp, MathConstant::FractionNumDisplayStyleGapMin)
  } else {
    (MathConstant::FractionNumeratorShiftUp, MathConstant::FractionNumeratorGapMin)
  };
  let thickness = stix_math_length(MathConstant::FractionRuleThickness, size);
  return stix_math_length(shift_up, size)
    .max(stix_math_length(MathConstant::AxisHeight, size) + thickness / 2.0 + stix_math_length(gap_min, size) + depth);
}

/// 基底の段のフォントサイズ `size` で、インクの高さ `base_height` の基底に付く、インクの深さ `sup_depth` の上付きを
/// 上げる量（組版側の `ScriptConstants::superscript_shift` と同じ 3 規則の最大）。
fn expected_superscript_shift(size: Length, cramped: bool, base_height: Length, sup_depth: Length) -> Length {
  let standard = if cramped {
    MathConstant::SuperscriptShiftUpCramped
  } else {
    MathConstant::SuperscriptShiftUp
  };
  return stix_math_length(standard, size)
    .max(base_height - stix_math_length(MathConstant::SuperscriptBaselineDropMax, size))
    .max(stix_math_length(MathConstant::SuperscriptBottomMin, size) + sup_depth);
}

/// 分数の段のフォントサイズ `size` で、インクの高さ `height` の分母を下げる量（MathML Core §3.3.2.1）。
fn expected_denominator_shift(size: Length, display: bool, height: Length) -> Length {
  let (shift_down, gap_min) = if display {
    (
      MathConstant::FractionDenominatorDisplayStyleShiftDown,
      MathConstant::FractionDenomDisplayStyleGapMin,
    )
  } else {
    (MathConstant::FractionDenominatorShiftDown, MathConstant::FractionDenominatorGapMin)
  };
  let thickness = stix_math_length(MathConstant::FractionRuleThickness, size);
  return stix_math_length(shift_down, size).max(
    thickness / 2.0 + stix_math_length(gap_min, size) + height - stix_math_length(MathConstant::AxisHeight, size),
  );
}

/// `runs` からテキストが `text` の最初のグリフ列を返す。
fn run_with_text<'a>(runs: &'a [LineRun], text: &str) -> &'a LineRun {
  return runs
    .iter()
    .find(|line_run| return line_run.run.text == text)
    .unwrap_or_else(|| panic!("テキスト {text:?} のグリフ列があるはず"));
}

#[test]
fn script_levels_use_math_scale_down_and_ssty_glyphs() {
  let runs = first_line_runs("$2^{2^{2}}$\n");

  let twos: Vec<&LineRun> = runs.iter().filter(|line_run| return line_run.run.text == "2").collect();
  let [base, script, script_script] = twos.as_slice() else {
    panic!("2 が 3 段ぶん並ぶはず: {} 本", twos.len());
  };
  assert!(script.dx >= base.dx + base.width, "上付きは本体の右に置かれる");
  assert!(script.dy > base.dy, "上付きは本体より上に上がる");
  assert!(script_script.dy > base.dy, "さらに内側の上付きも本体より上にある");
  let size = base.run.font_size;
  assert_eq!(script.run.font_size, stix_scaled_size(MathConstant::ScriptPercentScaleDown, size));
  assert_eq!(script_script.run.font_size, stix_scaled_size(MathConstant::ScriptScriptPercentScaleDown, size));
  let glyphs: Vec<u32> = twos.iter().map(|line_run| return line_run.run.glyphs[0].gid).collect();
  assert!(
    glyphs[0] != glyphs[1] && glyphs[1] != glyphs[2] && glyphs[0] != glyphs[2],
    "段ごとに ssty の別の字形のはず: {glyphs:?}"
  );
}

/// 数学用イタリックの 𝑥（U+1D465）
const MATH_X: &str = "\u{1D465}";

/// 数学用イタリックの 𝑖（U+1D456）
const MATH_I: &str = "\u{1D456}";

#[test]
fn superscript_on_a_short_base_sits_at_the_font_shift() {
  let runs = first_line_runs("$x^{2}$\n");

  let base = run_with_text(&runs, MATH_X);
  let sup = run_with_text(&runs, "2");
  // 𝑥 のインクの高さ − SuperscriptBaselineDropMax も、上付きの底の下限も SuperscriptShiftUp に届かない
  // （箱の高さ＝ascender を使うと 762 − 230 で必ず上回るので、この一致はインクで測っていることも確かめる）
  assert_eq!(sup.dy - base.dy, stix_math_length(MathConstant::SuperscriptShiftUp, base.run.font_size));
  assert_eq!(sup.dx, base.dx + base.width, "上付きは基底の右端から始まる");
}

#[test]
fn subscript_on_a_short_base_sits_at_the_font_shift() {
  let runs = first_line_runs("$x_{i}$\n");

  let base = run_with_text(&runs, MATH_X);
  let sub = run_with_text(&runs, MATH_I);
  assert_eq!(base.dy - sub.dy, stix_math_length(MathConstant::SubscriptShiftDown, base.run.font_size));
}

#[test]
fn tall_base_pushes_the_superscript_up() {
  let runs = first_line_runs("$(a)^{2}$\n");

  let paren = run_with_text(&runs, ")");
  let sup = run_with_text(&runs, "2");
  assert!(
    sup.dy - paren.dy > stix_math_length(MathConstant::SuperscriptShiftUp, paren.run.font_size),
    "背の高い基底では 基底の高さ − SuperscriptBaselineDropMax が標準のシフトを上回る: dy={}",
    sup.dy
  );
}

#[test]
fn stacked_scripts_differ_by_the_italic_correction_in_either_order() {
  let placements = |source: &str| {
    let runs = first_line_runs(source);
    let sup = run_with_text(&runs, "2");
    let sub = run_with_text(&runs, MATH_I);
    let base = run_with_text(&runs, MATH_X);
    return (sup.dx, sup.dy, sub.dx, sub.dy, base.run.font_size, base.run.glyphs[0].gid);
  };

  let sub_first = placements("$x_{i}^{2}$\n");
  let sup_first = placements("$x^{2}_{i}$\n");

  assert_eq!(sub_first, sup_first, "書く順序によらず同じ配置");
  let (sup_dx, sup_dy, sub_dx, sub_dy, size, gid) = sub_first;
  assert_eq!(
    sup_dx - sub_dx,
    stix_units(stix_italics_correction(gid), size),
    "上付きは下付きより基底の補正ぶん右（傾いた基底）"
  );
  // STIX Two Math では上付きの底と下付きの頂のギャップが SubSuperscriptGapMin に足りず、両方が離れる
  assert!(sup_dy > stix_math_length(MathConstant::SuperscriptShiftUp, size), "上付きが上がる: {sup_dy}");
  assert!(-sub_dy > stix_math_length(MathConstant::SubscriptShiftDown, size), "下付きが下がる: {sub_dy}");
}

#[test]
fn space_after_script_follows_the_scripts() {
  let runs = first_line_runs("$x^{2}y$\n");

  let base = run_with_text(&runs, MATH_X);
  let sup = run_with_text(&runs, "2");
  let next = run_with_text(&runs, "\u{1D466}");
  assert_eq!(
    next.dx - (sup.dx + sup.width),
    stix_math_length(MathConstant::SpaceAfterScript, base.run.font_size),
    "スクリプトの後ろに SpaceAfterScript が空き、Atom の幅に残る"
  );
}

#[test]
fn script_in_a_non_math_font_is_placed_by_its_ink() {
  let runs = first_line_runs("$x^{あ}$\n");

  let base = run_with_text(&runs, MATH_X);
  let sup = run_with_text(&runs, "あ");
  assert_eq!(
    sup.run.font_type,
    FontType::JapaneseSerif,
    "和文は数式フォントではなく和文フォントで組まれる（テストが別フォントの経路を通っている）"
  );
  assert!(sup.dy - base.dy >= stix_math_length(MathConstant::SuperscriptShiftUp, base.run.font_size));
  assert_eq!(sup.dx, base.dx + base.width);
}

#[test]
fn slanted_glyph_before_an_operator_gets_its_italic_correction() {
  let runs = first_line_runs("$f(x)$\n");

  let (gid, advance) = sole_glyph(run_with_text(&runs, MATH_F));
  assert!(stix_italics_correction(gid) > 0, "𝑓 は補正を持つ（テストの前提）");
  assert_eq!(advance, stix_advance(gid) + stix_italics_correction(gid), "直立の ( の前で 𝑓 の補正が送りに入る");
}

#[test]
fn italic_correction_is_added_only_before_an_upright_glyph() {
  // 𝑥𝑦𝑎 は通常記号の並びで 1 本のグリフ列になる。𝑥 の次の 𝑦 は傾いた字形、𝑦 の次の 𝑎 は補正の登録が無い（直立扱い）
  let runs = first_line_runs("$xya$\n");

  let glyphs = &run_with_text(&runs, &format!("{MATH_X}{MATH_Y}{MATH_A}")).run.glyphs;
  let advances: Vec<i32> = glyphs.iter().map(|glyph| return glyph.x_advance).collect();
  let expected: Vec<i32> = vec![
    stix_advance(glyphs[0].gid),
    stix_advance(glyphs[1].gid) + stix_italics_correction(glyphs[1].gid),
    stix_advance(glyphs[2].gid),
  ];
  assert_eq!(advances, expected, "傾いた字形どうしの間は詰まり、直立の前と末尾だけ補正が入る");
}

#[test]
fn glyph_without_italic_correction_keeps_its_advance() {
  let runs = first_line_runs("$a+1$\n");

  let (gid, advance) = sole_glyph(run_with_text(&runs, MATH_A));
  assert_eq!(stix_italics_correction(gid), 0, "𝑎 は補正の登録が無い（テストの前提）");
  assert_eq!(advance, stix_advance(gid), "登録の無い字形は補正 0");
}

#[test]
fn operator_keeps_its_advance_despite_its_italic_correction() {
  let runs = first_line_runs("$\\int x$\n");

  let (gid, advance) = sole_glyph(run_with_text(&runs, "\u{222B}"));
  assert!(stix_italics_correction(gid) > 0, "∫ は補正を持つ（テストの前提）");
  assert_eq!(advance, stix_advance(gid), "演算子は傾いた字形として扱わず、補正を送りに足さない");
}

#[test]
fn script_content_gets_italic_correction_through_the_atom_path() {
  let runs = first_line_runs("$a^{x}$\n");

  let sup = run_with_text(&runs, MATH_X);
  let (gid, advance) = sole_glyph(sup);
  assert_eq!(advance, stix_advance(gid) + stix_italics_correction(gid), "上付きの中身の 𝑥 も末尾の補正を受ける");
  assert_eq!(sup.width, stix_units(advance, sup.run.font_size), "長さはスクリプト段のフォントサイズで縮む");
}

#[test]
fn subscript_on_a_slanted_base_sits_at_its_advance() {
  let runs = first_line_runs("$x_{2}$\n");

  let base = run_with_text(&runs, MATH_X);
  let sub = run_with_text(&runs, "2");
  let (gid, _) = sole_glyph(base);
  // 補正入りの幅と補正を別々に sp へ丸めるので 1sp までずれうる
  let expected = base.dx + stix_units(stix_advance(gid), base.run.font_size);
  assert!(
    (sub.dx - expected).abs().sp() <= 1,
    "下付きは補正を除いた基底の送り幅の位置: {} vs {expected}",
    sub.dx
  );
}

#[test]
fn large_operator_pulls_its_subscript_back_by_the_italic_correction() {
  let runs = first_line_runs("$\\int_{a}^{n}$\n");

  let base = run_with_text(&runs, "\u{222B}");
  let sup = run_with_text(&runs, "\u{1D45B}");
  let sub = run_with_text(&runs, MATH_A);
  let (gid, _) = sole_glyph(base);
  let correction = stix_units(stix_italics_correction(gid), base.run.font_size);
  assert_eq!(sup.dx, base.dx + base.width, "大型演算子の上付きは基底の右端");
  assert_eq!(sub.dx, base.dx + base.width - correction, "大型演算子の下付きは補正ぶん手前");
}

#[test]
fn scripts_on_an_upright_base_share_a_column() {
  let runs = first_line_runs("$a_{n}^{2}$\n");

  let base = run_with_text(&runs, MATH_A);
  let sup = run_with_text(&runs, "2");
  let sub = run_with_text(&runs, "\u{1D45B}");
  assert_eq!(sup.dx, base.dx + base.width);
  assert_eq!(sub.dx, sup.dx, "補正の登録が無い基底では上下付きが同じ列");
}

#[test]
fn scripts_on_an_empty_base_share_a_column() {
  let runs = first_line_runs("${}_{2}^{3}$\n");

  assert_eq!(run_with_text(&runs, "2").dx, run_with_text(&runs, "3").dx, "空の基底は補正 0");
}

#[test]
fn scripts_on_a_non_math_base_share_a_column() {
  let runs = first_line_runs("$あ_{2}^{3}$\n");

  assert_eq!(
    run_with_text(&runs, "あ").run.font_type,
    FontType::JapaneseSerif,
    "基底は和文フォント（テストの前提）"
  );
  assert_eq!(run_with_text(&runs, "2").dx, run_with_text(&runs, "3").dx, "数式フォント以外の基底は補正 0");
}

#[test]
fn base_ending_with_scripts_takes_no_italic_correction() {
  let runs = first_line_runs("${x^{2}}_{3}^{4}$\n");

  assert_eq!(
    run_with_text(&runs, "3").dx,
    run_with_text(&runs, "4").dx,
    "末尾がスクリプトの基底は、内側の字形の補正を外側の下付きに使わない"
  );
}

#[test]
fn cursor_after_scripts_follows_the_farther_script() {
  let runs = first_line_runs("$\\int_{abc}^{n}y$\n");

  let base = run_with_text(&runs, "\u{222B}");
  let sup = run_with_text(&runs, "\u{1D45B}");
  let sub = run_with_text(&runs, "\u{1D44E}\u{1D44F}\u{1D450}");
  let next = run_with_text(&runs, MATH_Y);
  let sup_end = sup.dx + sup.width;
  let sub_end = sub.dx + sub.width;
  assert!(
    sub_end > sup_end && sub_end > base.dx + base.width,
    "下付きの右端が基底・上付きより遠い（テストの前提）: {sub_end:?} / {sup_end:?}"
  );
  let space = stix_math_length(MathConstant::SpaceAfterScript, base.run.font_size);
  // ∫ は Op、y は Ord なので間に細アキ（3mu = 本文サイズの 3/18）が入る
  let thin_space = (base.run.font_size * 3) / 18.0f64;
  assert_eq!(next.dx, sub_end + space + thin_space, "後続は最も遠いスクリプトの右端 + SpaceAfterScript + 細アキ");
}

/// N-ARY SUMMATION（U+2211）
const SUM: &str = "\u{2211}";

/// N-ARY INTERSECTION（U+22C2）
const BIG_CAP: &str = "\u{22C2}";

/// INTEGRAL（U+222B）
const INTEGRAL: &str = "\u{222B}";

/// 本文 `source` の最初の行で、テキストが `text` のグリフ列（1 グリフ）の gid。
fn inline_gid(source: &str, text: &str) -> u32 { return sole_glyph(run_with_text(&first_line_runs(source), text)).0; }

#[test]
fn display_large_operator_grows_past_the_text_glyph_and_centers_on_the_math_axis() {
  for (command, symbol) in [("sum", SUM), ("bigcap", BIG_CAP), ("int", INTEGRAL)] {
    let (runs, _) = display_parts(&format!("\\begin{{equation}}\n\\{command} x\n\\end{{equation}}\n"));

    let operator = run_with_text(&runs, symbol);
    let (gid, _) = sole_glyph(operator);
    let size = run_with_text(&runs, MATH_X).run.font_size;
    assert_eq!(operator.run.font_size, size, "フォントサイズは変えずに字形を替える: {command}");
    assert_ne!(gid, inline_gid(&format!("$\\{command}$\n"), symbol), "text 段の字形より大きい字形: {command}");
    let (top, bottom) = stix_run_ink(&operator.run);
    let center = operator.dy + (top + bottom) / 2.0;
    let axis = stix_math_length(MathConstant::AxisHeight, size);
    assert!(
      (center - axis).abs() <= Length::from_sp(1),
      "インクの縦中央 {center:?} が数式軸 {axis:?} に載る: {command}"
    );
  }
}

#[test]
fn inline_large_operator_keeps_the_text_glyph_on_the_baseline() {
  let runs = first_line_runs("$\\sum x$\n");

  assert_eq!(
    run_with_text(&runs, SUM).dy,
    Length::ZERO,
    "text 段の大型演算子は軸に合わせず本文のベースラインのまま"
  );
}

#[test]
fn display_integral_pulls_its_subscript_back_by_the_display_glyph_correction() {
  let (runs, _) = display_parts("\\begin{equation}\n\\int_{a}^{n} x\n\\end{equation}\n");

  let base = run_with_text(&runs, INTEGRAL);
  let sup = run_with_text(&runs, "\u{1D45B}");
  let sub = run_with_text(&runs, MATH_A);
  let (gid, _) = sole_glyph(base);
  assert_ne!(gid, inline_gid("$\\int$\n", INTEGRAL), "display 段の字形（テストの前提）");
  let correction = stix_units(stix_italics_correction(gid), base.run.font_size);
  assert!(correction > Length::ZERO, "display 段の ∫ は補正を持つ（テストの前提）");
  assert_eq!(sup.dx, base.dx + base.width, "上付きは基底の右端");
  assert_eq!(sub.dx, base.dx + base.width - correction, "下付きは display の字形の補正ぶん手前");
}

#[test]
fn display_sum_stacks_its_limits_centered_above_and_below() {
  let (runs, _) = display_parts("\\begin{equation}\n\\sum_{i}^{n} x\n\\end{equation}\n");

  let operator = run_with_text(&runs, SUM);
  let over = run_with_text(&runs, "\u{1D45B}");
  let under = run_with_text(&runs, MATH_I);
  let size = operator.run.font_size;
  let (gid, _) = sole_glyph(operator);
  assert_eq!(stix_italics_correction(gid), 0, "∑ は補正の登録が無い（テストの前提）");
  let center = |line_run: &LineRun| return line_run.dx + line_run.width / 2.0;
  // 半分の幅を別々に sp へ丸めるので 1sp までずれうる
  assert!(
    (center(over) - center(operator)).abs() <= Length::from_sp(1),
    "補正の無い演算子の上限は演算子の中央"
  );
  assert!(
    (center(under) - center(operator)).abs() <= Length::from_sp(1),
    "補正の無い演算子の下限は演算子の中央"
  );
  let (top, bottom) = stix_run_ink(&operator.run);
  let (_, over_depth) = stix_ink_extent(over);
  let (under_height, _) = stix_ink_extent(under);
  let rise = stix_math_length(MathConstant::UpperLimitBaselineRiseMin, size)
    .max(stix_math_length(MathConstant::UpperLimitGapMin, size) + over_depth);
  let drop = stix_math_length(MathConstant::LowerLimitBaselineDropMin, size)
    .max(stix_math_length(MathConstant::LowerLimitGapMin, size) + under_height);
  assert_eq!(over.dy, operator.dy + top + rise, "上限のベースラインは演算子のインクの頂から上げる");
  assert_eq!(under.dy, operator.dy + bottom - drop, "下限のベースラインは演算子のインクの底から下げる");
}

#[test]
fn wide_lower_limit_widens_the_operator_and_pushes_the_next_atom() {
  let (runs, _) = display_parts("\\begin{equation}\n\\sum_{abc} x\n\\end{equation}\n");

  let operator = run_with_text(&runs, SUM);
  let under = run_with_text(&runs, "\u{1D44E}\u{1D44F}\u{1D450}");
  let next = run_with_text(&runs, MATH_X);
  assert!(under.width > operator.width, "下限が演算子より広い（テストの前提）");
  assert!(operator.dx > under.dx, "演算子は広い下限の中ほどに来る");
  // ∑ は Op、x は Ord なので間に細アキ（3mu）が入る。limits の後ろに SpaceAfterScript は置かない
  let thin_space = (operator.run.font_size * 3) / 18.0f64;
  assert_eq!(next.dx, under.dx + under.width + thin_space, "後続は下限の右端 + 細アキ");
}

#[test]
fn empty_limits_take_no_width_beyond_the_operator() {
  let (runs, _) = display_parts("\\begin{equation}\n\\sum_{}^{} x\n\\end{equation}\n");

  let operator = run_with_text(&runs, SUM);
  let next = run_with_text(&runs, MATH_X);
  let thin_space = (operator.run.font_size * 3) / 18.0f64;
  assert_eq!(next.dx, operator.dx + operator.width + thin_space, "空の範囲は幅を足さない");
}

#[test]
fn inline_sum_keeps_its_limits_at_the_shoulder() {
  let runs = first_line_runs("$\\sum_{i}^{n}$\n");

  let base = run_with_text(&runs, SUM);
  let sup = run_with_text(&runs, "\u{1D45B}");
  let sub = run_with_text(&runs, MATH_I);
  assert_eq!(sup.dx, base.dx + base.width, "インライン数式の上限は肩");
  assert_eq!(sub.dx, sup.dx, "インライン数式の下限は添字（補正 0 なので上付きと同じ列）");
}

#[test]
fn inline_fraction_stacks_script_size_parts_around_a_rule_on_the_math_axis() {
  let (runs, rules) = first_line_parts("$y\\frac{a}{b}$\n");

  let reference = run_with_text(&runs, MATH_Y);
  let numerator = run_with_text(&runs, MATH_A);
  let denominator = run_with_text(&runs, MATH_B);
  let [rule] = rules.as_slice() else {
    panic!("罫は 1 本のはず: {} 本", rules.len());
  };
  let size = reference.run.font_size;
  let thickness = stix_math_length(MathConstant::FractionRuleThickness, size);
  assert_eq!(
    numerator.run.font_size,
    stix_scaled_size(MathConstant::ScriptPercentScaleDown, size),
    "text 段の分子は script 段"
  );
  assert_eq!(denominator.run.font_size, numerator.run.font_size);
  assert_eq!(rule.top - rule.bottom, thickness);
  assert_eq!(
    rule.bottom - reference.dy,
    stix_math_length(MathConstant::AxisHeight, size) - thickness / 2.0,
    "罫の中心は数式軸"
  );
  let (_, numerator_depth) = stix_ink_extent(numerator);
  let (denominator_height, _) = stix_ink_extent(denominator);
  assert_eq!(numerator.dy - reference.dy, expected_numerator_shift(size, false, numerator_depth));
  assert_eq!(reference.dy - denominator.dy, expected_denominator_shift(size, false, denominator_height));
  assert_eq!(rule.width, numerator.width.max(denominator.width), "罫は分子・分母の幅の最大");
  assert_eq!(numerator.dx - rule.dx, ((rule.width - numerator.width) / 2.0f32).max(Length::ZERO), "分子は中央");
  assert_eq!(
    denominator.dx - rule.dx,
    ((rule.width - denominator.width) / 2.0f32).max(Length::ZERO),
    "分母は中央"
  );
  assert_eq!(rule.dx - (reference.dx + reference.width), FRACTION_PADDING, "罫の前に左のアキ");
}

#[test]
fn display_fraction_uses_display_style_constants_and_text_size_parts() {
  let (runs, rules) = display_parts("\\begin{equation}\nx\\frac{y}{d}\n\\end{equation}\n");

  let reference = run_with_text(&runs, MATH_X);
  let numerator = run_with_text(&runs, MATH_Y);
  let denominator = run_with_text(&runs, MATH_D);
  let [rule] = rules.as_slice() else {
    panic!("罫は 1 本のはず: {} 本", rules.len());
  };
  let size = reference.run.font_size;
  assert_eq!(numerator.run.font_size, size, "display 段の分子は text 段で縮めない");
  let (_, numerator_depth) = stix_ink_extent(numerator);
  let gap_rule = stix_math_length(MathConstant::AxisHeight, size)
    + stix_math_length(MathConstant::FractionRuleThickness, size) / 2.0
    + stix_math_length(MathConstant::FractionNumDisplayStyleGapMin, size)
    + numerator_depth;
  assert!(
    gap_rule > stix_math_length(MathConstant::FractionNumeratorDisplayStyleShiftUp, size),
    "𝑦 の深いインクでは display のギャップの下限が標準のシフトを上回る（入力の前提）"
  );
  assert_eq!(numerator.dy - reference.dy, gap_rule);
  let (denominator_height, _) = stix_ink_extent(denominator);
  assert_eq!(reference.dy - denominator.dy, expected_denominator_shift(size, true, denominator_height));
  assert_eq!(rule.top - rule.bottom, stix_math_length(MathConstant::FractionRuleThickness, size));
}

#[test]
fn fraction_in_a_display_superscript_uses_text_constants_at_script_size() {
  let (runs, rules) = display_parts("\\begin{equation}\nx^{\\frac{a}{b}}\n\\end{equation}\n");

  let reference = run_with_text(&runs, MATH_X);
  let numerator = run_with_text(&runs, MATH_A);
  let denominator = run_with_text(&runs, MATH_B);
  let [rule] = rules.as_slice() else {
    panic!("罫は 1 本のはず: {} 本", rules.len());
  };
  let size = reference.run.font_size;
  let script = stix_scaled_size(MathConstant::ScriptPercentScaleDown, size);
  assert_eq!(numerator.run.font_size, stix_scaled_size(MathConstant::ScriptScriptPercentScaleDown, size));
  assert_eq!(
    rule.top - rule.bottom,
    stix_math_length(MathConstant::FractionRuleThickness, script),
    "罫の太さは分数の段（script）の大きさ"
  );
  let (_, numerator_depth) = stix_ink_extent(numerator);
  let (denominator_height, _) = stix_ink_extent(denominator);
  assert_eq!(
    numerator.dy - denominator.dy,
    expected_numerator_shift(script, false, numerator_depth)
      + expected_denominator_shift(script, false, denominator_height),
    "上付きの中の分数は display の定数を使わない"
  );
}

#[test]
fn nested_fraction_draws_a_thinner_inner_rule() {
  let (runs, rules) = first_line_parts("$y\\frac{\\frac{a}{b}}{c}$\n");

  let size = run_with_text(&runs, MATH_Y).run.font_size;
  // 収集は出現順で、外側の分数の子は 分子（内側の分数）→ 分母 → 罫 の順
  let [inner, outer] = rules.as_slice() else {
    panic!("罫は 2 本のはず: {} 本", rules.len());
  };
  let script = stix_scaled_size(MathConstant::ScriptPercentScaleDown, size);
  assert_eq!(inner.top - inner.bottom, stix_math_length(MathConstant::FractionRuleThickness, script));
  assert_eq!(outer.top - outer.bottom, stix_math_length(MathConstant::FractionRuleThickness, size));
  let denominator = run_with_text(&runs, "\u{1D450}");
  assert_eq!(
    outer.width,
    (inner.width + FRACTION_PADDING + FRACTION_PADDING).max(denominator.width),
    "外側の罫は 内側の分数（両側のアキ込み）と分母 𝑐 の幅の最大"
  );
}

#[test]
fn adjacent_fraction_rules_do_not_touch() {
  let (_, rules) = first_line_parts("$\\frac{a}{b}\\frac{c}{d}$\n");

  let [first, second] = rules.as_slice() else {
    panic!("罫は 2 本のはず: {} 本", rules.len());
  };
  assert_eq!(
    second.dx - (first.dx + first.width),
    FRACTION_PADDING + FRACTION_PADDING,
    "左右のアキの和だけ離れる"
  );
}

#[test]
fn superscript_in_a_denominator_uses_the_cramped_shift() {
  let runs = first_line_runs("$\\frac{x^{2}}{x^{2}}$\n");

  let bases: Vec<&LineRun> = runs.iter().filter(|line_run| return line_run.run.text == MATH_X).collect();
  let sups: Vec<&LineRun> = runs.iter().filter(|line_run| return line_run.run.text == "2").collect();
  let ([numerator, denominator], [numerator_sup, denominator_sup]) = (bases.as_slice(), sups.as_slice()) else {
    panic!("𝑥 と 2 が分子・分母に 1 つずつのはず");
  };
  let size = numerator.run.font_size;
  // script 段の 𝑥 は ssty 字形（インク −10..517）。517 − SuperscriptBaselineDropMax 230 = 287 は cramped のシフト 252 と
  // 標準のシフト 360 の間なので、cramped かどうかで結果が分かれる
  let (base_height, _) = stix_ink_extent(numerator);
  let (_, sup_depth) = stix_ink_extent(numerator_sup);
  let normal = expected_superscript_shift(size, false, base_height, sup_depth);
  let cramped = expected_superscript_shift(size, true, base_height, sup_depth);
  assert_ne!(normal, cramped, "入力の前提: cramped かどうかでシフトが変わる");
  assert_eq!(numerator_sup.dy - numerator.dy, normal, "分子は cramped でない");
  assert_eq!(denominator_sup.dy - denominator.dy, cramped, "分母は cramped");
}

#[test]
fn fraction_rule_is_painted_as_a_filled_rect() {
  let compilation = TestProject::builder()
    .source_text("$y\\frac{a}{b}$\n")
    .build()
    .compile()
    .unwrap_or_else(|failure| panic!("分数の compile は成功するはず: {:?}", failure.into_report()));

  let ops: Vec<&PaintOp> = compilation.publication.pages().iter().flat_map(|page| return page.ops()).collect();
  let size = ops
    .iter()
    .find_map(|op| match op {
      PaintOp::DrawGlyphRun { run, .. } if run.text == MATH_Y => return Some(run.font_size),
      _ => return None,
    })
    .expect("𝑦 のグリフ列があるはず");
  let heights: Vec<f32> = ops
    .iter()
    .filter_map(|op| match op {
      PaintOp::FillRect { rect, color: None } => return Some(rect.height()),
      _ => return None,
    })
    .collect();
  assert!(
    heights.contains(&stix_math_length(MathConstant::FractionRuleThickness, size).to_pt()),
    "罫は太さ FractionRuleThickness の黒の塗りつぶし矩形になる: {heights:?}"
  );
}

#[test]
fn empty_fraction_compiles_with_a_zero_width_rule() {
  let (_, rules) = first_line_parts("$\\frac{}{}$\n");

  let [rule] = rules.as_slice() else {
    panic!("罫は 1 本のはず: {} 本", rules.len());
  };
  assert_eq!(rule.width, Length::ZERO);
  TestProject::builder()
    .source_text("$\\frac{}{}$\n")
    .build()
    .compile()
    .unwrap_or_else(|failure| panic!("空の分数も描画矩形を作れて compile は成功するはず: {:?}", failure.into_report()));
}

/// 根号記号 √（U+221A）
const RADICAL_SIGN: &str = "\u{221A}";

#[test]
fn radical_vinculum_continues_the_top_of_the_surd_over_the_radicand() {
  let (runs, rules) = first_line_parts("$\\sqrt{x}$\n");

  let surd = run_with_text(&runs, RADICAL_SIGN);
  let radicand = run_with_text(&runs, MATH_X);
  let [rule] = rules.as_slice() else {
    panic!("横線は 1 本のはず: {} 本", rules.len());
  };
  let size = radicand.run.font_size;
  let (surd_top, surd_bottom) = stix_run_ink(&surd.run);
  let thickness = stix_math_length(MathConstant::RadicalRuleThickness, size);
  assert_eq!(surd.run.font_size, size, "根号記号は被根号の段の大きさのまま縦にだけ伸ばす");
  assert_eq!(rule.top, surd.dy + surd_top, "横線の上端は根号記号のインクの上端");
  assert_eq!(rule.top - rule.bottom, thickness);
  assert_eq!(rule.dx, surd.dx + surd.width, "横線は記号の送り幅の位置から");
  assert_eq!(rule.width, radicand.width, "横線は被根号の幅");
  assert_eq!(radicand.dx, rule.dx);
  let (height, depth) = stix_ink_extent(radicand);
  let gap = stix_math_length(MathConstant::RadicalVerticalGap, size);
  let target = height + depth + gap + thickness;
  let excess = ((surd_top - surd_bottom) - target).max(Length::ZERO);
  assert!(excess.is_positive(), "元の √（1187 単位）は 𝑥 の目標を超える（入力の前提）");
  assert_eq!(rule.bottom - radicand.dy, height + gap + excess / 2.0, "余りの半分をギャップへ足す");
}

#[test]
fn tall_radicand_stretches_the_surd_to_cover_it() {
  let (inline_runs, _) = first_line_parts("$\\sqrt{x}$\n");
  let (runs, _) = display_parts("\\begin{equation}\n\\sqrt{\\frac{a}{b}}\n\\end{equation}\n");

  let natural = run_with_text(&inline_runs, RADICAL_SIGN);
  let surd = run_with_text(&runs, RADICAL_SIGN);
  let denominator = run_with_text(&runs, MATH_B);
  assert_ne!(
    surd.run.glyphs[0].gid, natural.run.glyphs[0].gid,
    "表示数式の分数を覆うには元の字形では足りず size variant か glyph assembly になる"
  );
  let (_, surd_bottom) = stix_run_ink(&surd.run);
  let (_, denominator_bottom) = stix_run_ink(&denominator.run);
  assert!(
    surd.dy + surd_bottom <= denominator.dy + denominator_bottom,
    "伸ばした記号は分母のインクの底まで届く"
  );
}

#[test]
fn display_radical_uses_the_display_gap() {
  let (runs, rules) = display_parts("\\begin{equation}\n\\sqrt{x}\n\\end{equation}\n");

  let surd = run_with_text(&runs, RADICAL_SIGN);
  let radicand = run_with_text(&runs, MATH_X);
  let [rule] = rules.as_slice() else {
    panic!("横線は 1 本のはず: {} 本", rules.len());
  };
  let size = radicand.run.font_size;
  let (surd_top, surd_bottom) = stix_run_ink(&surd.run);
  let (height, depth) = stix_ink_extent(radicand);
  let gap = stix_math_length(MathConstant::RadicalDisplayStyleVerticalGap, size);
  let target = height + depth + gap + stix_math_length(MathConstant::RadicalRuleThickness, size);
  let excess = ((surd_top - surd_bottom) - target).max(Length::ZERO);
  assert_eq!(rule.bottom - radicand.dy, height + gap + excess / 2.0);
}

#[test]
fn narrow_degree_sits_right_above_the_surd() {
  let runs = first_line_runs("$\\sqrt[3]{x}$\n");

  let degree = run_with_text(&runs, "3");
  let surd = run_with_text(&runs, RADICAL_SIGN);
  let size = run_with_text(&runs, MATH_X).run.font_size;
  assert_eq!(
    degree.run.font_size,
    stix_scaled_size(MathConstant::ScriptScriptPercentScaleDown, size),
    "指数は scriptscript 段"
  );
  assert!(
    stix_math_length(MathConstant::RadicalKernAfterDegree, size) < -degree.width,
    "STIX の後の kern（−335 単位）は 1 文字の指数の幅より負に大きい（入力の前提）"
  );
  assert_eq!(surd.dx, degree.dx, "後の kern は −指数の幅で止まり、記号は指数の左端から始まる");
}

#[test]
fn wide_degree_pushes_the_surd_right_by_the_kerns() {
  let runs = first_line_runs("$\\sqrt[100]{x}$\n");

  let degree = run_with_text(&runs, "100");
  let surd = run_with_text(&runs, RADICAL_SIGN);
  let size = run_with_text(&runs, MATH_X).run.font_size;
  let after = stix_math_length(MathConstant::RadicalKernAfterDegree, size);
  assert!(after > -degree.width, "3 文字の指数は後の kern より広い（入力の前提）");
  assert_eq!(surd.dx - degree.dx, degree.width + after);
}

#[test]
fn degree_bottom_rises_by_the_percent_of_the_radical_height() {
  let (runs, rules) = first_line_parts("$\\sqrt[3]{x}$\n");

  let degree = run_with_text(&runs, "3");
  let surd = run_with_text(&runs, RADICAL_SIGN);
  let radicand = run_with_text(&runs, MATH_X);
  let [rule] = rules.as_slice() else {
    panic!("横線は 1 本のはず: {} 本", rules.len());
  };
  let size = radicand.run.font_size;
  let (_, surd_bottom) = stix_run_ink(&surd.run);
  let (_, radicand_depth) = stix_ink_extent(radicand);
  let (_, degree_depth) = stix_ink_extent(degree);
  let ascent = rule.top - radicand.dy + stix_math_length(MathConstant::RadicalExtraAscender, size);
  let descent = radicand_depth.max(-(surd_bottom + surd.dy - radicand.dy));
  let (percent, _) = stix_math_constant(MathConstant::RadicalDegreeBottomRaisePercent);
  assert_eq!(
    degree.dy - radicand.dy,
    -descent + (ascent + descent).scale(f64::from(percent) / 100.0) + degree_depth,
    "指数のインクの底は 根号の下端 + 根号の高さ × RadicalDegreeBottomRaisePercent"
  );
}

#[test]
fn empty_radical_compiles_with_a_zero_width_vinculum() {
  let (_, rules) = first_line_parts("$\\sqrt{}$\n");

  let [rule] = rules.as_slice() else {
    panic!("横線は 1 本のはず: {} 本", rules.len());
  };
  assert_eq!(rule.width, Length::ZERO);
  TestProject::builder()
    .source_text("$\\sqrt{}$\n")
    .build()
    .compile()
    .unwrap_or_else(|failure| panic!("空の根号も描画矩形を作れて compile は成功するはず: {:?}", failure.into_report()));
}

#[test]
fn subscript_cuts_in_under_a_base_with_a_bottom_right_kern() {
  let runs = first_line_runs("$f_{n}$\n");

  let base = run_with_text(&runs, MATH_F);
  let sub = run_with_text(&runs, "\u{1D45B}");
  let (gid, _) = sole_glyph(base);
  let size = base.run.font_size;
  // 2 つの補正の高さ: 下付きのインクの頂・基底のインクの底（基底のベースライン基準）。𝑛 は TopLeft の表を持たない
  let sub_top = stix_run_ink(&sub.run).0 + (sub.dy - base.dy);
  let base_bottom = stix_run_ink(&base.run).1;
  let kern = [sub_top, base_bottom]
    .map(|height| return stix_math_kern(gid, MathKernCorner::BottomRight, stix_height_units(height, size)))
    .into_iter()
    .min()
    .expect("高さは 2 つある");
  assert!(kern < 0, "𝑓 の右下は下付きを潜り込ませる（テストの前提）: {kern}");
  assert_eq!(
    sub.dx,
    base.dx + base.width - stix_units(stix_italics_correction(gid), size) + stix_units(kern, size),
    "下付きは補正を除いた位置から math kern ぶんカットインする"
  );
}

#[test]
fn superscript_moves_by_the_top_right_kern_of_the_base() {
  let runs = first_line_runs("$W^{2}$\n");

  let base = run_with_text(&runs, "\u{1D44A}");
  let sup = run_with_text(&runs, "2");
  let (gid, _) = sole_glyph(base);
  let size = base.run.font_size;
  // 2 つの補正の高さ: 上付きのインクの底・基底のインクの頂。2 は BottomLeft の表を持たない
  let sup_bottom = stix_run_ink(&sup.run).1 + (sup.dy - base.dy);
  let base_top = stix_run_ink(&base.run).0;
  let kern = [sup_bottom, base_top]
    .map(|height| return stix_math_kern(gid, MathKernCorner::TopRight, stix_height_units(height, size)))
    .into_iter()
    .min()
    .expect("高さは 2 つある");
  assert_ne!(kern, 0, "𝑊 の右上は kern を持つ（テストの前提）");
  assert_eq!(sup.dx, base.dx + base.width + stix_units(kern, size), "上付きは基底の右端から math kern ぶん動く");
}

#[test]
fn subscript_kern_uses_the_top_left_table_of_the_script_glyph() {
  let runs = first_line_runs("$f_{x}$\n");

  let base = run_with_text(&runs, MATH_F);
  let sub = run_with_text(&runs, "\u{1D465}");
  let (base_gid, _) = sole_glyph(base);
  let (sub_gid, _) = sole_glyph(sub);
  let base_size = base.run.font_size;
  let sub_size = sub.run.font_size;
  // 下付きのベースラインは基底のベースラインから sub_shift_down 下
  let sub_shift_down = base.dy - sub.dy;
  let sub_top = stix_run_ink(&sub.run).0 + (sub.dy - base.dy);
  let base_bottom = stix_run_ink(&base.run).1;
  let kerns = [sub_top, base_bottom].map(|height| {
    let base_kern = stix_math_kern(base_gid, MathKernCorner::BottomRight, stix_height_units(height, base_size));
    let sub_kern =
      stix_math_kern(sub_gid, MathKernCorner::TopLeft, stix_height_units(height + sub_shift_down, sub_size));
    return (stix_units(base_kern, base_size), stix_units(sub_kern, sub_size));
  });
  assert!(
    kerns.iter().any(|(_, sub_kern)| return *sub_kern != Length::ZERO),
    "script 段の 𝑥 の左上は評価する高さのどれかで kern を持つ（テストの前提）: {kerns:?}"
  );
  let kern = kerns
    .iter()
    .map(|(base_kern, sub_kern)| return *base_kern + *sub_kern)
    .min()
    .expect("高さは 2 つある");
  assert_eq!(
    sub.dx,
    base.dx + base.width - stix_units(stix_italics_correction(base_gid), base_size) + kern,
    "下付きの kern は基底の右下と下付きの左上の和（それぞれのグリフの大きさで換算）の小さい方"
  );
}

/// 結合用サーカムフレックス（U+0302。`\hat` のアクセント記号）
const COMBINING_CIRCUMFLEX: &str = "\u{0302}";

/// STIX Two Math のグリフ `gid` の上付けアクセントの取付点（フォント単位。登録が無ければ `None`）。
#[expect(
  clippy::unwrap_in_result,
  reason = "vendor の STIX Two Math が MathTopAccentAttachment を読める形で持つことはテストの前提で、崩れたら panic で知らせる必要がある"
)]
fn stix_top_accent_attachment(gid: u32) -> Option<i32> {
  let font = stix_math_font();
  let info = font
    .tables()
    .math()
    .and_then(|math| return math.math_glyph_info())
    .expect("MathGlyphInfo を読めるはず");
  return info
    .math_top_accent_attachment()
    .expect("STIX Two Math は MathTopAccentAttachment を持つはず")
    .expect("MathTopAccentAttachment を読めるはず")
    .attachment(GlyphId::new(gid));
}

/// 1 グリフのグリフ列の、上付けアクセントの取付点（グリフ列の原点からの横位置。登録が無ければ送り幅の中央）。
fn expected_attachment(line_run: &LineRun) -> Length {
  let (gid, _) = sole_glyph(line_run);
  let size = line_run.run.font_size;
  return stix_top_accent_attachment(gid)
    .map_or_else(|| return stix_units(stix_advance(gid), size) / 2.0, |units| return stix_units(units, size));
}

#[test]
fn accent_aligns_its_attachment_with_the_attachment_of_a_single_glyph_base() {
  let runs = first_line_runs("$\\hat{x}$\n");

  let base = run_with_text(&runs, MATH_X);
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  let (base_gid, _) = sole_glyph(base);
  let (accent_gid, _) = sole_glyph(accent);
  assert!(stix_top_accent_attachment(base_gid).is_some(), "𝑥 は取付点を持つ（テストの前提）");
  assert!(
    stix_top_accent_attachment(accent_gid).is_some(),
    "結合用サーカムフレックスは取付点を持つ（テストの前提）"
  );
  assert_eq!(accent.run.font_size, base.run.font_size, "アクセント記号は基底の段の大きさ");
  assert_eq!(accent.dx + expected_attachment(accent), base.dx + expected_attachment(base), "取付点どうしを揃える");
  assert_eq!(
    accent.dy, base.dy,
    "𝑥 のインクの高さ（479）は AccentBaseHeight（480）以下なのでベースラインを揃える"
  );
}

#[test]
fn base_without_an_attachment_takes_half_of_its_advance() {
  let runs = first_line_runs("$\\hat{\\infty}$\n");

  let base = run_with_text(&runs, "\u{221E}");
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  let (base_gid, _) = sole_glyph(base);
  assert!(stix_top_accent_attachment(base_gid).is_none(), "∞ は取付点を持たない（テストの前提）");
  assert_eq!(
    accent.dx + expected_attachment(accent),
    base.dx + stix_units(stix_advance(base_gid), base.run.font_size) / 2.0,
    "登録の無いグリフは送り幅の中央（MATH の規定の既定値）"
  );
}

#[test]
fn accent_on_a_compound_base_centers_on_its_advance() {
  let runs = first_line_runs("$\\hat{xy}$\n");

  // 隣り合う Ord のテキストは lowering（`merge_adjacent_atom_text`）が 1 本の run に畳むので、run の分かれ方に依らず
  // アクセント以外のグリフ列の両端から基底の送り幅を測る
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  let base_runs: Vec<&LineRun> =
    runs.iter().filter(|line_run| return line_run.run.text != COMBINING_CIRCUMFLEX).collect();
  let (Some(first), Some(last)) = (base_runs.first(), base_runs.last()) else {
    panic!("基底のグリフ列があるはず");
  };
  assert_eq!(
    base_runs.iter().map(|line_run| return line_run.run.glyphs.len()).sum::<usize>(),
    2,
    "基底は 𝑥𝑦 の 2 グリフ（テストの前提）"
  );
  let base_width = last.dx + last.width - first.dx;
  assert_eq!(
    accent.dx + expected_attachment(accent),
    first.dx + base_width / 2.0,
    "複数グリフの基底は送り幅の中央"
  );
}

#[test]
fn accent_over_a_base_taller_than_accent_base_height_rises_by_the_excess() {
  let runs = first_line_runs("$\\hat{t}$\n");

  let base = run_with_text(&runs, "\u{1D461}");
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  let (height, _) = stix_ink_extent(base);
  let excess = height - stix_math_length(MathConstant::AccentBaseHeight, base.run.font_size);
  assert!(excess.is_positive(), "𝑡 のインクの高さ（593）は AccentBaseHeight（480）を超える（テストの前提）");
  assert_eq!(accent.dy - base.dy, excess);
}

#[test]
fn nested_accent_rises_over_the_inner_accent() {
  let runs = first_line_runs("$\\hat{\\hat{x}}$\n");

  let base = run_with_text(&runs, MATH_X);
  let accents: Vec<&LineRun> =
    runs.iter().filter(|line_run| return line_run.run.text == COMBINING_CIRCUMFLEX).collect();
  let [inner, outer] = accents.as_slice() else {
    panic!("アクセントは 2 つのはず: {} 個", accents.len());
  };
  let inner_top = stix_run_ink(&inner.run).0 + (inner.dy - base.dy);
  assert_eq!(
    outer.dy - base.dy,
    inner_top - stix_math_length(MathConstant::AccentBaseHeight, base.run.font_size),
    "外側は内側のアクセントの墨の頂を基底の高さとして上げる"
  );
}

#[test]
fn accent_does_not_widen_its_base() {
  let runs = first_line_runs("$\\hat{x}y$\n");

  let x = run_with_text(&runs, MATH_X);
  let y = run_with_text(&runs, MATH_Y);
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  assert!(accent.dx > x.dx + x.width, "結合記号の原点は 𝑥 の送り幅より右（テストの前提）");
  assert_eq!(y.dx, x.dx + x.width, "次のアトムは基底の送り幅の直後");
}

#[test]
fn superscript_clears_the_accent() {
  let runs = first_line_runs("$\\hat{x}^{2}$\n");

  let x = run_with_text(&runs, MATH_X);
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  let sup = run_with_text(&runs, "2");
  let accent_top = stix_run_ink(&accent.run).0 + (accent.dy - x.dy);
  let (_, sup_depth) = stix_ink_extent(sup);
  assert_eq!(
    sup.dy - x.dy,
    expected_superscript_shift(x.run.font_size, false, accent_top, sup_depth),
    "基底のインクの頂はアクセントの頂"
  );
}

#[test]
fn scripts_on_an_accent_start_at_the_base_advance() {
  for source in ["$\\hat{x}^{2}$\n", "$\\hat{x}_{2}$\n"] {
    let runs = first_line_runs(source);

    let x = run_with_text(&runs, MATH_X);
    let script = run_with_text(&runs, "2");
    assert_eq!(
      script.dx,
      x.dx + x.width,
      "アクセント付きの基底はグリフではないのでイタリック補正も math kern も 0: {source}"
    );
  }
}

#[test]
fn accent_in_a_script_is_set_at_the_script_size() {
  let runs = first_line_runs("$y^{\\hat{x}}$\n");

  let y = run_with_text(&runs, MATH_Y);
  let x = run_with_text(&runs, MATH_X);
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  assert_eq!(x.run.font_size, stix_scaled_size(MathConstant::ScriptPercentScaleDown, y.run.font_size));
  assert_eq!(accent.run.font_size, x.run.font_size, "アクセント記号は基底と同じ script 段の大きさ");
  assert_eq!(accent.dx + expected_attachment(accent), x.dx + expected_attachment(x));
}

#[test]
fn empty_accent_compiles() {
  TestProject::builder()
    .source_text("$\\hat{}$\n")
    .build()
    .compile()
    .unwrap_or_else(|failure| panic!("空の基底のアクセントも compile は成功するはず: {:?}", failure.into_report()));
}

#[test]
fn accent_over_a_base_taller_than_the_flattened_height_uses_the_flattened_glyph() {
  let natural = sole_glyph(run_with_text(&first_line_runs("$\\hat{x}$\n"), COMBINING_CIRCUMFLEX)).0;
  let medium = sole_glyph(run_with_text(&first_line_runs("$\\hat{t}$\n"), COMBINING_CIRCUMFLEX)).0;
  let runs = first_line_runs("$\\hat{f}$\n");

  let base = run_with_text(&runs, MATH_F);
  let accent = run_with_text(&runs, COMBINING_CIRCUMFLEX);
  let size = base.run.font_size;
  let (height, _) = stix_ink_extent(base);
  assert!(
    height > stix_math_length(MathConstant::FlattenedAccentBaseHeight, size),
    "𝑓 のインクの高さ（711）は FlattenedAccentBaseHeight（656）を超える（テストの前提）"
  );
  assert_eq!(medium, natural, "FlattenedAccentBaseHeight 以下の 𝑡（593）は元の字形");
  assert_ne!(sole_glyph(accent).0, natural, "超えた基底は平たい字形（flac）");
  assert_eq!(
    accent.dy - base.dy,
    height - stix_math_length(MathConstant::AccentBaseHeight, size),
    "上げる量は平たい字形でも同じ規則"
  );
  assert_eq!(
    accent.dx + expected_attachment(accent),
    base.dx + expected_attachment(base),
    "平たい字形も自身の取付点で揃える"
  );
}
