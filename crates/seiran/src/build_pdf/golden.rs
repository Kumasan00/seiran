//! 確定レイアウトの golden スナップショット回帰テスト
//!
//! `tests/text/` の各入力を [`super::build_pages`] で組版し、確定ページ列を
//! [`super::dump::dump_pages`] で決定的テキストへ書き出して、`tests/golden/<name>.txt` と比較する。
//! これにより PDF バイト列の非決定性（生成時刻・ID ハッシュ）を避けて、座標レベルの
//! レイアウト回帰を `cargo test` で検出できる。
//!
//! golden の再生成は `UPDATE_GOLDEN=1 cargo test -p seiran` で行い、差分は `git diff` で確認する。
//!
//! ## 入力の固定（fixture と vendor/）
//!
//! 設定・スタイル・文献はユーザローカルの `config/`（gitignore 対象）ではなく、コミット済みの
//! fixture（`crates/seiran/tests/config/`）を読む。fixture が参照するフォント・CSL・ロケールは
//! `tools/fetch-test-assets.sh` がピン留めハッシュ付きで `vendor/` へ取得する（未取得なら
//! このテストは取得手順を案内して失敗する）。golden の座標はフォントのバイト列に依存するため、
//! 入力をコミット済み fixture + ハッシュ検証済み資産に固定することで、環境やローカル設定の
//! 差異による偽陽性を防ぎ、CI でも同一の golden 比較が回る。
//!
//! ## カレントディレクトリについて
//!
//! fixture の `config.toml` / `style.toml` が参照する相対パス（フォント・CSL・ロケール等）は、
//! 実運用（リポジトリルートからの `cargo run`）と同じくプロセスのカレントディレクトリ基準で
//! 解決される。一方 `cargo test` の作業ディレクトリはパッケージ配下（`crates/seiran`）になるため、
//! 実ビルドを忠実に再現するようテスト冒頭でカレントディレクトリをワークスペースルートへ固定する。
//! seiran クレートの他テストはカレントディレクトリに依存しないため、同一値への固定は並行実行でも
//! 競合しない。

use std::{
  fs,
  path::{Path, PathBuf},
};

use citation::{References, read_references};
use config::{Config, Style};
use font::{FontData, FontDataExt};
use model::{AnchorMark, Length, Page, PlacedBlock};

use super::{build_pages, dump::dump_pages};

/// golden 比較対象の入力（`tests/text/` 配下の `.sei`、拡張子なし）。
///
/// `figure.sei` は外部画像ファイルの読込を伴い（`\image` のパスをカレントディレクトリ基準で
/// 解決して寸法を確定する）、レイアウトが画像実体に依存するため対象から除外する。
const GOLDEN_INPUTS: &[&str] = &[
  "align",
  "cases",
  "cite",
  "color",
  "equation",
  "footnote",
  "footnote_per_page",
  "footnote_split",
  "gather",
  "hyperref",
  "hyphenation",
  "index",
  "itemize",
  "justify",
  "matrix",
  "multiline",
  "pagebreak",
  "quote",
  "ref",
  "split",
  "table",
  "text",
  "theorem",
  "title_page",
  "toc",
  "yakumono",
];

/// ワークスペースルート（このクレート = `crates/seiran` の 2 階層上）を返す。
///
/// `diagnostics` テストモジュールも同じ fixture 前提（cwd 固定・fixture config 読込）を共有するため
/// `pub(super)` にして再利用する。
pub(super) fn workspace_root() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran の 2 階層上がワークスペースルート")
    .to_path_buf();
}

