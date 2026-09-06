//! compiler 配下のテストが共有する fixture プロジェクトの組み立て
//!
//! ここで組んだ `MemoryProjectSource` を production と同じ入口（`compile` /
//! `layout_project_for_test`）へ渡すので、テストも `input::load` の読込順序と横断検証を必ず通る。
//!
//! # パスの扱い
//!
//! 実バイト列はワークスペースルート基準の絶対パスで読み、`MemoryProjectSource` へは
//! `base_dir` を前置したキーで登録する。既定の `base_dir` は空パスなので、登録キーも
//! `config.toml` が解決したパスも `crates/seiran-compiler/tests/...` のようなワークスペース相対の
//! ままになる（診断に出るソース名が実行環境に依存しない）。`set_current_dir` は使わない。
//!
//! adapter 同値テストのように `FilesystemProjectSource` と同じ入力を要求する場合だけ
//! [`TestProjectBuilder::absolute_base_dir`] で `base_dir` をワークスペースルートにし、
//! 両 adapter が同じ絶対パスを引くようにする。
//!
//! 画像（`\image{...}` の字面）もソース・フォントと同じ規則で `base_dir` を前置したキーで登録する
//! （frontend が同じ規則で解決するため）。既定の `sources`（fixture config.toml の `cite.sei` +
//! `figure.sei`）を使う場合は `figure.sei` が参照する画像を builder が [`FIGURE_IMAGE_ASSETS`] として
//! 自動登録するので、呼び出し側の明示登録は不要。`sources` を [`TestProjectBuilder::sources`] で
//! 差し替えた場合はこの自動登録が働かないので、画像は [`TestProjectBuilder::asset`] /
//! [`TestProjectBuilder::assets`] で明示登録する。
//!
//! # 設定の上書き
//!
//! config.toml の差分は**生の TOML テーブルへ 1 回だけ**適用する（`ProjectConfig` は `Serialize` を
//! 持たず、production が実際に読む表現もこの TOML なので、型付きの並行実装を作らない）。
//! `Style` だけは型付きで書き換えてよいが、pipeline へは必ず `style.toml` として登録し
//! `input::load` に読み直させる。上書きが 1 つも無いファイルは実ファイルのテキストをそのまま
//! 登録する（再直列化を挟まない）。

use std::{
  collections::HashSet,
  fs,
  path::{Path, PathBuf},
};

use crate::{
  compiler::{self, Compilation, CompileFailure},
  length::Length,
  project::{MemoryProjectSource, ProjectPath},
  style::{self, FootnoteNumbering, RunningTemplate, Style},
  typeset::LaidOutDocument,
};

/// fixture の設定ファイル（ワークスペースルート相対）。
const CONFIG_REL: &str = "crates/seiran-compiler/tests/config/config.toml";

/// `figure.sei` が参照する画像 fixture（`\image{...}` の字面と同じ、ワークスペース相対）。
pub(super) const FIGURE_IMAGE_ASSETS: &[&str] = &[
  "./tests/image/testimage1.jpg",
  "./tests/image/testimage2.jpg",
  "./tests/image/testimage3.jpg",
  "./tests/image/testimage4.png",
  "./tests/image/testimage5.png",
  "./tests/image/testimage6.svg",
];

/// config.toml の生テーブルへの上書き。
type ConfigOverride = Box<dyn Fn(&mut toml::value::Table)>;

/// 型付き `Style` への上書き。
type StyleOverride = Box<dyn Fn(&mut Style)>;

/// ワークスペースルートを返す。
fn workspace_root() -> PathBuf {
  return Path::new(env!("CARGO_MANIFEST_DIR"))
    .ancestors()
    .nth(2)
    .expect("crates/seiran-compiler の 2 階層上がワークスペースルート")
    .to_path_buf();
}

