//! 確定レイアウトの golden スナップショット回帰テスト
//!
//! `tests/text/` の各入力を [`super::build_pages`] で組版し、確定ページ列を
//! [`hlist::dump_pages`] で決定的テキストへ書き出して、`tests/golden/<name>.txt` と比較する。
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

use font::{FontData, FontDataExt};
use hlist::dump_pages;

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
  "itemize",
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
fn load_base() -> (read_config::Config, read_style::Style, read_references::References) {
  assert!(
    Path::new("vendor/fonts").is_dir(),
    "golden テストの資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
  );
  let config =
    read_config::read_config(Path::new("crates/seiran/tests/config/config.toml")).expect("fixture config.toml の読込");
  let style = read_style::read_style(config.style_path.as_deref()).expect("fixture style.toml の読込");
  let references =
    read_references::read_references(config.references_path.as_deref()).expect("fixture references の読込");
  return (config, style, references);
}

/// 指定入力 1 つを組版し、確定ページ列のダンプ文字列を返す。
///
/// `sources` だけを対象入力へ差し替え、フォント・用紙・スタイルはベース設定を共有する。
fn dump_input(
  base_config: &read_config::Config,
  style: &read_style::Style,
  references: &read_references::References,
  name: &str,
) -> String {
  let mut config = base_config.clone();
  config.sources = vec![PathBuf::from(format!("tests/text/{name}.sei"))];
  let font_data = FontData::new(&config.font_configs).expect("フォントの読み込み");
  let laid_out = build_pages(&config, style, references, &font_data).expect("build_pages の実行");
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