/// golden ファイルを置くディレクトリ（`crates/seiran/tests/golden`）を返す。
fn golden_dir() -> PathBuf { return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"); }

/// カレントディレクトリをワークスペースルートへ固定する（相対パス解決を実ビルドに合わせる）。
pub(super) fn enter_workspace_root() {
  std::env::set_current_dir(workspace_root()).expect("カレントディレクトリをワークスペースルートへ固定");
}

/// fixture の設定・スタイル・文献を読み込む（カレントディレクトリ = ワークスペースルート前提）。
///
/// fixture が参照するフォント・CSL は `vendor/` の取得済み資産に依存するため、未取得なら
/// 個々のフォント読込エラーではなく取得手順を先に案内して失敗させる。
pub(super) fn load_base() -> (Config, Style, References) {
  assert!(
    Path::new("vendor/fonts").is_dir(),
    "golden テストの資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
  );
  let config =
    config::read_config(Path::new("crates/seiran/tests/config/config.toml")).expect("fixture config.toml の読込");
  let style = config::read_style(config.style_path.as_deref()).expect("fixture style.toml の読込");
  let references = read_references(config.references_path.as_deref()).expect("fixture references の読込");
  return (config, style, references);
}

/// 入力ごとの style 差分を適用する。
///
/// 既定で無効の機能（タイトルページ・目次・ヘッダー / フッター）は、fixture で全入力に対して
/// 有効化すると全 golden にタイトルページ等が乗ってノイズになるため、その機能の検証用に
/// 書かれた入力に限ってここで有効化する。対象外の入力はベース style をそのまま使う。
fn apply_input_style_overrides(name: &str, style: &mut config::Style) {
  match name {
    // タイトルページ + ヘッダー / フッター（入力の本文が「ヘッダー・フッターはタイトルページには
    // 描画されず、本文ページから現れる」ことの検証を前提に書かれている）。前付けページの挿入で
    // page_numbering（前付け roman / 本文 arabic）の系列分離もここで踏む
    "title_page" => {
      style.title_page.enabled = true;
      style.header.left = "{title}".to_string();
      style.header.right = "{page} / {pages}".to_string();
      style.footer.center = "{page}".to_string();
    },
    // 目次（エントリ収集・リーダー・ページ番号・レベル別字下げ）
    "toc" => style.toc.enabled = true,
    // 脚注のページ単位採番（#226）。既定は通し番号なので、この入力だけ切り替える
    // （通し番号の golden は `footnote` が押さえる）。
    "footnote_per_page" => style.footnote.numbering = config::FootnoteNumbering::PerPage,
    _ => {},
  }
}

/// 入力ごとの config 差分を適用する。
///
/// 欧文ハイフネーション（#173）は文書ロケールから言語を導出するため、その検証入力だけ
/// `document.language` を英語に切り替える。あわせて本文幅を狭め（既定 fixture は版面が広く
/// 語中折り返しが起きない）、長い欧文語が語中でハイフン分割される様子を golden に固定する。
/// 対象外の入力はベース config をそのまま使う（既存 golden は language = "ja" のまま不変）。
fn apply_input_config_overrides(name: &str, config: &mut Config) {
  if name == "hyphenation" {
    config.document.language = Some("en".to_string());
    config.pdf.margin.left = Length::mm(275.0);
    config.pdf.margin.right = Length::mm(275.0);
  }
  // ページ単位採番は複数ページにまたがらないと検証にならない。既定 fixture の版面は
  // 1 ページが広大で改ページが起きないため、この入力だけ小さい紙面にする。
  if name == "footnote_per_page" {
    config.pdf.width = Length::mm(150.0);
    config.pdf.height = Length::mm(130.0);
    config.pdf.margin.left = Length::mm(20.0);
    config.pdf.margin.right = Length::mm(20.0);
    config.pdf.margin.top = Length::mm(15.0);
    config.pdf.margin.bottom = Length::mm(15.0);
  }
  // 長い脚注の繰越（#227）は、脚注 1 個が 1 ページの脚注領域に収まらないと起きない。
  // `footnote_per_page` よりさらに紙面を詰めて、繰越の連鎖が確実に起きるようにする。
  if name == "footnote_split" {
    config.pdf.width = Length::mm(120.0);
    config.pdf.height = Length::mm(85.0);
    config.pdf.margin.left = Length::mm(15.0);
    config.pdf.margin.right = Length::mm(15.0);
    config.pdf.margin.top = Length::mm(12.0);
    config.pdf.margin.bottom = Length::mm(12.0);
  }
}

/// 指定入力 1 つを組版し、確定ページ列のダンプ文字列を返す。
///
/// `sources` だけを対象入力へ差し替え、フォント・用紙はベース設定を共有する。style は
/// ベースに [`apply_input_style_overrides`] の入力別差分を重ねたものを使う。
fn dump_input(base_config: &Config, style: &Style, references: &References, name: &str) -> String {
  let mut config = base_config.clone();
  config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
  let mut style = style.clone();
  apply_input_style_overrides(name, &mut style);
  apply_input_config_overrides(name, &mut config);
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
  let laid_out = build_pages(&config, &style, references, &font_data).expect("build_pages の実行");
  return dump_pages(&laid_out.pages);
}

#[test]
fn layout_dumps_match_golden() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let update = std::env::var_os("UPDATE_GOLDEN").is_some();
  if update {
    fs::create_dir_all(golden_dir()).expect("golden ディレクトリの作成");
  }

  // Act / Assert — 各入力のダンプを golden と比較（UPDATE_GOLDEN=1 で再生成）
  let mut mismatches = Vec::new();
  for name in GOLDEN_INPUTS {
    let dump = dump_input(&base_config, &style, &references, name);
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

/// `\index` マーカーの有無だけを差分とする 2 つの入力（`index.sei` / `index_baseline.sei`）を
/// 比較し、`\index` を取り除いたソースとレイアウトが完全に一致することを確認する
/// （issue #246 の受け入れ条件）。`index.sei` 側のダンプから (a) `"index "` で始まる行（索引語
/// 自体の出力）、(b) 索引ページ生成が事後追加する内部リンクアンカー行
/// （`anchor mark=Label("index-page:...")`）、(c) 巻末に追加される索引ページ自体（issue #247）を
/// 除いた残りが、`index_baseline.sei`（`\index` を含まない、`GOLDEN_INPUTS` 非登録）のダンプと
/// 一致するかを見る。`index_baseline` はレイアウトが変わらないことの基準としてのみ使うため golden
/// ファイルは持たない。
#[test]
fn index_marks_are_invisible_to_layout() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();

  // Act
  let with_index = dump_input(&base_config, &style, &references, "index");
  let without_index = dump_input(&base_config, &style, &references, "index_baseline");
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
        && !line.starts_with("anchor mark=Label(\"index-page:");
    })
    .collect();
  let stripped = stripped_lines.iter().fold(String::new(), |mut acc, line| {
    acc.push_str(line);
    acc.push('\n');
    return acc;
  });

  // Assert — 本文ページ範囲だけを比較する（巻末索引ページ自体は issue #247 で新規に追加される内容
  // なので、本文と \index の有無を比較する本テストの対象外）
  assert_eq!(stripped, without_index, "\\index の有無で本文のレイアウトが変わってはならない");
}