/// `[section]` の `key` へ文字列値を設定する（テーブルが無ければ作る）。
pub(super) fn set_str(table: &mut toml::value::Table, section: &str, key: &str, value: &str) {
  let section_table = table.entry(section).or_insert_with(|| return toml::Value::Table(toml::value::Table::new()));
  section_table
    .as_table_mut()
    .unwrap_or_else(|| panic!("[{section}] はテーブルのはず"))
    .insert(key.to_string(), toml::Value::String(value.to_string()));
}

/// テストが `compile` / `layout_project_for_test` へ渡す fixture プロジェクト。
pub(super) struct TestProject {
  /// 登録済みの資源だけを持つ入力 seam
  source: MemoryProjectSource,
  /// `compile` の `root`（設定ファイルパス）
  config_path: ProjectPath,
  /// 相対パス解決の基準ディレクトリ
  base_dir: PathBuf,
  /// 登録したフォントの登録キー（重複除去済み）
  font_keys: Vec<PathBuf>,
}

impl TestProject {
  /// fixture の既定値から組み立てを始める。
  pub(super) fn builder() -> TestProjectBuilder { return TestProjectBuilder::new(); }

  /// production の公開入口をそのまま呼ぶ。
  ///
  /// # Errors
  ///
  /// コンパイルのいずれかの phase が失敗した場合にエラーを返す。
  pub(super) fn compile(&self) -> Result<Compilation, CompileFailure> {
    return compiler::compile(&self.source, &self.config_path, &self.base_dir);
  }

  /// 失敗することを期待して `compile` を呼び、その診断を返す。
  pub(super) fn compile_err(&self) -> CompileFailure {
    return match self.compile() {
      Ok(_) => panic!("このケースは失敗するはず"),
      Err(failure) => failure,
    };
  }

  /// 組版中間表現（`Publication` では失われる情報）を取り出す。
  ///
  /// # Errors
  ///
  /// 入力読込または組版までのいずれかの phase が失敗した場合にエラーを返す。
  pub(super) fn layout(&self) -> Result<LaidOutDocument, CompileFailure> {
    return compiler::layout_project_for_test(&self.source, &self.config_path, &self.base_dir);
  }

  /// 組版に成功することを期待して `layout` を呼ぶ。
  pub(super) fn laid_out(&self) -> LaidOutDocument {
    return self
      .layout()
      .unwrap_or_else(|failure| panic!("fixture の組版は成功するはず: {:?}", failure.into_report()));
  }

  /// 読込回数の検査に使う入力 seam。
  pub(super) fn memory_source(&self) -> &MemoryProjectSource { return &self.source; }

  /// `compile` へ渡している設定ファイルパス。
  pub(super) fn config_path(&self) -> &ProjectPath { return &self.config_path; }

  /// `compile` へ渡している相対パス解決の基準ディレクトリ。
  pub(super) fn base_dir(&self) -> &Path { return &self.base_dir; }

  /// 登録したフォントの登録キー（同じファイルを指す種別は 1 件に畳まれている）。
  pub(super) fn font_keys(&self) -> &[PathBuf] { return &self.font_keys; }
}

/// [`TestProject`] の組み立て（差分は宣言順に適用される）。
pub(super) struct TestProjectBuilder {
  /// 登録キーと相対パス解決の基準（既定は空パス＝ワークスペース相対のまま）
  base_dir: PathBuf,
  /// `sources` の差し替え（`None` なら fixture config.toml の値をそのまま使う）
  sources: Option<Vec<String>>,
  /// config.toml の生テーブルへの上書き
  config_overrides: Vec<ConfigOverride>,
  /// 型付き `Style` への上書き
  style_overrides: Vec<StyleOverride>,
  /// ワークスペース相対で書く資源（画像）。登録キーは他と同じく `base_dir` を前置する
  assets: Vec<PathBuf>,
}

impl TestProjectBuilder {
  /// 既定値（fixture の config.toml / style.toml / references.toml、`base_dir` は空パス）で始める。
  fn new() -> Self {
    return TestProjectBuilder {
      base_dir: PathBuf::new(),
      sources: None,
      config_overrides: Vec::new(),
      style_overrides: Vec::new(),
      assets: Vec::new(),
    };
  }

