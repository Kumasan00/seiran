//! PDF テキスト生成アプリケーション（Seiran）
//!
//! Seiran は、TeX スタイルのテキストファイルから高品質な PDF を生成する
//! コマンドラインアプリケーションです。
//!
//! ## 主要機能
//!
//! - **19 フォント種別対応**: Serif/Sans Serif/Monospace（各 4 種）+ Math + 日本語（6 種）
//! - **高度なテキストシェイピング**: `HarfRust` による OpenType フィーチャー対応
//! - **フォント最適化**: 使用グリフのみをサブセット化して PDF 出力（krilla が内部実施）
//! - **多言語対応**: Latin、日本語、その他の言語を自動判定
//! - **バリアブルフォント**: 可変軸の指定とインスタンス化
//!
//! ## 実行フロー
//!
//! 通常の PDF 生成フロー（`build` サブコマンド）：
//! 1. CLI パース → TOML 設定読み込み（config / style / references）
//! 2. 字句解析・構文解析 → 評価（Document IR）→ lowering（`LayoutNode`）
//! 3. フォント読み込み・検証 → シェーピング + 計測（`layout::build_blocks`）
//! 4. 画像サイズ確定（`pdf_gen::resolve_images`）→ 行分割・縦組版（`hlist::break_pages`）
//! 5. PDF 描画（`pdf_gen::create_pdf`）→ ファイル出力
//!
//! ## サブコマンド
//!
//! - `build [-c <config>]` - 設定ファイルの `sources` 配列から PDF を生成（メイン機能）
//! - `variation-axes <font>` - フォントのバリアブル軸情報を表示
//! - `ttc-names <ttc_file>` - TTC ファイル内のフォント名一覧を表示
//! - `script-langs <font>` - フォント対応の Script/Language タグを表示
//!
//! ## ロギング
//!
//! 既定では INFO レベル以上のトレーシングログを、RFC 3339 タイムスタンプ付きで標準エラー出力に
//! 記録します。観測・進捗の表示は `tracing` が担い、ユーザ向けの致命的エラーは `miette` 診断が
//! 担当します（役割を二重化しない）。
//!
//! ### レベル方針（taxonomy）
//!
//! - **ERROR**: 原則使わない。ユーザ向けの致命的エラーは `miette` 診断が担当し、`tracing` の
//!   ERROR と二重化しない。
//! - **WARN**: 処理は続行するが利用者が知るべき事象（フォントフォールバック、ソース拡張子の
//!   不一致、GSUB / GPOS テーブル欠如など）。
//! - **INFO**: 既定表示。build パイプラインの各ステージ完了とサマリ指標（件数・ページ数・所要
//!   時間 `elapsed_ms`）のみ。「実行を追える粒度」に限定する。
//! - **DEBUG**: 段の内部詳細（ソースごと・ブロックごと・ページごと・フォントごと等のループ
//!   単位）。各コア処理クレートが自段の完了を DEBUG で出す。
//! - **TRACE**: 最細粒度（トークン・グリフ単位）。現状は未使用（枠のみ定義）。
//!
//! ### 構造化フィールドの命名規約
//!
//! - パスは `<entity>_path`（例: `config_path` / `source_path` / `output_path`）。
//! - 件数は `<entity>_count`（例: `source_count` / `page_count` / `block_count`）。
//! - 時間はステージが `elapsed_ms`、ビルド全体が `total_elapsed_ms`。
//!
//! ### 冗長度の制御（別 issue）
//!
//! DEBUG / TRACE を実際に画面へ出す制御手段（`RUST_LOG` / `EnvFilter` や `-v` / `-q` フラグ）は
//! 本バージョンでは未導入で、既定では INFO のみを表示する（`with_max_level(INFO)` で固定）。
//! DEBUG / TRACE はコンパイルされるが既定では出力されない。

mod build_pdf;
use tracing_subscriber::fmt;

/// アプリケーションのメインエントリーポイント
///
/// ロギング（INFO 以上、RFC 3339 タイムスタンプ付きで stderr）を初期化し、
/// CLI 引数で指定されたサブコマンドを実行します。
///
/// # Errors
///
/// 設定読み込み・フォント検証・パース・PDF 生成の各段のエラーを
/// `miette` 診断として表示します。
fn main() -> miette::Result<()> {
  fmt::fmt()
    .pretty()
    .with_max_level(tracing::Level::INFO)
    .with_thread_ids(false)
    .with_thread_names(false)
    .with_target(false)
    .with_file(false)
    .with_timer(fmt::time::LocalTime::rfc_3339())
    .init();

  let cli_args = cli::parse_arg();

  match cli_args.command {
    cli::Command::Build { config_path } => build_pdf::build_pdf(&config_path)?,
    cli::Command::VariationAxes {
      font_path,
      font_index,
    } => {
      subcommand::get_variation_axes(&font_path, font_index)?;
    },
    cli::Command::TtcNames { ttc_file_path } => {
      subcommand::get_ttc_names(&ttc_file_path)?;
    },
    cli::Command::ScriptLangs {
      font_path,
      font_index,
    } => {
      subcommand::script_langs(&font_path, font_index)?;
    },
  }

  return Ok(());
}