/// ページの末尾（最下部）ブロックが見出し行かどうかを返す。
///
/// 見出しは配置時にページに `PlacedAnchor { mark: Heading, y }`（見出し行の上端）を残す。ページの
/// 最終ブロックが `Line` で、その上端が見出しアンカーの y と一致すれば「見出しがページ末尾に孤立」。
fn page_ends_with_heading(page: &Page) -> bool {
  let Some(PlacedBlock::Line { line, baseline_y }) = page.blocks.last() else {
    return false;
  };
  let top = *baseline_y - line.height;
  return page.anchors.iter().any(|anchor| {
    return matches!(anchor.mark, AnchorMark::Heading { .. }) && (anchor.y - top).abs() < Length::pt(0.5);
  });
}

#[test]
fn keep_with_next_prevents_heading_orphan_end_to_end() {
  // Arrange — 版面を小さくして見出しがページ境界に当たりやすくする（見出し + 本文数行は空ページに
  // 収まる大きさ）。keepwithnext.sei は見出し直前を filler で埋め、見出しがページ末尾に来る配置。
  enter_workspace_root();
  let (mut config, style, references) = load_base();
  config.pdf.height = Length::mm(45.0);
  config.pdf.margin.top = Length::mm(10.0);
  config.pdf.margin.bottom = Length::mm(10.0);
  config.sources = vec![PathBuf::from("tests/text/keepwithnext.sei")];
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");

  // Act
  let laid_out = build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");

  // Assert — どのページも見出しで終わらない（見出し直後の改ページ禁止・#168）。複数ページに分かれ、
  // 見出しがページ境界に絡むことでテストが空振りでないことも確認する。
  assert!(laid_out.pages.len() >= 2, "複数ページに分かれるはず: {} ページ", laid_out.pages.len());
  for (index, page) in laid_out.pages.iter().enumerate() {
    assert!(!page_ends_with_heading(page), "page {index} が見出しで終わっている（孤立）: {:#?}", page.blocks);
  }
}

/// `footnote_per_page.sei` を指定の採番方式で組版し、ページごとの脚注番号列を返すテストヘルパ
fn footnote_numbers_per_page(numbering: config::FootnoteNumbering) -> Vec<Vec<u32>> {
  enter_workspace_root();
  let (mut config, mut style, references) = load_base();
  style.footnote.numbering = numbering;
  config.sources = vec![PathBuf::from("tests/text/footnote_per_page.sei")];
  apply_input_config_overrides("footnote_per_page", &mut config);
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
  let laid_out = build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");
  return laid_out
    .pages
    .iter()
    .map(|page| return page.footnotes.iter().map(|footnote| return footnote.number).collect())
    .collect();
}