  /// `base_dir` をワークスペースルート（絶対パス）にする。
  ///
  /// `FilesystemProjectSource` と同じ入力を要求するテストだけが使う — 実 adapter は
  /// カレントディレクトリ非依存であるために絶対パスを必要とする。
  pub(super) fn absolute_base_dir(mut self) -> Self {
    self.base_dir = workspace_root();
    return self;
  }

  /// `sources` を差し替える（ワークスペースルート相対で書く）。
  pub(super) fn sources(mut self, sources: &[&str]) -> Self {
    self.sources = Some(sources.iter().map(|source| return (*source).to_string()).collect());
    return self;
  }

  /// config.toml の生テーブルへ差分を適用する。
  pub(super) fn config_toml(mut self, apply: impl Fn(&mut toml::value::Table) + 'static) -> Self {
    self.config_overrides.push(Box::new(apply));
    return self;
  }

  /// 型付き `Style` へ差分を適用する（TOML へ直列化して `style.toml` として登録される）。
  pub(super) fn style(mut self, apply: impl Fn(&mut Style) + 'static) -> Self {
    self.style_overrides.push(Box::new(apply));
    return self;
  }

  /// 画像等をワークスペース相対パスで登録する（`\image{...}` の字面と同じ文字列を渡す）。
  pub(super) fn asset(mut self, path: &str) -> Self {
    self.assets.push(PathBuf::from(path));
    return self;
  }

  /// 複数の資源をまとめて登録する（[`Self::asset`] の繰り返し）。
  pub(super) fn assets(mut self, paths: &[&str]) -> Self {
    for path in paths {
      self = self.asset(path);
    }
    return self;
  }

  /// golden fixture 名に対応する既定の入力（`sources` と機能有効化の差分）を適用する。
  pub(super) fn golden_fixture(self, name: &str) -> Self {
    let name = name.to_string();
    let fixture = name.clone();
    return self
      .sources(&[&format!("tests/text/{name}.sei")])
      .config_toml(move |table| apply_fixture_config_overrides(&fixture, table))
      .style(move |style| apply_fixture_style_overrides(&name, style));
  }

  /// 資源を登録した [`TestProject`] を組み立てる。
  pub(super) fn build(self) -> TestProject {
    let root = workspace_root();
    assert!(
      root.join("vendor/fonts").is_dir(),
      "golden テストの資産 vendor/ が未取得です。tools/fetch-test-assets.sh を実行してください"
    );

    let config_text = fs::read_to_string(root.join(CONFIG_REL)).expect("fixture config.toml を読めるはず");
    let mut table: toml::value::Table = config_text.parse().expect("fixture config.toml をパースできるはず");
    let mut config_changed = false;
    if let Some(sources) = &self.sources {
      table.insert(
        "sources".to_string(),
        toml::Value::Array(sources.iter().map(|source| return toml::Value::String(source.clone())).collect()),
      );
      config_changed = true;
    }
    for apply in &self.config_overrides {
      apply(&mut table);
      config_changed = true;
    }
    let config_text = if config_changed {
      toml::to_string(&table).expect("config.toml を再直列化できるはず")
    } else {
      config_text
    };

    let style_rel = string_field(&table, "style_path").expect("fixture config.toml は style_path を持つはず");
    let references_rel =
      string_field(&table, "references_path").expect("fixture config.toml は references_path を持つはず");
    let style_text = fs::read_to_string(root.join(&style_rel)).expect("fixture style.toml を読めるはず");
    let mut style = style::parse(&style_text, &style_rel).expect("fixture style.toml をパースできるはず");
    let style_text = if self.style_overrides.is_empty() {
      style_text
    } else {
      for apply in &self.style_overrides {
        apply(&mut style);
      }
      toml::to_string(&style).expect("Style を TOML へ再直列化できるはず")
    };

    let mut source = MemoryProjectSource::new()
      .with_text(self.key(CONFIG_REL), config_text)
      .with_text(self.key(&style_rel), style_text)
      .with_text(
        self.key(&references_rel),
        fs::read_to_string(root.join(&references_rel)).expect("fixture references.toml を読めるはず"),
      );

    // ソースは実在するものだけを登録する（欠落ソースの診断を検証するテストがあるため）
    for source_rel in string_array_field(&table, "sources") {
      if let Ok(text) = fs::read_to_string(root.join(&source_rel)) {
        source = source.with_text(self.key(&source_rel), text);
      }
    }

    let (source_with_fonts, font_keys) = self.register_fonts(source, &root, &table);
    source = source_with_fonts;

    for path in [
      style.reference.csl_path.as_ref(),
      style.reference.locale_path.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
      let bytes = fs::read(root.join(path)).unwrap_or_else(|error| panic!("CSL 資産を読めるはず: {path}: {error}"));
      // `style.toml` が持つ CSL パスも他の資源と同じくワークスペース相対なので、同じ規則で前置する
      source = source.with_bytes(self.key(path), bytes);
    }

    for asset in &self.assets_to_register() {
      let bytes =
        fs::read(root.join(asset)).unwrap_or_else(|error| panic!("資産を読めるはず: {}: {error}", asset.display()));
      source = source.with_bytes(self.key(asset), bytes);
    }

    return TestProject {
      source,
      config_path: ProjectPath::new(self.key(CONFIG_REL)),
      base_dir: self.base_dir,
      font_keys,
    };
  }

