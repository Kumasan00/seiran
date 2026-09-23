//! 確定レイアウトの golden スナップショット回帰テスト
//!
//! fixture を組版した決定的テキストを `tests/golden/` と比較する。再生成は
//! `UPDATE_GOLDEN=1 cargo test -p seiran-compiler` で行う。検証手段の選択（layout dump golden と
//! PDF バイト比較の使い分け）は `.claude/skills/verify-typesetting/SKILL.md` が規定し、
//! 本モジュール内部のテスト分類はこの doc が正典。
//!
//! 入力は例外なく [`crate::compiler::test_support::TestProject`] が組み立て、production と同じ
//! `input::load` → frontend → semantics → font → typeset を通る。
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
//! `TestProject::layout` を使う。
//!
//! - **2 つの `typeset::Page` ダンプをテスト内で直接比較**（`assert_eq!` / `assert_ne!`）:
//!   [`index_marks_are_invisible_to_layout`]・style 差分 2 種
//!   [`layout_dump_changes_with_line_height`] / [`layout_dump_changes_with_punctuation_spacing`]・
//!   [`blank_code_line_keeps_a_full_line_height`]
//! - **`Page` / `PlacedBlock` へ直接アサート**（ダンプ関数は通らない）:
//!   [`keep_with_next_prevents_heading_orphan_end_to_end`]・
//!   [`index_group_heading_never_ends_a_column`]・
//!   脚注ページ単位採番 2 種 [`per_page_footnote_numbering_restarts_on_each_page`] /
//!   [`continuous_footnote_numbering_runs_through_pages`]（共通ヘルパ [`footnote_numbers_per_page`]
//!   経由）・[`index_entries_follow_the_page_the_content_lands_on`]・
//!   [`footnote_links_follow_the_page_the_line_lands_on`]・
//!   [`long_footnote_splits_across_pages_without_overlapping_body`]・
//!   [`figure_images_resolve_to_expected_display_sizes`]（`figure.sei` は golden 対象外なので、
//!   画像の確定描画寸法はここで固定する）・
//!   [`figure_image_without_size_fits_two_column_width_not_text_width`]（2 段組みでサイズ両省略の
//!   画像が `body_column_width` にフィットすることを固定し、本文パスの呼び出し元が段幅を
//!   `text_width` と取り違える退行を検出する）
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
  fs,
  path::{Path, PathBuf},
};