#[test]
fn per_page_footnote_numbering_restarts_on_each_page() {
  // Act
  let per_page = footnote_numbers_per_page(config::FootnoteNumbering::PerPage);

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

#[test]
fn long_footnote_splits_across_pages_without_overlapping_body() {
  // Arrange
  enter_workspace_root();
  let (mut config, style, references) = load_base();
  config.sources = vec![PathBuf::from("tests/text/footnote_split.sei")];
  apply_input_config_overrides("footnote_split", &mut config);
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");

  // Act
  let laid_out = build_pages(&config, &style, &references, &font_data).expect("build_pages の実行");

  // Assert — 脚注 1 が分割され、続きが次ページの脚注領域へ繰り越される（#227）
  let fragments: Vec<Vec<(u32, bool)>> = laid_out
    .pages
    .iter()
    .map(|page| return page.footnotes.iter().map(|f| return (f.number, f.continued)).collect())
    .collect();
  let carried = fragments
    .iter()
    .position(|page| return page.iter().any(|(_, continued)| return *continued))
    .unwrap_or_else(|| panic!("脚注が分割されて繰越が生じるはず（空振り検知）: {fragments:?}"));
  assert!(carried > 0, "繰越は 2 ページ目以降に現れるはず: {fragments:?}");
  // 繰越はそのページの脚注領域の先頭（自前の脚注より前）に置かれる
  assert_eq!(fragments[carried].first(), Some(&(1, true)), "繰越が脚注領域の先頭のはず: {fragments:?}");
  assert!(
    fragments[carried].iter().any(|(number, continued)| return *number == 2 && !continued),
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
fn block_top(block: &model::PlacedBlock) -> Option<Length> {
  return match block {
    model::PlacedBlock::Line { line, baseline_y } => Some(*baseline_y - line.height),
    model::PlacedBlock::Rule { y, .. } => Some(*y),
    _ => None,
  };
}

/// 配置済みブロックの下端（本文の重なり判定に使う。行のみを見る）
fn block_bottom(block: &model::PlacedBlock) -> Option<Length> {
  return match block {
    model::PlacedBlock::Line { line, baseline_y } => Some(*baseline_y + line.depth),
    _ => None,
  };
}

#[test]
fn continuous_footnote_numbering_runs_through_pages() {
  // Act — 同じ入力を既定（通し）で組む
  let continuous = footnote_numbers_per_page(config::FootnoteNumbering::Continuous);

  // Assert — ページをまたいでも 1 からの通し連番のまま（ページ単位採番の導入で既定が変わっていない）
  let flattened: Vec<u32> = continuous.iter().flatten().copied().collect();
  let expected: Vec<u32> = (1..=u32::try_from(flattened.len()).expect("脚注数は u32 に収まる")).collect();
  assert_eq!(flattened, expected, "通し採番はページをまたいで連番のはず: {continuous:?}");
}

#[test]
fn layout_dump_is_deterministic_across_builds() {
  // Arrange
  enter_workspace_root();
  let (base_config, style, references) = load_base();

  // Act — 同一入力・同一設定で 2 回組版してダンプする
  let first = dump_input(&base_config, &style, &references, "text");
  let second = dump_input(&base_config, &style, &references, "text");

  // Assert — パイプラインは決定的（時刻・乱数・環境依存値がレイアウトに混入しない）
  assert_eq!(first, second);
}

#[test]
fn layout_dump_changes_with_line_height() {
  // Arrange — 行送り（line_height_factor）だけを変えた 2 スタイル。行送りは 2 行目以降の
  // ベースライン送りに効くため、複数行が縦に並ぶ入力（itemize）を対象にする。
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut taller = style.clone();
  taller.text.line_height_factor += 0.5;

  // Act
  let base_dump = dump_input(&base_config, &style, &references, "itemize");
  let taller_dump = dump_input(&base_config, &taller, &references, "itemize");

  // Assert — レイアウトに影響する定数変更はダンプの差分として現れる
  assert_ne!(base_dump, taller_dump);
}

#[test]
fn layout_dump_changes_with_punctuation_spacing() {
  // Arrange — 和文約物アキ調整（JIS X 4051）の on/off だけを変えた 2 スタイル。
  // 約物が密な入力（yakumono）で連続約物の詰め・約物の収縮点化が座標差として現れる。
  enter_workspace_root();
  let (base_config, style, references) = load_base();
  let mut disabled = style.clone();
  disabled.text.punctuation_spacing = false;

  // Act — 既定（有効）と無効（フォントの送り幅そのまま）を組版してダンプする
  let enabled_dump = dump_input(&base_config, &style, &references, "yakumono");
  let disabled_dump = dump_input(&base_config, &disabled, &references, "yakumono");

  // Assert — 約物アキ調整はレイアウトを変える（無効化で従来出力へ戻せる）
  assert_ne!(enabled_dump, disabled_dump);
}