  /// `config.toml` の `[font_configs.*]` が指すフォントを実バイト列で登録する（同じパスは 1 回だけ）。
  ///
  /// 登録したキーも返す（同じフォントを複数回読まないことの検査に使う）。
  fn register_fonts(
    &self,
    mut source: MemoryProjectSource,
    root: &Path,
    table: &toml::value::Table,
  ) -> (MemoryProjectSource, Vec<PathBuf>) {
    let font_configs = table
      .get("font_configs")
      .and_then(|value| return value.as_table())
      .expect("fixture config.toml は [font_configs.*] を持つはず");
    let mut registered: HashSet<PathBuf> = HashSet::new();
    let mut keys: Vec<PathBuf> = Vec::new();
    for entry in font_configs.values() {
      let Some(font_path) =
        entry.as_table().and_then(|font| return font.get("font_path")).and_then(|v| return v.as_str())
      else {
        continue;
      };
      let key = self.key(font_path);
      if registered.insert(key.clone()) {
        let bytes =
          fs::read(root.join(font_path)).unwrap_or_else(|error| panic!("フォントを読めるはず: {font_path}: {error}"));
        source = source.with_bytes(&key, bytes);
        keys.push(key);
      }
    }
    return (source, keys);
  }

  /// 登録する資産の一覧（明示登録 `self.assets` に加え、`sources` が既定（fixture の
  /// `cite.sei` + `figure.sei`）のままなら `FIGURE_IMAGE_ASSETS` も足す。重複除去済み。
  ///
  /// `sources` を差し替えたテストは `figure.sei` を読まないことが多いので自動登録しない —
  /// 必要なら呼び出し側が [`Self::asset`] / [`Self::assets`] で明示する。
  fn assets_to_register(&self) -> Vec<PathBuf> {
    let mut assets = self.assets.clone();
    if self.sources.is_none() {
      assets.extend(FIGURE_IMAGE_ASSETS.iter().map(PathBuf::from));
    }
    let mut seen: HashSet<PathBuf> = HashSet::new();
    assets.retain(|path| return seen.insert(path.clone()));
    return assets;
  }

  /// ワークスペース相対パスから `MemoryProjectSource` の登録キーを作る。
  fn key(&self, relative: impl AsRef<Path>) -> PathBuf { return self.base_dir.join(relative); }
}

/// テーブル直下の文字列フィールドを取り出す。
fn string_field(table: &toml::value::Table, key: &str) -> Option<String> {
  return table.get(key).and_then(|value| return value.as_str()).map(str::to_string);
}