use crate::{
  compiler::{
    dump,
    test_support::{self, FIGURE_IMAGE_ASSETS, TestProject},
  },
  length::Length,
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
  "index_split",
  "itemize",
  "justify",
  "math_break",
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
  "theorem",
  "title_page",
  "toc",
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

  // 各入力を compile() 経由で組版し、Publication のダンプを golden と比較
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
/// 両方を要求する横断検証）でしか検出できない。`TestProject::layout` が将来
/// `CompilationInputs` を直接組み立てる経路へ戻ると、この診断が出なくなって失敗する。
#[test]
fn layout_helper_reports_cross_input_layout_validation() {
  // Arrange — 左右余白の合計（600mm）が fixture の用紙幅（595mm）を超える
  let project = TestProject::builder()
    .golden_fixture("text")
    .style_toml(|table| {
      test_support::set(table, "page", "margin_left", "300mm");
      test_support::set(table, "page", "margin_right", "300mm");
    })
    .build();

  // Act
  let Err(failure) = project.layout() else {
    panic!("横断検証に失敗するはず");
  };

  // Assert — 段名 wrapper ではなく leaf の診断がそのまま出る
  let codes: Vec<String> = failure
    .diagnostics()
    .map(|diagnostic| return diagnostic.code().expect("leaf 診断は code を持つはず").to_string())
    .collect();
  assert_eq!(codes, vec!["typeset::geometry::horizontal_margins".to_string()]);
}

/// 索引マーカーを除けば本文レイアウトが変わらないことを確認する。
#[test]
fn index_marks_are_invisible_to_layout() {
  // Act
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

  // Assert — 索引ページを除いた本文だけを比較する
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
  // Arrange — 版面を小さくして見出しがページ境界に当たりやすくする（見出し + 本文数行は空ページに
  // 収まる大きさ）。keepwithnext.sei は見出し直前を filler で埋め、見出しがページ末尾に来る配置。
  let project = TestProject::builder()
    .sources(&["tests/text/keepwithnext.sei"])
    .config_toml(|table| test_support::set(table, "pdf", "height", "45mm"))
    .style_toml(|table| {
      test_support::set(table, "page", "margin_top", "10mm");
      test_support::set(table, "page", "margin_bottom", "10mm");
    })
    .build();

  // Act
  let laid_out = project.laid_out();

  // Assert — 見出しがページ末尾に孤立せず、テストが複数ページを使っている
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
  let HBoxContent::Glyphs(run) = &single.content else {
    return None;
  };
  return LABELS.contains(&run.text.as_str()).then(|| return run.text.clone());
}

#[test]
fn index_group_heading_never_ends_a_column() {
  // Arrange — 索引が複数の段・ページへ分かれる小さな版面にする（段組みは style.index.column_count = 2）。
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

  // Act
  let laid_out = project.laid_out();

  // Assert — 見出し行の直後には必ず同じ段の中に次の行が来る（段が変わると baseline_y が上へ戻る）
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
  // Arrange — `figure.sei` は画像のサイズ指定 4 パターン（両指定 / width のみ / height のみ /
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

  // Act
  let sizes = figure_image_sizes();

  // Assert
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
  // Arrange — 2 段組みでは本文の 1 段あたりの幅（`body_column_width`）が単段の `text_width`
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

  // Act — 6 枚中サイズ両省略は testimage5（5 番目、index 4）
  assert_eq!(sizes.len(), 6, "画像は 6 枚あるはず: {sizes:?}");
  let (width, height) = sizes[4];

  // Assert — 段間 18pt（既定）を引いた 2 段組みの段幅（実測して固定した値）
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
  let HBoxContent::Atom(children) = &marker.content else {
    panic!("脚注マーカーは上付きの閉じた箱のはず: {marker:?}");
  };
  let text: String = children
    .iter()
    .filter_map(|child| match &child.item.content {
      HBoxContent::Glyphs(run) => return Some(run.text.as_str()),
      HBoxContent::Atom(_) => return None,
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
  // Act
  let per_page = footnote_numbers_per_page("per_page");

  // Assert — 脚注を持つページが 2 つ以上あり（空振りでないこと）、どのページも 1 から始まる連番。
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

/// 脚注本体のリンクが、その行が落ちたページのクリック矩形になることを end-to-end で確かめる（#515）
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
  // Act — 同じ入力を既定（通し）で組む
  let continuous = footnote_numbers_per_page("continuous");

  // Assert — ページをまたいでも 1 からの通し連番のまま（ページ単位採番の導入で既定が変わっていない）
  let flattened: Vec<u32> = continuous.iter().flatten().copied().collect();
  let expected: Vec<u32> = (1..=u32::try_from(flattened.len()).expect("脚注数は u32 に収まる")).collect();
  assert_eq!(flattened, expected, "通し採番はページをまたいで連番のはず: {continuous:?}");
}

#[test]
fn layout_dump_changes_with_line_height() {
  // Arrange — 行送り（line_height_factor）だけを変えた 2 スタイル。行送りは 2 行目以降の
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

  // Act
  let base_dump = dump_pages_of("itemize");
  let taller_dump = dump_pages(&taller.laid_out().pages);

  // Assert — レイアウトに影響する定数変更はダンプの差分として現れる
  assert_ne!(base_dump, taller_dump);
}

#[test]
fn layout_dump_changes_with_punctuation_spacing() {
  // Arrange — 和文約物アキ調整（JIS X 4051）の on/off だけを変えた 2 スタイル。
  // 約物が密な入力（yakumono）で連続約物の詰め・約物の収縮点化が座標差として現れる。
  let disabled = TestProject::builder()
    .golden_fixture("yakumono")
    .style_toml(|table| test_support::set(table, "text", "punctuation_spacing", false))
    .build();

  // Act — 既定（有効）と無効（フォントの送り幅そのまま）を組版してダンプする
  let enabled_dump = dump_pages_of("yakumono");
  let disabled_dump = dump_pages(&disabled.laid_out().pages);

  // Assert — 約物アキ調整はレイアウトを変える（無効化で従来出力へ戻せる）
  assert_ne!(enabled_dump, disabled_dump);
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
  // Arrange — 前付けなしの本文ページ数を基準にする
  let body_only = page_count_with_front_matter(false, false, false);

  // Act
  let title_only = page_count_with_front_matter(true, false, false);
  let toc_only = page_count_with_front_matter(false, true, false);
  let both = page_count_with_front_matter(true, true, false);
  let empty_title_only = page_count_with_front_matter(true, false, true);
  let empty_title_and_toc = page_count_with_front_matter(true, true, true);

  // Assert — タイトルページはちょうど 1 ページ、目次は 1 ページ以上、両方なら和。
  // 中身の無いタイトルページは 0 ページ（白紙を作らない）
  assert_eq!(title_only, body_only + 1, "タイトルページだけなら 1 ページ増える");
  assert!(toc_only > body_only, "目次だけなら 1 ページ以上増える: {toc_only} vs {body_only}");
  assert_eq!(both, toc_only + 1, "両方ならタイトルページ 1 + 目次のページ数");
  assert_eq!(empty_title_only, body_only, "中身の無いタイトルページは白紙ページを作らない");
  assert_eq!(empty_title_and_toc, toc_only, "中身の無いタイトルページは目次の前に白紙ページを作らない");
}
