//! 確定レイアウトの golden スナップショット回帰テスト
//!
//! `tests/text/` の各入力を [`super::build_pages`] で組版し、確定ページ列を
//! [`model::dump_pages`] で決定的テキストへ書き出して、`tests/golden/<name>.txt` と比較する。
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
use model::{AnchorMark, Length, Page, PlacedBlock, dump_pages};

use super::build_pages;

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
  "gather",
  "hyperref",
  "hyphenation",
  "itemize",
  "justify",
  "matrix",
  "multiline",
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
fn workspace_root() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran の 2 階層上がワークスペースルート")
    .to_path_buf();
}

/// golden ファイルを置くディレクトリ（`crates/seiran/tests/golden`）を返す。
fn golden_dir() -> PathBuf { return Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden"); }

/// カレントディレクトリをワークスペースルートへ固定する（相対パス解決を実ビルドに合わせる）。
fn enter_workspace_root() {
  std::env::set_current_dir(workspace_root()).expect("カレントディレクトリをワークスペースルートへ固定");
}

/// fixture の設定・スタイル・文献を読み込む（カレントディレクトリ = ワークスペースルート前提）。
///
/// fixture が参照するフォント・CSL は `vendor/` の取得済み資産に依存するため、未取得なら
/// 個々のフォント読込エラーではなく取得手順を先に案内して失敗させる。
fn load_base() -> (Config, Style, References) {
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

/// ページの末尾（最下部）ブロックが見出し行かどうかを返す。
///
/// 見出しは配置時にページに `PlacedAnchor { mark: Heading, y }`（見出し行の上端）を残す。ページの
/// 最終ブロックが `Line` で、その上端が見出しアンカーの y と一致すれば「見出しがページ末尾に孤立」。
fn page_ends_with_heading(page: &Page) -> bool {
  let Some(PlacedBlock::Line { line, baseline_y }) = page.blocks.last() else {
    return false;
  };
  let top = *baseline_y - line.height;
  return page
    .anchors
    .iter()
    .any(|anchor| matches!(anchor.mark, AnchorMark::Heading { .. }) && (anchor.y - top).abs() < Length::pt(0.5));
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