/// テーブル直下の文字列配列フィールドを取り出す。
fn string_array_field(table: &toml::value::Table, key: &str) -> Vec<String> {
  return table
    .get(key)
    .and_then(|value| return value.as_array())
    .map(|values| return values.iter().filter_map(|value| return value.as_str()).map(str::to_string).collect())
    .unwrap_or_default();
}

/// 検証対象の機能に必要な style 差分を fixture 名ごとに適用する。
///
/// ページ余白は style が所有する（#389）ので版面を変える上書きもここに置く。config 側の上書き
/// （[`apply_fixture_config_overrides`]）は用紙寸法と言語だけを扱う。
fn apply_fixture_style_overrides(name: &str, style: &mut Style) {
  match name {
    "title_page" => {
      style.title_page.enabled = true;
      style.header.left = RunningTemplate::parse("{title}");
      style.header.right = RunningTemplate::parse("{page} / {pages}");
      style.footer.center = RunningTemplate::parse("{page}");
    },
    "toc" => style.toc.enabled = true,
    // 索引のページ番号列を範囲表記へ畳む（既定は無効なので golden ではここで有効化する）
    "index_ranges" => style.index.collapse_page_ranges = true,
    // 索引へ区分見出し（五十音行・A–Z）を挟む（既定は無効なので golden ではここで有効化する）
    "index_groups" => style.index.group_headings = true,
    "hyphenation" => {
      style.page.margin_left = Length::mm(275.0);
      style.page.margin_right = Length::mm(275.0);
    },
    // 本文 2 段組みで左段・右段の両方に脚注が着地する版面（用紙寸法の縮小は config 側が持つ）
    "footnote_columns" => {
      style.columns.count = 2;
      style.page.margin_left = Length::mm(10.0);
      style.page.margin_right = Length::mm(10.0);
      style.page.margin_top = Length::mm(10.0);
      style.page.margin_bottom = Length::mm(10.0);
    },
    // ページ単位採番が複数ページにまたがる版面にする（用紙寸法の縮小は config 側が持つ）
    "footnote_per_page" => {
      style.footnote.numbering = FootnoteNumbering::PerPage;
      style.page.margin_left = Length::mm(20.0);
      style.page.margin_right = Length::mm(20.0);
      style.page.margin_top = Length::mm(15.0);
      style.page.margin_bottom = Length::mm(15.0);
    },
    // 1 個の脚注が収まらず繰越が連鎖する版面（`footnote_split`）と、そこへ長い表を足して
    // ページ跨ぎも起こす版面（`index_split`、索引語の出現ページ帰属の検証用）
    "footnote_split" | "index_split" => {
      style.page.margin_left = Length::mm(15.0);
      style.page.margin_right = Length::mm(15.0);
      style.page.margin_top = Length::mm(12.0);
      style.page.margin_bottom = Length::mm(12.0);
    },
    _ => {},
  }
}

/// 検証対象の機能に必要な config 差分を fixture 名ごとに適用する。
///
/// production が実際に読む表現へ揃えるため、生の TOML テーブルへ 1 回だけ適用する
/// （型付き `ProjectConfig` への並行実装は持たない）。
fn apply_fixture_config_overrides(name: &str, table: &mut toml::value::Table) {
  match name {
    "hyphenation" => set_str(table, "document", "language", "en"),
    // 本文 2 段組みで段の折返しが起きる版面にする（余白・段数は style の上書きが担う）
    "footnote_columns" => {
      set_str(table, "pdf", "width", "120mm");
      set_str(table, "pdf", "height", "60mm");
    },
    // ページ単位採番が複数ページにまたがる版面にする（余白側は style の上書きが担う）
    "footnote_per_page" => {
      set_str(table, "pdf", "width", "150mm");
      set_str(table, "pdf", "height", "130mm");
    },
    // 脚注の繰越（`footnote_split`）と、それに加えて表のページ跨ぎ（`index_split`）が起きる版面にする
    "footnote_split" | "index_split" => {
      set_str(table, "pdf", "width", "120mm");
      set_str(table, "pdf", "height", "85mm");
    },
    _ => {},
  }
}
